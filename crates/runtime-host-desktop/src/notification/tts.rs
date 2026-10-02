use std::borrow::Cow;

use vrcx_0_application_activity::notification::{
    render_delivery, OverlayLocale, RenderedNotification,
};
use vrcx_0_application_activity::ActivityDelivery;
use vrcx_0_host_desktop::tts::TtsEngine;

use super::{NotificationDeliveryPreferences, NotificationTtsNameMode};

pub(super) fn send_tts_notification(
    tts: &dyn TtsEngine,
    delivery: &ActivityDelivery,
    render: &RenderedNotification,
    preferences: &NotificationDeliveryPreferences,
    locale: OverlayLocale,
    user_memo: Option<&str>,
) {
    let text = notification_tts_text(delivery, render, preferences, locale, user_memo);
    if let Err(error) = tts.speak(
        &text,
        non_empty(&preferences.notification_tts_voice_native),
        preferences.notification_tts_volume,
    ) {
        tracing::warn!("[TTS] notification speak failed: {error}");
    }
}

pub(super) fn notification_tts_text(
    delivery: &ActivityDelivery,
    render: &RenderedNotification,
    preferences: &NotificationDeliveryPreferences,
    locale: OverlayLocale,
    user_memo: Option<&str>,
) -> String {
    let render = notification_tts_render(delivery, render, preferences, locale);
    if memo_actor_user_id(delivery, &render, preferences).is_none() {
        return render.text.clone();
    }
    let Some(memo_first_line) = memo_first_line(user_memo) else {
        return render.text.clone();
    };
    let name_mode = preferences.notification_tts_name_mode;
    let title = render.title.trim();
    let replacement = match name_mode {
        NotificationTtsNameMode::Note => memo_first_line.to_string(),
        NotificationTtsNameMode::UsernameAndNote => format!("{title}, {memo_first_line}"),
        NotificationTtsNameMode::Username => return render.text.clone(),
    };
    render.text.replacen(title, &replacement, 1)
}

pub(super) fn notification_tts_memo_actor_user_id<'a>(
    delivery: &'a ActivityDelivery,
    render: &RenderedNotification,
    preferences: &NotificationDeliveryPreferences,
    locale: OverlayLocale,
) -> Option<&'a str> {
    let render = notification_tts_render(delivery, render, preferences, locale);
    memo_actor_user_id(delivery, &render, preferences)
}

fn notification_tts_render<'a>(
    delivery: &ActivityDelivery,
    render: &'a RenderedNotification,
    preferences: &NotificationDeliveryPreferences,
    locale: OverlayLocale,
) -> Cow<'a, RenderedNotification> {
    if preferences.show_instance_id_in_location {
        Cow::Owned(render_delivery(delivery, locale, false))
    } else {
        Cow::Borrowed(render)
    }
}

fn memo_actor_user_id<'a>(
    delivery: &'a ActivityDelivery,
    render: &RenderedNotification,
    preferences: &NotificationDeliveryPreferences,
) -> Option<&'a str> {
    if preferences.notification_tts_name_mode == NotificationTtsNameMode::Username
        || render.title.trim().is_empty()
    {
        return None;
    }
    non_empty(&delivery.entry.actor_user_id)
}

fn memo_first_line(user_memo: Option<&str>) -> Option<&str> {
    user_memo?
        .lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}
