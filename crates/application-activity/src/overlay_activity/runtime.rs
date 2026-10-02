use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use vrcx_0_core::json::JsonExt;

use super::content::build_activity_content;
use super::definitions::{default_rule, known_definition_for_type, normalize_id};
use super::types::{
    OverlayActivityActorRelation, OverlayActivityCandidate, OverlayActivityDelivery,
    OverlayActivityEntry, OverlayActivityFavoriteGroupKeys, OverlayActivityFavoriteSubject,
    OverlayActivityFilters, OverlayActivityRule, OverlayActivityScope, OverlayActivitySnapshot,
    OverlayActivitySurface,
};

const DEFAULT_CAPACITY: usize = 128;
const DEDUP_CAPACITY: usize = 4096;
const DEDUP_TTL: Duration = Duration::from_secs(120);
const DELIVERY_LIVE_GRACE: chrono::Duration = chrono::Duration::seconds(5);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverlayFavoriteGroups {
    groups: HashMap<String, HashSet<String>>,
    all_favorites: HashSet<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct JoinedDeliveryCoverage {
    vr: bool,
    hmd: bool,
}

impl OverlayFavoriteGroups {
    pub fn from_map(groups: HashMap<String, Vec<String>>) -> Self {
        let mut normalized_groups = HashMap::new();
        let mut all_favorites = HashSet::new();
        for (group_key, user_ids) in groups {
            let group_key = normalize_id(&group_key);
            if group_key.is_empty() {
                continue;
            }
            let mut group = HashSet::new();
            for user_id in user_ids {
                let user_id = normalize_id(&user_id);
                if user_id.is_empty() {
                    continue;
                }
                all_favorites.insert(user_id.clone());
                group.insert(user_id);
            }
            if !group.is_empty() {
                normalized_groups.insert(group_key, group);
            }
        }
        Self {
            groups: normalized_groups,
            all_favorites,
        }
    }

    pub fn from_pairs<'a, I, U>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a [U])>,
        U: AsRef<str> + 'a,
    {
        let mut groups = HashMap::new();
        let mut all_favorites = HashSet::new();
        for (group_key, user_ids) in pairs {
            let group_key = normalize_id(group_key);
            if group_key.is_empty() {
                continue;
            }
            let mut group = HashSet::new();
            for user_id in user_ids {
                let user_id = normalize_id(user_id.as_ref());
                if user_id.is_empty() {
                    continue;
                }
                all_favorites.insert(user_id.clone());
                group.insert(user_id);
            }
            if !group.is_empty() {
                groups.insert(group_key, group);
            }
        }
        Self {
            groups,
            all_favorites,
        }
    }

    pub fn group_instance_notification_group_ids(
        &self,
        filters: &OverlayActivityFilters,
    ) -> Vec<String> {
        let mut group_ids = BTreeSet::new();
        for surface in [
            OverlayActivitySurface::Wrist,
            OverlayActivitySurface::Desktop,
            OverlayActivitySurface::Vr,
            OverlayActivitySurface::Hmd,
            OverlayActivitySurface::Webhook,
            OverlayActivitySurface::Tts,
        ] {
            let rule = filters.rule_for(surface, "group.instanceOpened");
            match rule.scope {
                OverlayActivityScope::AllFavorites => {
                    group_ids.extend(self.all_favorites.iter().cloned());
                }
                OverlayActivityScope::SelectedFavorites => {
                    if let OverlayActivityFavoriteGroupKeys::Selected(keys) =
                        rule.favorite_group_keys
                    {
                        for key in keys {
                            if let Some(selected) = self.groups.get(&normalize_id(&key)) {
                                group_ids.extend(selected.iter().cloned());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        group_ids.into_iter().collect()
    }

    fn contains_any(&self, subject_id: &str) -> bool {
        self.all_favorites.contains(subject_id)
    }

    fn contains_selected(&self, group_keys: &[String], subject_id: &str) -> bool {
        group_keys.iter().any(|group_key| {
            self.groups
                .get(group_key)
                .is_some_and(|group| group.contains(subject_id))
        })
    }
}

#[derive(Clone)]
pub struct OverlayActivityRuntime {
    pub(super) inner: Arc<OverlayActivityRuntimeInner>,
}

pub(super) struct OverlayActivityRuntimeInner {
    pub(super) state: Mutex<OverlayActivityState>,
    sink: Mutex<Option<Arc<dyn OverlayActivitySink>>>,
    group_notification_inputs_revision: AtomicU64,
}

pub trait OverlayActivitySink: Send + Sync {
    fn emit_overlay_activity_snapshot(&self, snapshot: OverlayActivitySnapshot);

    fn emit_overlay_activity_delivery(&self, _delivery: OverlayActivityDelivery) {}
}

#[derive(Clone, Debug)]
pub(super) struct OverlayActivityState {
    pub(super) filters: OverlayActivityFilters,
    pub(super) friend_favorite_groups: OverlayFavoriteGroups,
    pub(super) group_favorite_groups: OverlayFavoriteGroups,
    pub(super) friend_user_ids: HashSet<String>,
    location_hidden_user_ids: HashSet<String>,
    pub(super) group_instance_scope_key: String,
    pub(super) group_instance_baseline: HashMap<String, Vec<String>>,
    current_instance_location: String,
    current_instance_user_ids: HashSet<String>,
    joined_delivery_coverage: HashMap<(String, String), JoinedDeliveryCoverage>,
    pub(super) entries: VecDeque<OverlayActivityEntry>,
    pub(super) source_ids: HashSet<String>,
    pub(super) seen_order: VecDeque<(Instant, String)>,
    pub(super) next_sequence: u64,
    pub(super) capacity: usize,
    pub(super) dedup_capacity: usize,
    pub(super) live_since: Option<DateTime<Utc>>,
}

impl Default for OverlayActivityState {
    fn default() -> Self {
        Self {
            filters: OverlayActivityFilters::default(),
            friend_favorite_groups: OverlayFavoriteGroups::default(),
            group_favorite_groups: OverlayFavoriteGroups::default(),
            friend_user_ids: HashSet::new(),
            location_hidden_user_ids: HashSet::new(),
            group_instance_scope_key: String::new(),
            group_instance_baseline: HashMap::new(),
            current_instance_location: String::new(),
            current_instance_user_ids: HashSet::new(),
            joined_delivery_coverage: HashMap::new(),
            entries: VecDeque::new(),
            source_ids: HashSet::new(),
            seen_order: VecDeque::new(),
            next_sequence: 1,
            capacity: DEFAULT_CAPACITY,
            dedup_capacity: DEDUP_CAPACITY,
            live_since: None,
        }
    }
}

impl Default for OverlayActivityRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayActivityRuntime {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(OverlayActivityRuntimeInner {
                state: Mutex::new(OverlayActivityState::default()),
                sink: Mutex::new(None),
                group_notification_inputs_revision: AtomicU64::new(0),
            }),
        }
    }

    pub fn with_filters(filters: OverlayActivityFilters) -> Self {
        let runtime = Self::new();
        runtime.set_filters(filters);
        runtime
    }

    pub fn set_filters(&self, filters: OverlayActivityFilters) {
        let snapshot = {
            let Ok(mut state) = self.inner.state.lock() else {
                return;
            };
            if state.filters == filters {
                return;
            }
            let previous_group_ids = state
                .group_favorite_groups
                .group_instance_notification_group_ids(&state.filters)
                .into_iter()
                .collect::<HashSet<_>>();
            let next_group_ids = state
                .group_favorite_groups
                .group_instance_notification_group_ids(&filters)
                .into_iter()
                .collect::<HashSet<_>>();
            state.group_instance_baseline.retain(|group_id, _| {
                previous_group_ids.contains(group_id) && next_group_ids.contains(group_id)
            });
            state.filters = filters;
            state.entries.clear();
            state.source_ids.clear();
            state.seen_order.clear();
            state.joined_delivery_coverage.clear();
            snapshot_from_state(&state)
        };
        self.invalidate_group_notification_inputs();
        self.emit_snapshot(snapshot);
    }

    pub fn group_notification_inputs_revision(&self) -> u64 {
        self.inner
            .group_notification_inputs_revision
            .load(Ordering::Acquire)
    }

    pub fn invalidate_group_notification_inputs(&self) {
        self.inner
            .group_notification_inputs_revision
            .fetch_add(1, Ordering::AcqRel);
    }

    pub fn set_sink<S>(&self, sink: S)
    where
        S: OverlayActivitySink + 'static,
    {
        if let Ok(mut current) = self.inner.sink.lock() {
            *current = Some(Arc::new(sink));
        }
    }

    pub fn set_location_hidden_user_ids(&self, user_ids: HashSet<String>) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.location_hidden_user_ids = user_ids;
        }
    }

    pub fn set_favorite_groups(&self, favorite_groups: OverlayFavoriteGroups) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.friend_favorite_groups = favorite_groups;
        }
    }

    pub fn set_group_favorite_groups(&self, favorite_groups: OverlayFavoriteGroups) {
        if let Ok(mut state) = self.inner.state.lock() {
            if state.group_favorite_groups == favorite_groups {
                return;
            }
            let previous_group_ids = state
                .group_favorite_groups
                .group_instance_notification_group_ids(&state.filters)
                .into_iter()
                .collect::<HashSet<_>>();
            let next_group_ids = favorite_groups
                .group_instance_notification_group_ids(&state.filters)
                .into_iter()
                .collect::<HashSet<_>>();
            state.group_instance_baseline.retain(|group_id, _| {
                previous_group_ids.contains(group_id) && next_group_ids.contains(group_id)
            });
            state.group_favorite_groups = favorite_groups;
        }
    }

    pub fn set_friend_user_ids<I, S>(&self, user_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if let Ok(mut state) = self.inner.state.lock() {
            state.friend_user_ids = user_ids
                .into_iter()
                .map(|user_id| normalize_id(user_id.as_ref()))
                .filter(|user_id| !user_id.is_empty())
                .collect();
        }
    }

    pub fn set_current_instance_presence<I, S>(&self, location: &str, user_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let location = location.trim().to_string();
        let user_ids = user_ids
            .into_iter()
            .map(|user_id| normalize_id(user_id.as_ref()))
            .filter(|user_id| !user_id.is_empty())
            .collect::<HashSet<_>>();
        if let Ok(mut state) = self.inner.state.lock() {
            if state.current_instance_location != location {
                state.joined_delivery_coverage.clear();
            }
            state
                .joined_delivery_coverage
                .retain(|(user_id, joined_location), _| {
                    joined_location == &location && user_ids.contains(user_id)
                });
            state.current_instance_location = location;
            state.current_instance_user_ids = user_ids;
        }
    }

    pub fn set_delivery_armed(&self, armed: bool) {
        if let Ok(mut state) = self.inner.state.lock() {
            if armed {
                state.live_since.get_or_insert_with(Utc::now);
            } else {
                state.live_since = None;
                state.joined_delivery_coverage.clear();
            }
        }
    }

    pub fn clear_runtime_state(&self) {
        let snapshot = {
            let Ok(mut state) = self.inner.state.lock() else {
                return;
            };
            state.friend_favorite_groups = OverlayFavoriteGroups::default();
            state.group_favorite_groups = OverlayFavoriteGroups::default();
            state.friend_user_ids.clear();
            state.group_instance_scope_key.clear();
            state.group_instance_baseline.clear();
            state.current_instance_location.clear();
            state.current_instance_user_ids.clear();
            state.joined_delivery_coverage.clear();
            state.entries.clear();
            state.source_ids.clear();
            state.seen_order.clear();
            state.next_sequence = 1;
            state.live_since = None;
            snapshot_from_state(&state)
        };
        self.invalidate_group_notification_inputs();
        self.emit_snapshot(snapshot);
    }

    pub fn ingest_candidate(
        &self,
        candidate: OverlayActivityCandidate,
    ) -> Option<OverlayActivityEntry> {
        let (entry, snapshot, delivery) = {
            let mut state = self.inner.state.lock().ok()?;
            let definition = known_definition_for_type(&candidate.activity_type)?;

            let source_id = normalize_source_id(&candidate);
            if state.source_ids.contains(&source_id) {
                return None;
            }
            clear_joined_delivery_coverage_for_departing_gps(&mut state, &candidate);
            if candidate.activity_type == "GPS"
                && state
                    .location_hidden_user_ids
                    .contains(&normalize_id(&candidate.actor_user_id))
            {
                return None;
            }

            let wrist = surface_matches(
                &state,
                &candidate,
                OverlayActivitySurface::Wrist,
                definition,
            );
            let desktop = surface_matches(
                &state,
                &candidate,
                OverlayActivitySurface::Desktop,
                definition,
            );
            let mut vr =
                surface_matches(&state, &candidate, OverlayActivitySurface::Vr, definition);
            let mut hmd =
                surface_matches(&state, &candidate, OverlayActivitySurface::Hmd, definition);
            let webhook = surface_matches(
                &state,
                &candidate,
                OverlayActivitySurface::Webhook,
                definition,
            );
            let tts = surface_matches(&state, &candidate, OverlayActivitySurface::Tts, definition);
            let vr_suppressed = vr
                && suppresses_current_instance_gps(&state, &candidate, OverlayActivitySurface::Vr);
            if vr_suppressed {
                vr = false;
            }
            let hmd_suppressed = hmd
                && suppresses_current_instance_gps(&state, &candidate, OverlayActivitySurface::Hmd);
            if hmd_suppressed {
                hmd = false;
            }
            if !wrist && !desktop && !vr && !hmd && !webhook && !tts {
                if vr_suppressed || hmd_suppressed {
                    remember_source_id(&mut state, source_id);
                }
                return None;
            }
            remember_source_id(&mut state, source_id.clone());

            let actor_display_name = candidate.actor_display_name.trim().to_string();
            let content = build_activity_content(
                definition.key,
                definition.category,
                &candidate,
                &actor_display_name,
            );
            let actor_user_id = normalize_id(&candidate.actor_user_id);
            let actor_relation = actor_relation_for_user_id(&state, &actor_user_id);
            let entry = OverlayActivityEntry {
                sequence: state.next_sequence,
                source_id,
                activity_type: definition.key.to_string(),
                category: definition.category,
                created_at: candidate.created_at,
                actor_user_id,
                actor_display_name,
                content,
                actor_relation,
                payload: candidate.payload,
            };
            state.next_sequence = state.next_sequence.saturating_add(1);

            let snapshot = if wrist {
                state.entries.push_back(entry.clone());
                while state.entries.len() > state.capacity {
                    state.entries.pop_front();
                }
                Some(snapshot_from_state(&state))
            } else {
                None
            };

            let delivery_is_live = (desktop || vr || hmd || webhook || tts)
                && is_live_event(&state, &entry.created_at);
            let delivery = if delivery_is_live {
                remember_joined_delivery(&mut state, &entry, vr, hmd);
                Some(OverlayActivityDelivery {
                    entry: entry.clone(),
                    desktop,
                    vr,
                    hmd,
                    webhook,
                    tts,
                })
            } else {
                None
            };

            (entry, snapshot, delivery)
        };
        if let Some(snapshot) = snapshot {
            self.emit_snapshot(snapshot);
        }
        if let Some(delivery) = delivery {
            self.emit_delivery(delivery);
        }
        Some(entry)
    }

    pub fn snapshot(&self) -> OverlayActivitySnapshot {
        let Ok(state) = self.inner.state.lock() else {
            return OverlayActivitySnapshot::default();
        };
        snapshot_from_state(&state)
    }

    pub fn filters(&self) -> OverlayActivityFilters {
        self.inner
            .state
            .lock()
            .map(|state| state.filters.clone())
            .unwrap_or_default()
    }

    pub(super) fn insert_friend_user_id(&self, user_id: String) {
        if let Ok(mut state) = self.inner.state.lock() {
            let user_id = normalize_id(&user_id);
            if !user_id.is_empty() {
                state.friend_user_ids.insert(user_id);
            }
        }
    }

    pub(super) fn remove_friend_user_id(&self, user_id: &str) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.friend_user_ids.remove(&normalize_id(user_id));
        }
    }

    fn emit_snapshot(&self, snapshot: OverlayActivitySnapshot) {
        let sink = self.inner.sink.lock().ok().and_then(|sink| sink.clone());
        if let Some(sink) = sink {
            sink.emit_overlay_activity_snapshot(snapshot);
        }
    }

    fn emit_delivery(&self, delivery: OverlayActivityDelivery) {
        let sink = self.inner.sink.lock().ok().and_then(|sink| sink.clone());
        if let Some(sink) = sink {
            sink.emit_overlay_activity_delivery(delivery);
        }
    }
}

fn surface_matches(
    state: &OverlayActivityState,
    candidate: &OverlayActivityCandidate,
    surface: OverlayActivitySurface,
    definition: &super::definitions::ActivityTypeDefinition,
) -> bool {
    let fallback = default_rule(definition);
    let rule = state
        .filters
        .surface(surface)
        .types
        .get(definition.key)
        .unwrap_or(&fallback);
    candidate_matches_rule(state, candidate, rule)
}

fn suppresses_current_instance_gps(
    state: &OverlayActivityState,
    candidate: &OverlayActivityCandidate,
    surface: OverlayActivitySurface,
) -> bool {
    if candidate.activity_type != "GPS" {
        return false;
    }
    if state.filters.rule_for(surface, "GPS").scope != OverlayActivityScope::SelectedFavorites
        || !surface_filters_joined_friends(state, surface)
    {
        return false;
    }
    let location = candidate.payload.trimmed_text("location");
    let Some(key) = current_instance_friend_key(state, &candidate.actor_user_id, &location) else {
        return false;
    };
    let Some(coverage) = state.joined_delivery_coverage.get(&key) else {
        return false;
    };
    match surface {
        OverlayActivitySurface::Vr => coverage.vr,
        OverlayActivitySurface::Hmd => coverage.hmd,
        _ => false,
    }
}

fn clear_joined_delivery_coverage_for_departing_gps(
    state: &mut OverlayActivityState,
    candidate: &OverlayActivityCandidate,
) {
    if candidate.activity_type != "GPS" {
        return;
    }
    let user_id = normalize_id(&candidate.actor_user_id);
    let location = candidate.payload.trimmed_text("location");
    if user_id.is_empty() || location.is_empty() || location == state.current_instance_location {
        return;
    }
    state
        .joined_delivery_coverage
        .retain(|(covered_user_id, _), _| covered_user_id != &user_id);
}

fn remember_joined_delivery(
    state: &mut OverlayActivityState,
    entry: &OverlayActivityEntry,
    vr: bool,
    hmd: bool,
) {
    if entry.activity_type != "OnPlayerJoined" {
        return;
    }
    let Some(key) =
        current_instance_friend_key(state, &entry.actor_user_id, &entry.content.location)
    else {
        return;
    };
    let vr = vr && surface_filters_joined_friends(state, OverlayActivitySurface::Vr);
    let hmd = hmd && surface_filters_joined_friends(state, OverlayActivitySurface::Hmd);
    if !vr && !hmd {
        return;
    }
    let coverage = state.joined_delivery_coverage.entry(key).or_default();
    coverage.vr |= vr;
    coverage.hmd |= hmd;
}

fn current_instance_friend_key(
    state: &OverlayActivityState,
    actor_user_id: &str,
    location: &str,
) -> Option<(String, String)> {
    let user_id = normalize_id(actor_user_id);
    let location = location.trim().to_string();
    if user_id.is_empty()
        || location.is_empty()
        || location != state.current_instance_location
        || !state.friend_user_ids.contains(&user_id)
        || !state.current_instance_user_ids.contains(&user_id)
    {
        return None;
    }
    Some((user_id, location))
}

fn surface_filters_joined_friends(
    state: &OverlayActivityState,
    surface: OverlayActivitySurface,
) -> bool {
    state.filters.rule_for(surface, "OnPlayerJoined").scope == OverlayActivityScope::Friends
}

fn remember_source_id(state: &mut OverlayActivityState, source_id: String) {
    if state.source_ids.insert(source_id.clone()) {
        let now = Instant::now();
        state.seen_order.push_back((now, source_id));
        while let Some((seen_at, _)) = state.seen_order.front() {
            let expired = now.duration_since(*seen_at) > DEDUP_TTL;
            let over_capacity = state.seen_order.len() > state.dedup_capacity;
            if !expired && !over_capacity {
                break;
            }
            if let Some((_, removed)) = state.seen_order.pop_front() {
                state.source_ids.remove(&removed);
            }
        }
    }
}

fn is_live_event(state: &OverlayActivityState, created_at: &str) -> bool {
    let Some(live_since) = state.live_since else {
        return false;
    };
    let trimmed = created_at.trim();
    if trimmed.is_empty() {
        return true;
    }
    match DateTime::parse_from_rfc3339(trimmed) {
        Ok(timestamp) => timestamp.with_timezone(&Utc) >= live_since - DELIVERY_LIVE_GRACE,
        Err(_) => true,
    }
}

fn snapshot_from_state(state: &OverlayActivityState) -> OverlayActivitySnapshot {
    OverlayActivitySnapshot {
        entries: state
            .entries
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.actor_relation = actor_relation_for_user_id(state, &entry.actor_user_id);
                entry
            })
            .collect(),
    }
}

fn actor_relation_for_user_id(
    state: &OverlayActivityState,
    actor_user_id: &str,
) -> OverlayActivityActorRelation {
    let actor_user_id = normalize_id(actor_user_id);
    if actor_user_id.is_empty() {
        return OverlayActivityActorRelation::None;
    }
    if state.friend_favorite_groups.contains_any(&actor_user_id) {
        return OverlayActivityActorRelation::Favorite;
    }
    if state.friend_user_ids.contains(&actor_user_id) {
        return OverlayActivityActorRelation::Friend;
    }
    OverlayActivityActorRelation::None
}

fn candidate_matches_rule(
    state: &OverlayActivityState,
    candidate: &OverlayActivityCandidate,
    rule: &OverlayActivityRule,
) -> bool {
    let actor_user_id = normalize_id(&candidate.actor_user_id);
    let favorite_membership = match &candidate.favorite_subject {
        OverlayActivityFavoriteSubject::None => None,
        OverlayActivityFavoriteSubject::UserId(user_id) => {
            Some((&state.friend_favorite_groups, normalize_id(user_id)))
        }
        OverlayActivityFavoriteSubject::GroupId(group_id) => {
            Some((&state.group_favorite_groups, normalize_id(group_id)))
        }
    };
    match rule.scope {
        OverlayActivityScope::Off => false,
        OverlayActivityScope::On => true,
        OverlayActivityScope::Friends => state.friend_user_ids.contains(&actor_user_id),
        OverlayActivityScope::SelectedFavorites => match &rule.favorite_group_keys {
            OverlayActivityFavoriteGroupKeys::All => favorite_membership
                .as_ref()
                .is_some_and(|(groups, subject_id)| groups.contains_any(subject_id)),
            OverlayActivityFavoriteGroupKeys::Selected(group_keys) => favorite_membership
                .as_ref()
                .is_some_and(|(groups, subject_id)| {
                    groups.contains_selected(group_keys, subject_id)
                }),
        },
        OverlayActivityScope::AllFavorites => favorite_membership
            .as_ref()
            .is_some_and(|(groups, subject_id)| groups.contains_any(subject_id)),
        OverlayActivityScope::EveryoneInInstance => candidate.current_instance,
    }
}

fn normalize_source_id(candidate: &OverlayActivityCandidate) -> String {
    let source_id = candidate.source_id.trim();
    if source_id.is_empty() {
        format!(
            "{}:{}:{}",
            candidate.activity_type.trim(),
            candidate.actor_user_id.trim(),
            candidate.created_at.trim()
        )
    } else {
        source_id.to_string()
    }
}
