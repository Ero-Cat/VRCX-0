use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use vrcx_0_contracts::activity::{ActivityEvent, ActivityFacts, ActivityKind, ActivitySubject};
use vrcx_0_core::json::{JsonExt, RawJson};

#[derive(Clone, Default)]
pub struct GroupInstanceMonitor {
    state: Arc<Mutex<GroupInstanceMonitorState>>,
}

#[derive(Default)]
struct GroupInstanceMonitorState {
    scope_key: String,
    watched_group_ids: HashSet<String>,
    baseline: HashMap<String, Vec<String>>,
}

impl GroupInstanceMonitor {
    pub fn set_watched_groups(&self, group_ids: &[String]) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let next = group_ids.iter().cloned().collect::<HashSet<_>>();
        let previous = std::mem::replace(&mut state.watched_group_ids, next);
        let watched = state.watched_group_ids.clone();
        state
            .baseline
            .retain(|group_id, _| previous.contains(group_id) && watched.contains(group_id));
    }

    pub fn clear(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = GroupInstanceMonitorState::default();
        }
    }

    pub fn scan(
        &self,
        scope_key: &str,
        group_id: &str,
        fetched_at: &str,
        instances: &[RawJson],
    ) -> Option<ActivityEvent> {
        let mut current_locations = instances
            .iter()
            .map(|instance| instance_location(instance.as_value()))
            .filter(|location| !location.is_empty())
            .collect::<Vec<_>>();
        current_locations.sort();
        current_locations.dedup();
        let new_locations = {
            let mut state = self.state.lock().ok()?;
            if state.scope_key != scope_key {
                state.scope_key = scope_key.to_string();
                state.baseline.clear();
            }
            let previous_locations = state
                .baseline
                .insert(group_id.to_string(), current_locations.clone())?;
            current_locations
                .into_iter()
                .filter(|location| !previous_locations.contains(location))
                .collect::<Vec<_>>()
        };
        let first_location = new_locations.first()?;
        let first = instances
            .iter()
            .find(|instance| instance_location(instance.as_value()) == *first_location)
            .map(RawJson::as_value)
            .unwrap_or(&Value::Null);
        let mut event = ActivityEvent::new(
            ActivityKind::GroupInstanceOpened,
            format!("group-instance-opened:{scope_key}:{group_id}:{first_location}"),
            fetched_at,
        );
        event.subject = ActivitySubject::Group(group_id.to_string());
        event.facts = ActivityFacts {
            group_id: group_id.to_string(),
            group_name: first_non_empty([
                nested_str(first, &["group", "name"]),
                nested_str(first, &["instance", "group", "name"]),
                Some(group_id),
            ]),
            count: new_locations.len() as u64,
            location: first_location.clone(),
            world_name: first_non_empty([
                first.trimmed_field("worldName"),
                nested_str(first, &["world", "name"]),
                nested_str(first, &["instance", "world", "name"]),
            ]),
            image_url: first_non_empty([
                nested_str(first, &["group", "iconUrl"]),
                nested_str(first, &["group", "icon"]),
                nested_str(first, &["group", "thumbnailUrl"]),
            ]),
            ..ActivityFacts::default()
        };
        Some(event)
    }
}

fn instance_location(value: &Value) -> String {
    first_non_empty([
        value.trimmed_field("location"),
        nested_str(value, &["instance", "location"]),
    ])
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> String {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn nested_str<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{
        ActivityFavoriteGroupKeys, ActivityFavoriteGroups, ActivityFilters, ActivityRouter,
        ActivityRule, ActivityScope,
    };
    use chrono::Utc;
    use serde_json::json;

    fn scan(monitor: &GroupInstanceMonitor, group_id: &str, instances: &[RawJson]) -> Option<u64> {
        monitor
            .scan("scope", group_id, &Utc::now().to_rfc3339(), instances)
            .map(|event| event.facts.count)
    }

    #[test]
    fn first_scan_seeds_and_later_new_locations_are_aggregated_per_group() {
        let monitor = GroupInstanceMonitor::default();
        monitor.set_watched_groups(&["grp_test".into()]);
        assert_eq!(scan(&monitor, "grp_test", &[instance("one")]), None);
        assert_eq!(
            scan(
                &monitor,
                "grp_test",
                &[instance("one"), instance("two"), instance("three")]
            ),
            Some(2)
        );

        let mut filters = ActivityFilters::default();
        filters.wrist.types.insert(
            "group.instanceOpened".into(),
            ActivityRule {
                scope: ActivityScope::AllFavorites,
                favorite_group_keys: ActivityFavoriteGroupKeys::All,
            },
        );
        let runtime = ActivityRouter::with_filters(filters);
        runtime.set_group_favorite_groups(ActivityFavoriteGroups::from_map(
            [("group:collection".into(), vec!["grp_test".into()])].into(),
        ));
        let event = monitor
            .scan(
                "scope",
                "grp_test",
                &Utc::now().to_rfc3339(),
                &[
                    instance("one"),
                    instance("two"),
                    instance("three"),
                    instance("four"),
                ],
            )
            .expect("new instance event");
        let entry = runtime.ingest(event).expect("group instance entry");
        assert_eq!(
            entry
                .content
                .body
                .as_message()
                .expect("group instance message")
                .params()["location"],
            "Test World groupPlus(Test Group)"
        );
    }

    #[test]
    fn newly_watched_groups_seed_existing_instances_before_notifying() {
        let monitor = GroupInstanceMonitor::default();
        assert_eq!(scan(&monitor, "grp_test", &[instance("one")]), None);

        monitor.set_watched_groups(&["grp_test".into()]);
        assert_eq!(
            scan(&monitor, "grp_test", &[instance("one"), instance("two")]),
            None
        );
        assert_eq!(
            scan(
                &monitor,
                "grp_test",
                &[instance("one"), instance("two"), instance("three")]
            ),
            Some(1)
        );
    }

    #[test]
    fn adding_a_watched_group_keeps_existing_group_baselines() {
        let monitor = GroupInstanceMonitor::default();
        monitor.set_watched_groups(&["grp_test".into()]);
        assert_eq!(scan(&monitor, "grp_test", &[instance("one")]), None);

        monitor.set_watched_groups(&["grp_test".into(), "grp_new".into()]);

        assert_eq!(
            scan(&monitor, "grp_test", &[instance("one"), instance("two")]),
            Some(1)
        );
        assert_eq!(
            scan(
                &monitor,
                "grp_new",
                &[instance_for_group("grp_new", "existing")]
            ),
            None
        );
    }

    #[test]
    fn watch_changes_retain_only_groups_that_remain_watched() {
        let monitor = GroupInstanceMonitor::default();
        monitor.set_watched_groups(&["grp_one".into()]);
        for group_id in ["grp_one", "grp_two"] {
            assert_eq!(
                scan(
                    &monitor,
                    group_id,
                    &[instance_for_group(group_id, "existing")]
                ),
                None
            );
        }

        monitor.set_watched_groups(&["grp_one".into(), "grp_two".into()]);

        assert_eq!(
            scan(
                &monitor,
                "grp_one",
                &[
                    instance_for_group("grp_one", "existing"),
                    instance_for_group("grp_one", "new"),
                ]
            ),
            Some(1)
        );
        assert_eq!(
            scan(
                &monitor,
                "grp_two",
                &[
                    instance_for_group("grp_two", "existing"),
                    instance_for_group("grp_two", "new"),
                ]
            ),
            None
        );
    }

    #[test]
    fn player_count_changes_do_not_create_new_instance_events() {
        let monitor = GroupInstanceMonitor::default();
        monitor.set_watched_groups(&["grp_test".into()]);
        assert_eq!(
            scan(&monitor, "grp_test", &[instance_with_count("one", 2)]),
            None
        );
        assert_eq!(
            scan(&monitor, "grp_test", &[instance_with_count("one", 8)]),
            None
        );
    }

    #[test]
    fn each_group_compares_only_its_own_location_list() {
        let monitor = GroupInstanceMonitor::default();
        monitor.set_watched_groups(&["grp_one".into(), "grp_two".into()]);
        assert_eq!(
            scan(&monitor, "grp_one", &[instance_for_group("grp_one", "one")]),
            None
        );
        assert_eq!(
            scan(&monitor, "grp_two", &[instance_for_group("grp_two", "one")]),
            None
        );
        assert_eq!(
            scan(&monitor, "grp_two", &[instance_for_group("grp_two", "one")]),
            None
        );
        assert_eq!(
            scan(
                &monitor,
                "grp_one",
                &[
                    instance_for_group("grp_one", "one"),
                    instance_for_group("grp_one", "two"),
                ]
            ),
            Some(1)
        );
    }

    fn instance(id: &str) -> RawJson {
        instance_with_count(id, 1)
    }

    fn instance_with_count(id: &str, user_count: u64) -> RawJson {
        json!({
            "location": format!(
                "wrld_test:{id}~group(grp_test)~groupAccessType(plus)"
            ),
            "group": { "id": "grp_test", "name": "Test Group" },
            "world": { "name": "Test World" },
            "createdAt": Utc::now().to_rfc3339(),
            "userCount": user_count,
        })
        .into()
    }

    fn instance_for_group(group_id: &str, id: &str) -> RawJson {
        json!({
            "location": format!("wrld_test:{id}~group({group_id})"),
            "group": { "id": group_id, "name": group_id },
        })
        .into()
    }
}
