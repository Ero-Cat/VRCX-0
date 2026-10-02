use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::feed::{FeedFilter, FeedRowOutput};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type")]
pub enum FeedLiveEntry {
    #[serde(rename_all = "camelCase")]
    Online {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        location: String,
        world_name: String,
        group_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_location: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Offline {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        location: String,
        world_name: String,
        group_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_location: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename = "GPS", rename_all = "camelCase")]
    Gps {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        location: String,
        world_name: String,
        previous_location: String,
        time: i64,
        group_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_location: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Status {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        status: String,
        status_description: String,
        previous_status: String,
        previous_status_description: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Bio {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        bio: String,
        previous_bio: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Avatar {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        owner_id: String,
        previous_owner_id: String,
        avatar_name: String,
        previous_avatar_name: String,
        current_avatar_image_url: String,
        previous_current_avatar_image_url: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    TrustLevel {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        trust_level: String,
        previous_trust_level: String,
        friend_number: i64,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    DisplayName {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        previous_display_name: String,
        friend_number: i64,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Friend {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Unfriend {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename_all = "camelCase")]
    OnPlayerJoining {
        #[serde(rename = "created_at")]
        created_at: String,
        user_id: String,
        display_name: String,
        location: String,
        traveling_to_location: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_location: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
    #[serde(rename = "instance.closed", rename_all = "camelCase")]
    InstanceClosed {
        #[serde(rename = "created_at")]
        created_at: String,
        id: String,
        location: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_location: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        owner_user_id: String,
    },
}

macro_rules! common_field {
    ($self:ident, $field:ident) => {
        match $self {
            FeedLiveEntry::Online { $field, .. }
            | FeedLiveEntry::Offline { $field, .. }
            | FeedLiveEntry::Gps { $field, .. }
            | FeedLiveEntry::Status { $field, .. }
            | FeedLiveEntry::Bio { $field, .. }
            | FeedLiveEntry::Avatar { $field, .. }
            | FeedLiveEntry::DisplayName { $field, .. }
            | FeedLiveEntry::TrustLevel { $field, .. }
            | FeedLiveEntry::Friend { $field, .. }
            | FeedLiveEntry::Unfriend { $field, .. }
            | FeedLiveEntry::OnPlayerJoining { $field, .. }
            | FeedLiveEntry::InstanceClosed { $field, .. } => $field,
        }
    };
}

impl FeedLiveEntry {
    pub fn entry_type(&self) -> &'static str {
        match self {
            Self::Online { .. } => "Online",
            Self::Offline { .. } => "Offline",
            Self::Gps { .. } => "GPS",
            Self::Status { .. } => "Status",
            Self::Bio { .. } => "Bio",
            Self::Avatar { .. } => "Avatar",
            Self::TrustLevel { .. } => "TrustLevel",
            Self::DisplayName { .. } => "DisplayName",
            Self::Friend { .. } => "Friend",
            Self::Unfriend { .. } => "Unfriend",
            Self::OnPlayerJoining { .. } => "OnPlayerJoining",
            Self::InstanceClosed { .. } => "instance.closed",
        }
    }

    pub fn filter(&self) -> Option<FeedFilter> {
        match self {
            Self::Online { .. } => Some(FeedFilter::Online),
            Self::Offline { .. } => Some(FeedFilter::Offline),
            Self::Gps { .. } => Some(FeedFilter::Gps),
            Self::Status { .. } => Some(FeedFilter::Status),
            Self::Bio { .. } => Some(FeedFilter::Bio),
            Self::Avatar { .. } => Some(FeedFilter::Avatar),
            Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. }
            | Self::OnPlayerJoining { .. }
            | Self::InstanceClosed { .. } => None,
        }
    }

    pub fn created_at(&self) -> &str {
        common_field!(self, created_at)
    }

    pub fn owner_user_id(&self) -> &str {
        common_field!(self, owner_user_id)
    }

    pub fn set_owner_user_id(&mut self, value: String) {
        *common_field!(self, owner_user_id) = value;
    }

    pub fn user_id(&self) -> &str {
        match self {
            Self::Online { user_id, .. }
            | Self::Offline { user_id, .. }
            | Self::Gps { user_id, .. }
            | Self::Status { user_id, .. }
            | Self::Bio { user_id, .. }
            | Self::Avatar { user_id, .. }
            | Self::DisplayName { user_id, .. }
            | Self::TrustLevel { user_id, .. }
            | Self::Friend { user_id, .. }
            | Self::Unfriend { user_id, .. }
            | Self::OnPlayerJoining { user_id, .. } => user_id,
            Self::InstanceClosed { .. } => "",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Online { display_name, .. }
            | Self::Offline { display_name, .. }
            | Self::Gps { display_name, .. }
            | Self::Status { display_name, .. }
            | Self::Bio { display_name, .. }
            | Self::Avatar { display_name, .. }
            | Self::DisplayName { display_name, .. }
            | Self::TrustLevel { display_name, .. }
            | Self::Friend { display_name, .. }
            | Self::Unfriend { display_name, .. }
            | Self::OnPlayerJoining { display_name, .. } => display_name,
            Self::InstanceClosed { .. } => "",
        }
    }

    pub fn set_display_name(&mut self, value: String) {
        match self {
            Self::Online { display_name, .. }
            | Self::Offline { display_name, .. }
            | Self::Gps { display_name, .. }
            | Self::Status { display_name, .. }
            | Self::Bio { display_name, .. }
            | Self::Avatar { display_name, .. }
            | Self::DisplayName { display_name, .. }
            | Self::TrustLevel { display_name, .. }
            | Self::Friend { display_name, .. }
            | Self::Unfriend { display_name, .. }
            | Self::OnPlayerJoining { display_name, .. } => *display_name = value,
            Self::InstanceClosed { .. } => {}
        }
    }

    pub fn location(&self) -> &str {
        match self {
            Self::Online { location, .. }
            | Self::Offline { location, .. }
            | Self::Gps { location, .. }
            | Self::OnPlayerJoining { location, .. }
            | Self::InstanceClosed { location, .. } => location,
            Self::Status { .. }
            | Self::Bio { .. }
            | Self::Avatar { .. }
            | Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. } => "",
        }
    }

    pub fn group_name(&self) -> &str {
        match self {
            Self::Online { group_name, .. }
            | Self::Offline { group_name, .. }
            | Self::Gps { group_name, .. } => group_name,
            Self::Status { .. }
            | Self::Bio { .. }
            | Self::Avatar { .. }
            | Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. }
            | Self::OnPlayerJoining { .. }
            | Self::InstanceClosed { .. } => "",
        }
    }

    pub fn world_name(&self) -> &str {
        match self {
            Self::Online { world_name, .. }
            | Self::Offline { world_name, .. }
            | Self::Gps { world_name, .. } => world_name,
            Self::OnPlayerJoining { world_name, .. } | Self::InstanceClosed { world_name, .. } => {
                world_name.as_deref().unwrap_or_default()
            }
            Self::Status { .. }
            | Self::Bio { .. }
            | Self::Avatar { .. }
            | Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. } => "",
        }
    }

    pub fn set_world_name(&mut self, value: String) {
        match self {
            Self::Online { world_name, .. }
            | Self::Offline { world_name, .. }
            | Self::Gps { world_name, .. } => *world_name = value,
            Self::OnPlayerJoining { world_name, .. } | Self::InstanceClosed { world_name, .. } => {
                *world_name = Some(value)
            }
            Self::Status { .. }
            | Self::Bio { .. }
            | Self::Avatar { .. }
            | Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. } => {}
        }
    }

    pub fn world_id(&self) -> &str {
        match self {
            Self::Online { world_id, .. }
            | Self::Offline { world_id, .. }
            | Self::Gps { world_id, .. }
            | Self::OnPlayerJoining { world_id, .. }
            | Self::InstanceClosed { world_id, .. } => world_id.as_deref().unwrap_or_default(),
            Self::Status { .. }
            | Self::Bio { .. }
            | Self::Avatar { .. }
            | Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. } => "",
        }
    }

    pub fn set_world_id(&mut self, value: String) {
        match self {
            Self::Online { world_id, .. }
            | Self::Offline { world_id, .. }
            | Self::Gps { world_id, .. }
            | Self::OnPlayerJoining { world_id, .. }
            | Self::InstanceClosed { world_id, .. } => *world_id = Some(value),
            Self::Status { .. }
            | Self::Bio { .. }
            | Self::Avatar { .. }
            | Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. } => {}
        }
    }

    pub fn display_location(&self) -> Option<&str> {
        match self {
            Self::Online {
                display_location, ..
            }
            | Self::Offline {
                display_location, ..
            }
            | Self::Gps {
                display_location, ..
            }
            | Self::OnPlayerJoining {
                display_location, ..
            }
            | Self::InstanceClosed {
                display_location, ..
            } => display_location.as_deref(),
            Self::Status { .. }
            | Self::Bio { .. }
            | Self::Avatar { .. }
            | Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. } => None,
        }
    }

    pub fn set_display_location(&mut self, value: String) {
        match self {
            Self::Online {
                display_location, ..
            }
            | Self::Offline {
                display_location, ..
            }
            | Self::Gps {
                display_location, ..
            }
            | Self::OnPlayerJoining {
                display_location, ..
            }
            | Self::InstanceClosed {
                display_location, ..
            } => *display_location = Some(value),
            Self::Status { .. }
            | Self::Bio { .. }
            | Self::Avatar { .. }
            | Self::DisplayName { .. }
            | Self::TrustLevel { .. }
            | Self::Friend { .. }
            | Self::Unfriend { .. } => {}
        }
    }

    pub fn correction_id(&self) -> String {
        match self {
            Self::InstanceClosed { id, .. } => format!("id:{id}"),
            _ => format!(
                "{}:{}:{}:{}:",
                self.entry_type(),
                self.created_at(),
                self.user_id(),
                self.location()
            ),
        }
    }

    pub fn search_fields(&self) -> Vec<&str> {
        match self {
            Self::Online {
                display_name,
                world_name,
                group_name,
                ..
            }
            | Self::Offline {
                display_name,
                world_name,
                group_name,
                ..
            }
            | Self::Gps {
                display_name,
                world_name,
                group_name,
                ..
            } => vec![display_name, world_name, group_name],
            Self::Status {
                display_name,
                status,
                status_description,
                previous_status,
                previous_status_description,
                ..
            } => vec![
                display_name,
                status,
                status_description,
                previous_status,
                previous_status_description,
            ],
            Self::Bio {
                display_name,
                bio,
                previous_bio,
                ..
            } => vec![display_name, bio, previous_bio],
            Self::Avatar {
                display_name,
                avatar_name,
                ..
            } => vec![display_name, avatar_name],
            Self::DisplayName {
                display_name,
                previous_display_name,
                ..
            } => vec![display_name, previous_display_name],
            Self::TrustLevel { display_name, .. }
            | Self::Friend { display_name, .. }
            | Self::Unfriend { display_name, .. } => vec![display_name],
            Self::OnPlayerJoining {
                display_name,
                world_name,
                ..
            } => vec![display_name, world_name.as_deref().unwrap_or_default()],
            Self::InstanceClosed {
                world_name,
                message,
                ..
            } => vec![world_name.as_deref().unwrap_or_default(), message],
        }
    }

    pub fn avatar_owner_id(&self) -> Option<&str> {
        match self {
            Self::Avatar { owner_id, .. } => Some(owner_id),
            _ => None,
        }
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("feed live entry is always serializable")
    }
}

impl From<&FeedLiveEntry> for FeedRowOutput {
    fn from(entry: &FeedLiveEntry) -> Self {
        let row = FeedRowOutput {
            created_at: optional_text(entry.created_at()),
            user_id: optional_text(entry.user_id()),
            display_name: optional_text(entry.display_name()),
            r#type: optional_text(entry.entry_type()),
            location: optional_text(entry.location()),
            world_name: optional_text(entry.world_name()),
            group_name: optional_text(entry.group_name()),
            owner_user_id: optional_text(entry.owner_user_id()),
            ..FeedRowOutput::default()
        };
        match entry {
            FeedLiveEntry::Online { time, .. } | FeedLiveEntry::Offline { time, .. } => {
                FeedRowOutput { time: *time, ..row }
            }
            FeedLiveEntry::Gps {
                previous_location,
                time,
                ..
            } => FeedRowOutput {
                previous_location: optional_text(previous_location),
                time: Some(*time),
                ..row
            },
            FeedLiveEntry::Status {
                status,
                status_description,
                previous_status,
                previous_status_description,
                ..
            } => FeedRowOutput {
                status: optional_text(status),
                status_description: optional_text(status_description),
                previous_status: optional_text(previous_status),
                previous_status_description: optional_text(previous_status_description),
                ..row
            },
            FeedLiveEntry::Bio {
                bio, previous_bio, ..
            } => FeedRowOutput {
                bio: optional_text(bio),
                previous_bio: optional_text(previous_bio),
                ..row
            },
            FeedLiveEntry::Avatar {
                owner_id,
                previous_owner_id,
                avatar_name,
                previous_avatar_name,
                current_avatar_image_url,
                previous_current_avatar_image_url,
                ..
            } => FeedRowOutput {
                owner_id: optional_text(owner_id),
                previous_owner_id: optional_text(previous_owner_id),
                avatar_name: optional_text(avatar_name),
                previous_avatar_name: optional_text(previous_avatar_name),
                current_avatar_image_url: optional_text(current_avatar_image_url),
                previous_current_avatar_image_url: optional_text(previous_current_avatar_image_url),
                ..row
            },
            FeedLiveEntry::DisplayName { .. }
            | FeedLiveEntry::TrustLevel { .. }
            | FeedLiveEntry::Friend { .. }
            | FeedLiveEntry::Unfriend { .. }
            | FeedLiveEntry::OnPlayerJoining { .. }
            | FeedLiveEntry::InstanceClosed { .. } => row,
        }
    }
}

fn optional_text(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}
