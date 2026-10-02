use serde_json::Value;
use vrcx_0_contracts::feed_live::FeedLiveEntry;
use vrcx_0_contracts::realtime::FriendLogUpsert;
use vrcx_0_core::derived_keys;
use vrcx_0_core::friends::FriendRecord;

use crate::realtime::RealtimeFriendOutput;

use vrcx_0_core::json::JsonExt;
use vrcx_0_core::text::first_owned;

use super::apply::record_string;

#[derive(Clone, Copy, Debug)]
pub(super) enum FriendRelationshipFeedKind {
    Friend,
    Unfriend,
}

impl FriendRelationshipFeedKind {
    fn feed_entry(
        self,
        created_at: String,
        user_id: String,
        display_name: String,
    ) -> FeedLiveEntry {
        match self {
            Self::Friend => FeedLiveEntry::Friend {
                created_at,
                user_id,
                display_name,
                owner_user_id: String::new(),
            },
            Self::Unfriend => FeedLiveEntry::Unfriend {
                created_at,
                user_id,
                display_name,
                owner_user_id: String::new(),
            },
        }
    }
}

fn patch_field_changed(patch: &Value, previous: &FriendRecord, key: &str) -> bool {
    let previous_value = record_string(previous, key);
    match patch.get(key) {
        None => false,
        Some(Value::Null) => !previous_value.is_empty(),
        Some(Value::String(next)) => *next != previous_value,
        Some(_) => true,
    }
}

pub(super) fn friend_log_upsert(
    user_id: &str,
    patch: &Value,
    previous: Option<&FriendRecord>,
    created_at: &str,
) -> FriendLogUpsert {
    FriendLogUpsert {
        target_user_id: user_id.to_string(),
        display_name: display_name(user_id, patch, previous),
        trust_level: first_owned([
            patch.text_field(derived_keys::TRUST_LEVEL),
            previous
                .map(|previous| record_string(previous, derived_keys::TRUST_LEVEL))
                .unwrap_or_default(),
        ]),
        friend_number: patch
            .i64_field(derived_keys::FRIEND_NUMBER)
            .or_else(|| {
                previous.and_then(|previous| previous.extra.i64_field(derived_keys::FRIEND_NUMBER))
            })
            .unwrap_or(0),
        created_at: created_at.to_string(),
        force_history: false,
    }
}

pub(crate) fn display_name_feed_entry(
    created_at: &str,
    user_id: &str,
    display_name: &str,
    previous_display_name: &str,
    friend_number: i64,
) -> FeedLiveEntry {
    FeedLiveEntry::DisplayName {
        created_at: created_at.to_string(),
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        previous_display_name: previous_display_name.to_string(),
        friend_number,
        owner_user_id: String::new(),
    }
}

pub(crate) fn trust_level_feed_entry(
    created_at: &str,
    user_id: &str,
    display_name: &str,
    trust_level: &str,
    previous_trust_level: &str,
    friend_number: i64,
) -> FeedLiveEntry {
    FeedLiveEntry::TrustLevel {
        created_at: created_at.to_string(),
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        trust_level: trust_level.to_string(),
        previous_trust_level: previous_trust_level.to_string(),
        friend_number,
        owner_user_id: String::new(),
    }
}

pub(super) fn add_profile_diff_feed_entries(
    output: &mut RealtimeFriendOutput,
    user_id: &str,
    patch: &Value,
    previous: &FriendRecord,
    previous_online: bool,
    created_at: &str,
) {
    if !previous_online {
        return;
    }
    let status_changed = patch_field_changed(patch, previous, "status");
    let status_description_changed = patch_field_changed(patch, previous, "statusDescription");
    let next_status = string_or_previous(patch, previous, "status");
    if (status_changed || status_description_changed)
        && next_status != "offline"
        && previous.status != "offline"
    {
        output.persistence.feed_entries.push(FeedLiveEntry::Status {
            created_at: created_at.to_string(),
            user_id: user_id.to_string(),
            display_name: display_name(user_id, patch, Some(previous)),
            status: next_status,
            status_description: string_or_previous(patch, previous, "statusDescription"),
            previous_status: previous.status.to_string(),
            previous_status_description: previous.status_description.to_string(),
            owner_user_id: String::new(),
        });
    }
}

pub(super) fn friend_relationship_feed_entry(
    relationship: FriendRelationshipFeedKind,
    user_id: &str,
    patch: &Value,
    previous: Option<&FriendRecord>,
    created_at: &str,
) -> FeedLiveEntry {
    relationship.feed_entry(
        created_at.to_string(),
        user_id.to_string(),
        display_name(user_id, patch, previous),
    )
}

pub(super) fn display_name(
    user_id: &str,
    patch: &Value,
    previous: Option<&FriendRecord>,
) -> String {
    first_owned([
        meaningful_name(patch, user_id),
        previous
            .map(|previous| meaningful_record_name(previous, user_id))
            .unwrap_or_default(),
        "Unknown".to_string(),
    ])
}

pub(super) fn meaningful_record_name(record: &FriendRecord, user_id: &str) -> String {
    vrcx_0_core::friends::meaningful_display_name(&record.display_name, &record.username, user_id)
        .unwrap_or_default()
}

pub(super) fn meaningful_name(value: &Value, user_id: &str) -> String {
    vrcx_0_core::friends::meaningful_display_name(
        &value.text_field("displayName"),
        &value.text_field("username"),
        user_id,
    )
    .unwrap_or_default()
}

fn string_or_previous(patch: &Value, previous: &FriendRecord, key: &str) -> String {
    let value = patch.text_field(key);
    if value.is_empty() {
        record_string(previous, key)
    } else {
        value
    }
}
