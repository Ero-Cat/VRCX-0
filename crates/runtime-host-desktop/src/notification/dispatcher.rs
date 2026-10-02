use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use vrcx_0_application_activity::notification::{
    extract_file_version, fallback_file_version, render_delivery, NotificationResolver,
    OverlayLocale, RenderedNotification,
};
use vrcx_0_application_activity::{ActivityDelivery, ActivitySink, ActivitySnapshot};
use vrcx_0_application_core::{ImageCache, RuntimeAuthScope, TaskSupervisor};
use vrcx_0_host_desktop::tts::TtsEngine;
use vrcx_0_persistence::DatabaseService;

use super::desktop::{send_desktop_notification, DesktopNotificationAction, DesktopNotifier};
use super::overlay_transport::OverlayNotificationTransport;
use super::policy::LocalNotificationPolicy;
use super::tts::{notification_tts_memo_actor_user_id, send_tts_notification};
use super::{NotificationDeliveryPlan, NotificationDeliveryPreferences};
use vrcx_0_contracts::activity::ActivityKind;
use vrcx_0_core::files::extract_file_id;
use vrcx_0_core::location::is_meaningful_world_name;
use vrcx_0_core::OwnerId;

const NOTIFICATION_IMAGE_FIRST_SEND_BUDGET: Duration = Duration::from_secs(1);
const NOTIFICATION_USER_IMAGE_FIRST_SEND_BUDGET: Duration = Duration::from_secs(2);
const NOTIFICATION_WORLD_NAME_FIRST_SEND_BUDGET: Duration = Duration::from_secs(2);

pub struct NotificationDispatcher {
    auth_scope: RuntimeAuthScope,
    image_cache: Arc<ImageCache>,
    resolver: Arc<NotificationResolver>,
    output: Arc<NotificationOutputContext>,
    completion_tx: mpsc::UnboundedSender<NotificationCompletion>,
    next_sequence: AtomicU64,
    tasks: TaskSupervisor,
}

pub(crate) struct NotificationDispatcherDeps {
    pub auth_scope: RuntimeAuthScope,
    pub db: Arc<DatabaseService>,
    pub image_cache: Arc<ImageCache>,
    pub resolver: Arc<NotificationResolver>,
    pub desktop: Arc<dyn DesktopNotifier>,
    pub tts: Arc<dyn TtsEngine>,
    pub tasks: TaskSupervisor,
    pub policy: Arc<LocalNotificationPolicy>,
}

struct NotificationOutputContext {
    overlay_transport: OverlayNotificationTransport,
    db: Arc<DatabaseService>,
    desktop: Arc<dyn DesktopNotifier>,
    tts: Arc<dyn TtsEngine>,
    policy: Arc<LocalNotificationPolicy>,
}

struct NotificationJob {
    delivery: ActivityDelivery,
    preferences: NotificationDeliveryPreferences,
    plan: NotificationDeliveryPlan,
    locale: OverlayLocale,
    endpoint: String,
    current_user_id: String,
}

struct PreparedNotification {
    delivery: ActivityDelivery,
    preferences: NotificationDeliveryPreferences,
    plan: NotificationDeliveryPlan,
    render: RenderedNotification,
    locale: OverlayLocale,
    local_image: Option<String>,
    desktop_action: Option<DesktopNotificationAction>,
}

enum NotificationCompletion {
    Ready {
        sequence: u64,
        notification: Box<PreparedNotification>,
    },
    Skip {
        sequence: u64,
    },
}

struct OrderedDeliveryBuffer<T> {
    next_sequence: u64,
    pending: BTreeMap<u64, Option<T>>,
}

impl<T> OrderedDeliveryBuffer<T> {
    fn new(next_sequence: u64) -> Self {
        Self {
            next_sequence,
            pending: BTreeMap::new(),
        }
    }

    fn push(&mut self, sequence: u64, value: Option<T>) -> Vec<T> {
        if sequence < self.next_sequence {
            return Vec::new();
        }
        self.pending.insert(sequence, value);
        let mut ready = Vec::new();
        while let Some(value) = self.pending.remove(&self.next_sequence) {
            self.next_sequence = self.next_sequence.saturating_add(1);
            if let Some(value) = value {
                ready.push(value);
            }
        }
        ready
    }
}

impl NotificationDispatcher {
    pub(crate) fn new(deps: NotificationDispatcherDeps) -> Self {
        let output = Arc::new(NotificationOutputContext {
            overlay_transport: OverlayNotificationTransport::new(),
            db: deps.db,
            desktop: deps.desktop,
            tts: deps.tts,
            policy: deps.policy,
        });
        let (completion_tx, completion_rx) = mpsc::unbounded_channel();
        let worker_output = Arc::clone(&output);
        deps.tasks.spawn(async move {
            run_ordered_output(completion_rx, worker_output).await;
        });
        Self {
            auth_scope: deps.auth_scope,
            image_cache: deps.image_cache,
            resolver: deps.resolver,
            output,
            completion_tx,
            next_sequence: AtomicU64::new(0),
            tasks: deps.tasks,
        }
    }
}

impl ActivitySink for NotificationDispatcher {
    fn emit_overlay_activity_snapshot(&self, _snapshot: ActivitySnapshot) {}

    fn emit_overlay_activity_delivery(&self, delivery: ActivityDelivery) {
        let settings = self.output.policy.settings();
        let plan = self.output.policy.plan(&delivery, &settings);
        if !plan.has_local_transport() {
            return;
        }
        let preferences = settings.preferences.clone();
        let locale = settings.locale;
        let (endpoint, current_user_id) = notification_session_identity(&self.auth_scope);
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let priority = delivery.entry.kind == ActivityKind::OnPlayerJoining;
        let mut delivery = delivery;
        if !priority {
            if let Some(actor_user_id) = image_actor_user_id(&delivery, &current_user_id) {
                if let Some(image_url) = self.resolver.cached_user_image(&endpoint, actor_user_id) {
                    delivery.entry.content.image_url = image_url;
                }
            }
        }
        let job = NotificationJob {
            delivery,
            preferences,
            plan,
            locale,
            endpoint,
            current_user_id,
        };

        if priority {
            let render = render_delivery(
                &job.delivery,
                job.locale,
                job.preferences.show_instance_id_in_location,
            );
            let prepared = prepare_rendered_notification(job, render, None);
            dispatch_prepared_notification(&prepared, self.output.as_ref());
            let _ = self
                .completion_tx
                .send(NotificationCompletion::Skip { sequence });
            return;
        }

        let image_cache = Arc::clone(&self.image_cache);
        let resolver = Arc::clone(&self.resolver);
        let tasks = self.tasks.clone();
        let completion_tx = self.completion_tx.clone();
        self.tasks.spawn(async move {
            let notification = prepare_notification(job, image_cache, resolver, tasks).await;
            let _ = completion_tx.send(NotificationCompletion::Ready {
                sequence,
                notification: Box::new(notification),
            });
        });
    }
}

async fn run_ordered_output(
    mut completion_rx: mpsc::UnboundedReceiver<NotificationCompletion>,
    output: Arc<NotificationOutputContext>,
) {
    let mut buffer = OrderedDeliveryBuffer::new(0);
    while let Some(completion) = completion_rx.recv().await {
        let (sequence, notification) = match completion {
            NotificationCompletion::Ready {
                sequence,
                notification,
            } => (sequence, Some(notification)),
            NotificationCompletion::Skip { sequence } => (sequence, None),
        };
        for notification in buffer.push(sequence, notification) {
            dispatch_prepared_notification(&notification, output.as_ref());
        }
    }
}

async fn prepare_notification(
    mut job: NotificationJob,
    image_cache: Arc<ImageCache>,
    resolver: Arc<NotificationResolver>,
    tasks: TaskSupervisor,
) -> PreparedNotification {
    let needs_local_image = job.preferences.image_notifications && job.plan.needs_local_image();
    let (actor_image, world) = tokio::join!(
        async {
            if needs_local_image {
                resolve_actor_image_with_budget(&tasks, Arc::clone(&resolver), &job).await
            } else {
                None
            }
        },
        resolve_world_name_with_budget(&tasks, Arc::clone(&resolver), &job),
    );
    if let Some(image_url) = actor_image {
        job.delivery.entry.content.image_url = image_url;
    }
    if let Some((world_name, display_location)) = world {
        job.delivery.entry.content.world_name = world_name;
        if !display_location.trim().is_empty() {
            job.delivery.entry.content.display_location = display_location;
        }
    }
    let render = render_delivery(
        &job.delivery,
        job.locale,
        job.preferences.show_instance_id_in_location,
    );
    let local_image = if needs_local_image {
        resolve_local_image_with_budget(&tasks, image_cache, &render.image_url).await
    } else {
        None
    };
    prepare_rendered_notification(job, render, local_image)
}

fn prepare_rendered_notification(
    job: NotificationJob,
    render: RenderedNotification,
    local_image: Option<String>,
) -> PreparedNotification {
    let owner_user_id = OwnerId::new(job.current_user_id);
    let desktop_action = if job.delivery.entry.kind == ActivityKind::GroupInstanceOpened {
        DesktopNotificationAction::open_group_profile(
            &owner_user_id,
            &job.delivery.entry.content.group_id,
        )
    } else {
        DesktopNotificationAction::open_user_profile(
            &owner_user_id,
            &job.delivery.entry.actor_user_id,
        )
    };
    PreparedNotification {
        delivery: job.delivery,
        preferences: job.preferences,
        plan: job.plan,
        render,
        locale: job.locale,
        local_image,
        desktop_action,
    }
}

fn dispatch_prepared_notification(
    notification: &PreparedNotification,
    output: &NotificationOutputContext,
) {
    if output.policy.paused(&output.policy.settings()) {
        return;
    }
    let plan = notification.plan;
    if plan.tts {
        let user_memo = notification_tts_memo_actor_user_id(
            &notification.delivery,
            &notification.render,
            &notification.preferences,
            notification.locale,
        )
        .and_then(|actor_user_id| load_user_memo(output.db.as_ref(), actor_user_id));
        send_tts_notification(
            output.tts.as_ref(),
            &notification.delivery,
            &notification.render,
            &notification.preferences,
            notification.locale,
            user_memo.as_deref(),
        );
    }
    let local_image = notification.local_image.as_deref();
    if plan.desktop {
        send_desktop_notification(
            output.desktop.as_ref(),
            &notification.render,
            notification.delivery.entry.kind,
            &notification.delivery.entry.content.group_name,
            &notification.preferences,
            local_image,
            notification.desktop_action.as_ref(),
        );
    }
    output.overlay_transport.send(
        plan,
        &notification.render,
        &notification.preferences,
        local_image,
    );
}

fn load_user_memo(db: &DatabaseService, actor_user_id: &str) -> Option<String> {
    let actor_user_id = actor_user_id.trim();
    if actor_user_id.is_empty() {
        return None;
    }
    match vrcx_0_persistence::memos::memo_get_user(db, actor_user_id.to_string()) {
        Ok(Some(memo)) => Some(memo.memo),
        Ok(None) => None,
        Err(error) => {
            tracing::debug!("failed to load TTS nickname memo: {error}");
            None
        }
    }
}

fn image_actor_user_id<'a>(
    delivery: &'a ActivityDelivery,
    current_user_id: &str,
) -> Option<&'a str> {
    if !delivery.entry.content.image_url.trim().is_empty() {
        return None;
    }
    let actor_user_id = delivery.entry.actor_user_id.trim();
    (actor_user_id.starts_with("usr_") && actor_user_id != current_user_id.trim())
        .then_some(actor_user_id)
}

async fn resolve_actor_image_with_budget(
    tasks: &TaskSupervisor,
    resolver: Arc<NotificationResolver>,
    job: &NotificationJob,
) -> Option<String> {
    let actor_user_id = image_actor_user_id(&job.delivery, &job.current_user_id)?.to_string();
    let endpoint = job.endpoint.clone();
    let (result_tx, result_rx) = oneshot::channel();
    tasks.spawn(async move {
        let result = resolver.user_image(&endpoint, &actor_user_id).await;
        let _ = result_tx.send(result);
    });
    tokio::time::timeout(NOTIFICATION_USER_IMAGE_FIRST_SEND_BUDGET, result_rx)
        .await
        .ok()?
        .ok()?
}

async fn resolve_world_name_with_budget(
    tasks: &TaskSupervisor,
    resolver: Arc<NotificationResolver>,
    job: &NotificationJob,
) -> Option<(String, String)> {
    if is_meaningful_world_name(&job.delivery.entry.content.world_name) {
        return None;
    }
    let endpoint = job.endpoint.clone();
    let delivery = job.delivery.clone();
    let (result_tx, result_rx) = oneshot::channel();
    tasks.spawn(async move {
        let result = resolver.world_name(&endpoint, &delivery).await;
        let _ = result_tx.send(result);
    });
    tokio::time::timeout(NOTIFICATION_WORLD_NAME_FIRST_SEND_BUDGET, result_rx)
        .await
        .ok()?
        .ok()?
}

fn notification_session_identity(auth_scope: &RuntimeAuthScope) -> (String, String) {
    let auth_scope = auth_scope.snapshot();
    if auth_scope.active {
        return (auth_scope.endpoint, auth_scope.current_user_id);
    }
    Default::default()
}

#[derive(Clone)]
struct LocalImageRequest {
    url: String,
    file_id: String,
    version: String,
}

fn local_image_request(image_url: &str) -> Option<LocalImageRequest> {
    let url = image_url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let file_id = extract_file_id(url)?;
    let version = extract_file_version(url, &file_id).unwrap_or_else(|| fallback_file_version(url));
    if version.is_empty() {
        return None;
    }
    Some(LocalImageRequest {
        url: url.to_string(),
        file_id,
        version,
    })
}

async fn resolve_local_image_with_budget(
    tasks: &TaskSupervisor,
    image_cache: Arc<ImageCache>,
    image_url: &str,
) -> Option<String> {
    let request = local_image_request(image_url)?;
    let (result_tx, result_rx) = oneshot::channel();
    tasks.spawn(async move {
        let result = fetch_local_image(image_cache, request).await;
        let _ = result_tx.send(result);
    });
    tokio::time::timeout(NOTIFICATION_IMAGE_FIRST_SEND_BUDGET, result_rx)
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
}

async fn fetch_local_image(
    image_cache: Arc<ImageCache>,
    request: LocalImageRequest,
) -> Option<String> {
    image_cache
        .get_image(&request.url, &request.file_id, &request.version)
        .await
        .ok()
}

#[cfg(test)]
mod tests;
