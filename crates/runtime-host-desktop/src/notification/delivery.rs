use vrcx_0_application_activity::ActivityDelivery;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotificationTtsNameMode {
    #[default]
    Username,
    Note,
    UsernameAndNote,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotificationDeliveryCondition {
    #[default]
    Never,
    Always,
    InsideVr,
    OutsideVr,
    GameClosed,
    GameRunning,
    DesktopMode,
}

impl NotificationDeliveryCondition {
    pub(super) fn from_config(value: &str) -> Self {
        match value {
            "Always" => Self::Always,
            "Inside VR" => Self::InsideVr,
            "Outside VR" => Self::OutsideVr,
            "Game Closed" => Self::GameClosed,
            "Game Running" => Self::GameRunning,
            "Desktop Mode" => Self::DesktopMode,
            _ => Self::Never,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationDeliveryPreferences {
    pub desktop_toast: NotificationDeliveryCondition,
    pub afk_desktop_toast: bool,
    pub overlay_toast: NotificationDeliveryCondition,
    pub desktop_notification_sound: bool,
    pub notification_tts: NotificationDeliveryCondition,
    pub notification_tts_name_mode: NotificationTtsNameMode,
    pub notification_tts_voice_native: String,
    pub notification_tts_volume: u8,
    pub xs_notifications: bool,
    pub ovrt_hud_notifications: bool,
    pub ovrt_wrist_notifications: bool,
    pub image_notifications: bool,
    pub notification_timeout_ms: i32,
    pub notification_opacity_percent: i32,
    pub show_instance_id_in_location: bool,
}

impl Default for NotificationDeliveryPreferences {
    fn default() -> Self {
        Self {
            desktop_toast: NotificationDeliveryCondition::Never,
            afk_desktop_toast: false,
            overlay_toast: NotificationDeliveryCondition::GameRunning,
            desktop_notification_sound: false,
            notification_tts: NotificationDeliveryCondition::Never,
            notification_tts_name_mode: NotificationTtsNameMode::Username,
            notification_tts_voice_native: String::new(),
            notification_tts_volume: vrcx_0_host_desktop::tts::DEFAULT_TTS_VOLUME,
            xs_notifications: false,
            ovrt_hud_notifications: false,
            ovrt_wrist_notifications: false,
            image_notifications: true,
            notification_timeout_ms: 3000,
            notification_opacity_percent: 100,
            show_instance_id_in_location: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotificationDeliveryGameState {
    pub is_game_running: bool,
    pub is_steamvr_running: bool,
    pub is_game_no_vr: bool,
    pub is_hmd_afk: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotificationDeliveryPlan {
    pub desktop: bool,
    pub xs: bool,
    pub ovrt: bool,
    pub ovrt_hud: bool,
    pub ovrt_wrist: bool,
    pub tts: bool,
}

impl NotificationDeliveryPlan {
    pub fn has_local_transport(self) -> bool {
        self.desktop || self.xs || self.ovrt || self.tts
    }

    pub fn needs_local_image(self) -> bool {
        self.desktop || self.xs || self.ovrt
    }
}

pub fn decide_notification_plan(
    delivery: &ActivityDelivery,
    preferences: &NotificationDeliveryPreferences,
    game: &NotificationDeliveryGameState,
) -> NotificationDeliveryPlan {
    let afk = preferences.afk_desktop_toast
        && game.is_hmd_afk
        && game.is_game_running
        && !game.is_game_no_vr;
    let desktop =
        delivery.desktop && (should_play_for_condition(preferences.desktop_toast, game) || afk);
    let vr = delivery.vr
        && game.is_steamvr_running
        && should_play_for_condition(preferences.overlay_toast, game);
    let xs = vr && preferences.xs_notifications;
    let ovrt_hud = cfg!(windows) && vr && preferences.ovrt_hud_notifications;
    let ovrt_wrist = cfg!(windows) && vr && preferences.ovrt_wrist_notifications;
    let ovrt = ovrt_hud || ovrt_wrist;
    let tts = delivery.tts && should_play_for_condition(preferences.notification_tts, game);

    NotificationDeliveryPlan {
        desktop,
        xs,
        ovrt,
        ovrt_hud,
        ovrt_wrist,
        tts,
    }
}

pub(crate) fn should_play_for_condition(
    condition: NotificationDeliveryCondition,
    game: &NotificationDeliveryGameState,
) -> bool {
    match condition {
        NotificationDeliveryCondition::Never => false,
        NotificationDeliveryCondition::Always => true,
        NotificationDeliveryCondition::InsideVr => game.is_steamvr_running,
        NotificationDeliveryCondition::OutsideVr => !game.is_steamvr_running,
        NotificationDeliveryCondition::GameClosed => !game.is_game_running,
        NotificationDeliveryCondition::GameRunning => game.is_game_running,
        NotificationDeliveryCondition::DesktopMode => game.is_game_no_vr && game.is_game_running,
    }
}
