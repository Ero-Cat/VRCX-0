mod activity_filters;
mod auth_webhook;
mod discord;
mod enrichment;
mod generic_webhook;
mod image_file;
mod localization;
mod ports;
mod preferences;
mod rendered;
mod rendering;
#[cfg(test)]
mod tests;
mod user_image;
mod webhook;
mod webhook_delivery;
mod webhook_sink;

pub use activity_filters::{
    load_location_hidden_user_ids, load_overlay_activity_filters,
    NotificationActivityFilterSurface, NotificationActivityFiltersSetInput,
    OverlayActivityFilterProfile, OverlayActivityPreferenceFilters,
    OverlayActivityPreferenceSurface,
};
pub use activity_filters::{
    save_notification_activity_filters, save_overlay_activity_preference_filters,
};
pub use auth_webhook::{
    auth_webhook_generic_payload, auth_webhook_is_enabled, auth_webhook_should_recover,
    AuthWebhookEvent, AuthWebhookEventKind,
};
pub use auth_webhook::{AuthWebhookQueue, AuthWebhookQueueDeps};
pub use enrichment::{resolve_delivery_world_name, RealtimeUserImageResolverSlot};
pub use generic_webhook::{filter_generic_webhook_payload, generic_webhook_payload};
pub use image_file::{extract_file_version, fallback_file_version};
pub use localization::{
    discord_embed_kind, discord_title_key, DiscordEmbedKind, OverlayLocale, OverlayLocalizer,
};
pub use ports::{
    CachedNotificationUserImageResolver, NotificationConfig, NotificationRemote,
    NotificationRemoteFuture, NotificationWebhookFuture, NotificationWebhookTransport,
    NotificationWebhookTransportError,
};
pub use preferences::{config_bool, parse_webhook_fields, NotificationWebhookFormat};
pub use rendered::RenderedNotification;
pub use rendering::{load_notification_locale, render_delivery};
pub use user_image::{normalize_avatar_image_url_128, UserImageCache};
pub use webhook::{
    discord_webhook_url_with_wait, send_json_webhook_with_retry, webhook_local_time_string,
    WebhookDeliveryFailure, WebhookDeliveryFailureKind, WebhookDeliveryOutcome,
};
pub use webhook_delivery::WebhookDeliveryMonitor;
pub use webhook_delivery::{
    WebhookDeliveryChannelSnapshot, WebhookDeliveryRecord, WebhookDeliverySnapshot,
};
pub use webhook_sink::{NotificationWebhookSink, NotificationWebhookSinkDeps};
