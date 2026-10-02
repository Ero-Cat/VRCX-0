use vrcx_0_contracts::activity::{
    stable_source_hash, ActivityActor, ActivityEvent, ActivityFacts, ActivityKind, ActivitySubject,
};
use vrcx_0_contracts::game_log::GameLogJoinLeaveEntry;
use vrcx_0_core::location::world_id_from_location;

use crate::game_log::video::VideoInput;
use crate::game_log::{GameLogAvatarChange, GameLogIngestOutput};
use crate::PlayerModeration;

pub(crate) fn game_log_activity_events<F, M>(
    output: &GameLogIngestOutput,
    mut include_join_leave: F,
    mut moderation_of: M,
) -> Vec<ActivityEvent>
where
    F: FnMut(&GameLogJoinLeaveEntry) -> bool,
    M: FnMut(&GameLogJoinLeaveEntry) -> PlayerModeration,
{
    let mut events = Vec::new();
    for entry in output
        .batch
        .join_leave
        .iter()
        .filter(|entry| include_join_leave(entry))
    {
        let Some(kind) = ActivityKind::from_key(&entry.event_type) else {
            continue;
        };
        events.push(join_leave_event(entry, kind));
        if entry.user_id.trim().is_empty() {
            continue;
        }
        let moderated_kinds = match kind {
            ActivityKind::OnPlayerJoined => [
                ActivityKind::BlockedOnPlayerJoined,
                ActivityKind::MutedOnPlayerJoined,
            ],
            ActivityKind::OnPlayerLeft => [
                ActivityKind::BlockedOnPlayerLeft,
                ActivityKind::MutedOnPlayerLeft,
            ],
            _ => continue,
        };
        let moderation = moderation_of(entry);
        for (moderated, kind) in [moderation.blocked, moderation.muted]
            .into_iter()
            .zip(moderated_kinds)
        {
            if moderated {
                events.push(join_leave_event(entry, kind));
            }
        }
    }
    events.extend(
        output
            .batch
            .events
            .iter()
            .map(|entry| game_log_event(&entry.created_at, &entry.data)),
    );
    for entry in &output.batch.externals {
        let mut event = ActivityEvent::new(
            ActivityKind::External,
            format!(
                "game-log-external:{}:{}:{}:{}",
                entry.user_id,
                entry.location,
                entry.created_at,
                stable_source_hash(&entry.message)
            ),
            entry.created_at.clone(),
        );
        event.actor = ActivityActor::new(entry.user_id.clone(), entry.display_name.clone());
        event.subject = ActivitySubject::User(entry.user_id.clone());
        event.facts = ActivityFacts {
            message: entry.message.clone(),
            location: entry.location.clone(),
            ..ActivityFacts::default()
        };
        events.push(event);
    }
    events
}

fn join_leave_event(entry: &GameLogJoinLeaveEntry, kind: ActivityKind) -> ActivityEvent {
    let mut event = ActivityEvent::new(
        kind,
        format!(
            "game-log:{}:{}:{}:{}",
            kind.key(),
            entry.user_id,
            entry.location,
            entry.created_at
        ),
        entry.created_at.clone(),
    );
    event.actor = ActivityActor::new(entry.user_id.clone(), entry.display_name.clone());
    event.subject = ActivitySubject::User(entry.user_id.clone());
    event.in_current_instance = true;
    event.facts = ActivityFacts {
        location: entry.location.clone(),
        world_id: world_id_from_location(&entry.location),
        world_name: entry.world_name.clone(),
        ..ActivityFacts::default()
    };
    event
}

pub(crate) fn lobby_avatar_change_event(change: &GameLogAvatarChange) -> ActivityEvent {
    let mut event = ActivityEvent::new(
        ActivityKind::LobbyAvatarChange,
        format!(
            "game-log-avatar:{}:{}:{}:{}",
            change.user_id, change.display_name, change.created_at, change.avatar_name
        ),
        change.created_at.clone(),
    );
    event.actor = ActivityActor::new(change.user_id.clone(), change.display_name.clone());
    event.subject = ActivitySubject::User(change.user_id.clone());
    event.in_current_instance = true;
    event.facts = ActivityFacts {
        avatar_name: change.avatar_name.clone(),
        location: change.location.clone(),
        world_id: world_id_from_location(&change.location),
        world_name: change.world_name.clone(),
        ..ActivityFacts::default()
    };
    event
}

pub(crate) fn game_log_event(created_at: &str, data: &str) -> ActivityEvent {
    let mut event = ActivityEvent::new(
        ActivityKind::Event,
        format!("game-log-event:{created_at}:{}", stable_source_hash(data)),
        created_at,
    );
    event.facts.message = data.to_string();
    event
}

pub(crate) fn video_activity_event(input: &VideoInput) -> ActivityEvent {
    let mut event = ActivityEvent::new(
        ActivityKind::VideoPlay,
        format!(
            "video-play:{}:{}:{}:{}",
            input.location,
            input.display_name,
            input.created_at,
            stable_source_hash(&input.video_url)
        ),
        input.created_at.clone(),
    );
    event.actor = ActivityActor::new(input.user_id.clone(), input.display_name.clone());
    event.subject = ActivitySubject::User(input.user_id.clone());
    event.in_current_instance = true;
    event.facts = ActivityFacts {
        location: input.location.clone(),
        world_id: world_id_from_location(&input.location),
        world_name: input.world_name.clone(),
        video: first_non_empty(&input.video_name, &input.video_url),
        image_url: input.thumbnail_url.clone(),
        ..ActivityFacts::default()
    };
    event
}

fn first_non_empty(preferred: &str, fallback: &str) -> String {
    match preferred.trim() {
        "" => fallback.trim().to_string(),
        value => value.to_string(),
    }
}

#[cfg(test)]
mod tests;
