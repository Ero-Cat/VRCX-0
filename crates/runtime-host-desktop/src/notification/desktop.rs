use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use vrcx_0_application_activity::notification::RenderedNotification;
use vrcx_0_core::vrchat_ids::{is_group_id, is_user_id};

use super::NotificationDeliveryPreferences;
use vrcx_0_contracts::activity::ActivityKind;
use vrcx_0_core::OwnerId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopNotificationAction {
    pub owner_user_id: OwnerId,
    pub target: DesktopNotificationTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DesktopNotificationTarget {
    OpenUserProfile {
        #[serde(rename = "userId")]
        user_id: String,
    },
    OpenGroupProfile {
        #[serde(rename = "groupId")]
        group_id: String,
    },
}

impl DesktopNotificationAction {
    pub fn open_user_profile(owner_user_id: &OwnerId, user_id: &str) -> Option<Self> {
        if !is_user_id(owner_user_id.as_str()) || !is_user_id(user_id) {
            return None;
        }
        Some(Self {
            owner_user_id: owner_user_id.clone(),
            target: DesktopNotificationTarget::OpenUserProfile {
                user_id: user_id.to_string(),
            },
        })
    }

    pub fn open_group_profile(owner_user_id: &OwnerId, group_id: &str) -> Option<Self> {
        if !is_user_id(owner_user_id.as_str()) || !is_group_id(group_id) {
            return None;
        }
        Some(Self {
            owner_user_id: owner_user_id.clone(),
            target: DesktopNotificationTarget::OpenGroupProfile {
                group_id: group_id.to_string(),
            },
        })
    }
}

pub trait DesktopNotifier: Send + Sync {
    fn show(
        &self,
        title: &str,
        body: Option<&str>,
        image: Option<&str>,
        play_sound: bool,
        action: Option<&DesktopNotificationAction>,
    ) -> Result<(), String>;
}

#[derive(Clone, Default)]
pub struct DesktopNotifierSlot {
    inner: Arc<Mutex<Option<Arc<dyn DesktopNotifier>>>>,
}

impl DesktopNotifierSlot {
    pub fn set(&self, notifier: Arc<dyn DesktopNotifier>) {
        match self.inner.lock() {
            Ok(mut slot) => {
                *slot = Some(notifier);
            }
            Err(error) => {
                tracing::warn!("failed to set desktop notification bridge: {error}");
            }
        }
    }
}

impl DesktopNotifier for DesktopNotifierSlot {
    fn show(
        &self,
        title: &str,
        body: Option<&str>,
        image: Option<&str>,
        play_sound: bool,
        action: Option<&DesktopNotificationAction>,
    ) -> Result<(), String> {
        let notifier = self
            .inner
            .lock()
            .map_err(|error| format!("desktop notification bridge lock poisoned: {error}"))?
            .clone();
        let Some(notifier) = notifier else {
            return Ok(());
        };
        notifier.show(title, body, image, play_sound, action)
    }
}

pub(super) fn send_desktop_notification(
    notifier: &dyn DesktopNotifier,
    render: &RenderedNotification,
    kind: ActivityKind,
    group_name: &str,
    preferences: &NotificationDeliveryPreferences,
    local_image: Option<&str>,
    action: Option<&DesktopNotificationAction>,
) {
    let title = desktop_notification_title(kind, group_name, &render.title);
    if let Err(error) = notifier.show(
        &title,
        non_empty(&render.body),
        local_image,
        preferences.desktop_notification_sound,
        action,
    ) {
        tracing::warn!("[Desktop] notification send failed: {error}");
    }
}

fn desktop_notification_title<'a>(
    kind: ActivityKind,
    group_name: &str,
    title: &'a str,
) -> Cow<'a, str> {
    let group_name = group_name.trim();
    if kind == ActivityKind::GroupAnnouncement && !group_name.is_empty() {
        return Cow::Owned(format!("{group_name} · {title}"));
    }
    Cow::Borrowed(title)
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use vrcx_0_core::OwnerId;

    use super::{desktop_notification_title, ActivityKind, DesktopNotificationAction};

    #[test]
    fn group_announcement_title_includes_source_group() {
        assert_eq!(
            desktop_notification_title(
                ActivityKind::GroupAnnouncement,
                "Maple Club",
                "Group Announcement"
            ),
            "Maple Club · Group Announcement"
        );
        assert_eq!(
            desktop_notification_title(ActivityKind::GroupAnnouncement, "", "Group Announcement"),
            "Group Announcement"
        );
        assert_eq!(
            desktop_notification_title(
                ActivityKind::GroupInformative,
                "Maple Club",
                "Group Information"
            ),
            "Group Information"
        );
    }

    #[test]
    fn open_user_profile_action_requires_canonical_owner_and_actor_ids() {
        let action = DesktopNotificationAction::open_user_profile(
            &OwnerId::new("usr_12345678-1234-1234-1234-1234567890ab"),
            "usr_abcdefab-cdef-abcd-efab-cdefabcdefab",
        );

        assert_eq!(
            action,
            Some(DesktopNotificationAction {
                owner_user_id: OwnerId::new("usr_12345678-1234-1234-1234-1234567890ab"),
                target: super::DesktopNotificationTarget::OpenUserProfile {
                    user_id: "usr_abcdefab-cdef-abcd-efab-cdefabcdefab".into(),
                },
            })
        );
        assert!(DesktopNotificationAction::open_user_profile(
            &OwnerId::new("usr_invalid"),
            "usr_abcdefab-cdef-abcd-efab-cdefabcdefab"
        )
        .is_none());
    }
}
