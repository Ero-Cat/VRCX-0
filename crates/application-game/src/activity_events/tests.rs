use super::*;
use crate::GameLogSideEffect;
use vrcx_0_contracts::game_log::{
    GameLogEventEntry, GameLogExternalEntry, GameLogJoinLeaveEntry, GameLogWriteBatch,
};

fn all_events(output: &GameLogIngestOutput) -> Vec<ActivityEvent> {
    game_log_activity_events(output, |_| true, |_| PlayerModeration::default())
}

#[test]
fn game_log_join_leave_batch_becomes_current_instance_activity() {
    let output = GameLogIngestOutput {
        batch: GameLogWriteBatch {
            join_leave: vec![GameLogJoinLeaveEntry {
                created_at: "2026-05-31T00:04:00.000Z".to_string(),
                event_type: "OnPlayerJoined".to_string(),
                display_name: "Joining User".to_string(),
                location: "wrld_1:123".to_string(),
                user_id: "usr_joining".to_string(),
                world_name: "Test World".to_string(),
                time: 0,
            }],
            ..GameLogWriteBatch::default()
        },
        ..GameLogIngestOutput::default()
    };

    let events = all_events(&output);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, ActivityKind::OnPlayerJoined);
    assert_eq!(events[0].actor.display_name, "Joining User");
    assert!(events[0].in_current_instance);
    assert_eq!(events[0].facts.world_id, "wrld_1");
}

#[test]
fn game_log_event_and_external_batches_become_system_activity() {
    let output = GameLogIngestOutput {
        batch: GameLogWriteBatch {
            events: vec![GameLogEventEntry {
                created_at: "2026-05-31T00:05:00.000Z".to_string(),
                data: "Something happened".to_string(),
            }],
            externals: vec![GameLogExternalEntry {
                created_at: "2026-05-31T00:05:01.000Z".to_string(),
                message: "External message".to_string(),
                display_name: "External User".to_string(),
                user_id: "usr_external".to_string(),
                location: "wrld_1:123".to_string(),
            }],
            ..GameLogWriteBatch::default()
        },
        ..GameLogIngestOutput::default()
    };

    let events = all_events(&output);

    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![ActivityKind::Event, ActivityKind::External]
    );
    assert_eq!(events[0].facts.message, "Something happened");
    assert_eq!(events[1].actor.display_name, "External User");
}

#[test]
fn game_log_system_and_video_events_with_same_timestamp_do_not_collide() {
    let output = GameLogIngestOutput {
        batch: GameLogWriteBatch {
            events: vec![
                GameLogEventEntry {
                    created_at: "2026-05-31T00:05:00.000Z".to_string(),
                    data: "First event".to_string(),
                },
                GameLogEventEntry {
                    created_at: "2026-05-31T00:05:00.000Z".to_string(),
                    data: "Second event".to_string(),
                },
            ],
            externals: vec![
                GameLogExternalEntry {
                    created_at: "2026-05-31T00:05:01.000Z".to_string(),
                    message: "First external".to_string(),
                    display_name: "External User".to_string(),
                    user_id: "usr_external".to_string(),
                    location: "wrld_1:123".to_string(),
                },
                GameLogExternalEntry {
                    created_at: "2026-05-31T00:05:01.000Z".to_string(),
                    message: "Second external".to_string(),
                    display_name: "External User".to_string(),
                    user_id: "usr_external".to_string(),
                    location: "wrld_1:123".to_string(),
                },
            ],
            ..GameLogWriteBatch::default()
        },
        side_effects: vec![
            GameLogSideEffect::Video(VideoInput {
                created_at: "2026-05-31T00:05:02.000Z".to_string(),
                location: "wrld_1:123".to_string(),
                video_url: "https://example.test/first".to_string(),
                video_id: "first".to_string(),
                display_name: "Video User".to_string(),
                user_id: "usr_video".to_string(),
                ..VideoInput::default()
            }),
            GameLogSideEffect::Video(VideoInput {
                created_at: "2026-05-31T00:05:02.000Z".to_string(),
                location: "wrld_1:123".to_string(),
                video_url: "https://example.test/second".to_string(),
                video_id: "second".to_string(),
                display_name: "Video User".to_string(),
                user_id: "usr_video".to_string(),
                ..VideoInput::default()
            }),
        ],
        ..GameLogIngestOutput::default()
    };

    let mut events = output
        .side_effects
        .iter()
        .filter_map(|side_effect| match side_effect {
            GameLogSideEffect::Video(input) => Some(video_activity_event(input)),
            _ => None,
        })
        .collect::<Vec<_>>();
    events.extend(all_events(&output));

    assert_eq!(events.len(), 6);
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![
            ActivityKind::VideoPlay,
            ActivityKind::VideoPlay,
            ActivityKind::Event,
            ActivityKind::Event,
            ActivityKind::External,
            ActivityKind::External
        ]
    );
    let source_ids = events
        .iter()
        .map(|event| event.source_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(source_ids.len(), events.len());
}
