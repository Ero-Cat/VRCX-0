use std::future::Future;
use std::time::Duration;

use crate::notification::NotificationResolver;
use crate::ActivityDelivery;
use vrcx_0_core::files::extract_file_id;

const DISCORD_RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct DiscordDeps<'a> {
    pub(crate) resolver: &'a NotificationResolver,
    pub(crate) endpoint: &'a str,
}

async fn within_timeout(lookup: impl Future<Output = Option<String>>) -> String {
    tokio::time::timeout(DISCORD_RESOLVE_TIMEOUT, lookup)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub(super) async fn resolve_avatar_name(
    deps: &DiscordDeps<'_>,
    delivery: &ActivityDelivery,
) -> String {
    let Some(file_id) = extract_file_id(&delivery.entry.content.image_url) else {
        return String::new();
    };
    within_timeout(deps.resolver.avatar_name(deps.endpoint, &file_id)).await
}

pub(super) async fn resolve_actor_icon_url(
    deps: &DiscordDeps<'_>,
    delivery: &ActivityDelivery,
) -> String {
    let actor = delivery.entry.actor_user_id.trim();
    if actor.is_empty() {
        return String::new();
    }
    within_timeout(deps.resolver.user_image(deps.endpoint, actor)).await
}

pub(super) async fn resolve_world_thumbnail_url(
    deps: &DiscordDeps<'_>,
    delivery: &ActivityDelivery,
) -> String {
    within_timeout(deps.resolver.world_image_url(deps.endpoint, delivery)).await
}
