use std::sync::Arc;

use vrcx_0_application_activity::ActivityRouter;
use vrcx_0_application_core::{RuntimeAuthScope, TaskSupervisor, WebClient, WorldCache};
use vrcx_0_application_game::{NowPlayingSnapshot, RuntimeSnapshot};
use vrcx_0_persistence::config::ConfigRepository;

pub trait VrOverlayRuntimeServices: Send + Sync {
    fn config(&self) -> &ConfigRepository;

    fn web_client(&self) -> &Arc<WebClient>;

    fn auth_scope(&self) -> &RuntimeAuthScope;

    fn world_cache(&self) -> &Arc<WorldCache>;

    fn tasks(&self) -> &TaskSupervisor;

    fn activity_router(&self) -> ActivityRouter;

    fn hmd_notifications_allowed(&self) -> bool;

    fn notification_friend_image(&self, endpoint: &str, user_id: &str) -> Option<String>;

    fn set_hmd_afk(&self, is_hmd_afk: bool);

    fn game_log_snapshot(&self) -> RuntimeSnapshot;

    fn now_playing(&self) -> NowPlayingSnapshot;
}
