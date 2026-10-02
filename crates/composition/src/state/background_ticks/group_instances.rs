use std::sync::{atomic::AtomicBool, Arc, Mutex};

use vrcx_0_application::game::{
    refresh_background_group_instances, refresh_background_group_instances_for_group,
    RuntimeGroupInstancesProjection,
};
use vrcx_0_application::social::{
    BACKGROUND_GROUP_INSTANCE_NOTIFICATION_CADENCE_SECONDS,
    BACKGROUND_GROUP_INSTANCE_NOTIFICATION_REFRESH_JOB,
};
use vrcx_0_application_core::BackgroundCapabilitySessionIdentity;

use crate::{GroupOrderSource, RuntimeHostContext};

use super::super::{
    background_capability_session_identity, background_capability_session_matches,
    emit_background_info, emit_background_warning, gui_maintenance_runtime_mode,
    AuthenticatedSessionProjection, SharedAtomicFlagGuard,
    BACKGROUND_GROUP_INSTANCE_CADENCE_SECONDS, BACKGROUND_GROUP_INSTANCE_REFRESH_JOB,
};
use super::BackgroundTickContext;

pub(in crate::state) async fn run_background_group_instance_refresh(
    context: &BackgroundTickContext<'_>,
    refresh_running: &Arc<AtomicBool>,
    group_order_source: &dyn GroupOrderSource,
) {
    let Some(_refresh_guard) = SharedAtomicFlagGuard::try_acquire(refresh_running) else {
        context.background_jobs.mark_scheduled(
            BACKGROUND_GROUP_INSTANCE_REFRESH_JOB,
            "Background group instance refresh is already running.",
            BACKGROUND_GROUP_INSTANCE_CADENCE_SECONDS,
        );
        return;
    };
    context.background_jobs.mark_running(
        BACKGROUND_GROUP_INSTANCE_REFRESH_JOB,
        "Refreshing background group instance facts.",
    );
    let Some(session) = background_capability_session_identity(context.session_slot) else {
        context.background_jobs.mark_scheduled(
            BACKGROUND_GROUP_INSTANCE_REFRESH_JOB,
            "Background group instance refresh is waiting for an authenticated session.",
            BACKGROUND_GROUP_INSTANCE_CADENCE_SECONDS,
        );
        return;
    };
    context
        .runtime_context
        .event_bus
        .emit(RuntimeGroupInstancesProjection::running(
            session.current_user_id.clone(),
            session.endpoint.clone(),
        ));
    let remote =
        vrcx_0_outbound_adapters::VrchatBackgroundGroupRemote::new(Arc::clone(context.web));
    match refresh_background_group_instances(&remote, &session).await {
        Ok(refresh) => {
            if !background_capability_session_matches(context.session_slot, &session) {
                tracing::warn!("ignored stale background group instance refresh");
                emit_stale_group_instance_refresh_idle(
                    context.session_slot,
                    context.runtime_context,
                    &session,
                );
                context.background_jobs.mark_scheduled(
                    BACKGROUND_GROUP_INSTANCE_REFRESH_JOB,
                    "Stale background group instance refresh ignored.",
                    BACKGROUND_GROUP_INSTANCE_CADENCE_SECONDS,
                );
                return;
            }
            let count = refresh.instances.len();
            context
                .runtime_context
                .event_bus
                .emit(RuntimeGroupInstancesProjection::ready(
                    session.current_user_id.clone(),
                    session.endpoint.clone(),
                    refresh.fetched_at,
                    refresh.instances,
                    group_order_source.read_group_order(&session.current_user_id),
                ));
            let detail = format!("group instance facts refreshed: {count} rows.");
            emit_background_info(
                context.runtime_context,
                context.backend_runtime,
                detail.clone(),
            );
            context
                .background_jobs
                .mark_completed(BACKGROUND_GROUP_INSTANCE_REFRESH_JOB, detail);
        }
        Err(error) => {
            if !background_capability_session_matches(context.session_slot, &session) {
                tracing::warn!("ignored stale background group instance refresh error");
                emit_stale_group_instance_refresh_idle(
                    context.session_slot,
                    context.runtime_context,
                    &session,
                );
                context.background_jobs.mark_scheduled(
                    BACKGROUND_GROUP_INSTANCE_REFRESH_JOB,
                    "Stale background group instance refresh error ignored.",
                    BACKGROUND_GROUP_INSTANCE_CADENCE_SECONDS,
                );
                return;
            }
            tracing::warn!(
                runtime_mode = %gui_maintenance_runtime_mode(context.backend_runtime),
                error = %error,
                "GUI maintenance group instance network request failed"
            );
            context
                .runtime_context
                .event_bus
                .emit(RuntimeGroupInstancesProjection::failed(
                    session.current_user_id.clone(),
                    session.endpoint.clone(),
                    error.to_string(),
                ));
            emit_background_warning(
                context.runtime_context,
                context.backend_runtime,
                format!("group instance refresh failed: {error}."),
            );
            context
                .background_jobs
                .mark_failed(BACKGROUND_GROUP_INSTANCE_REFRESH_JOB, error.to_string());
        }
    }
    context.background_jobs.mark_scheduled(
        BACKGROUND_GROUP_INSTANCE_REFRESH_JOB,
        "Next background group instance facts refresh is waiting.",
        BACKGROUND_GROUP_INSTANCE_CADENCE_SECONDS,
    );
}

pub(in crate::state) async fn run_background_group_instance_notification_refresh(
    context: &BackgroundTickContext<'_>,
    refresh_running: &Arc<AtomicBool>,
    group_ids: &[String],
) {
    let Some(_refresh_guard) = SharedAtomicFlagGuard::try_acquire(refresh_running) else {
        return;
    };
    let Some(session) = background_capability_session_identity(context.session_slot) else {
        return;
    };
    context.background_jobs.mark_running(
        BACKGROUND_GROUP_INSTANCE_NOTIFICATION_REFRESH_JOB,
        "Checking saved groups for new instances.",
    );
    let scope_key = format!(
        "{}\u{1f}{}\u{1f}{}",
        session.current_user_id, session.endpoint, session.auth_scope_generation
    );
    let remote =
        vrcx_0_outbound_adapters::VrchatBackgroundGroupRemote::new(Arc::clone(context.web));
    let mut failed = 0usize;
    for group_id in group_ids {
        match refresh_background_group_instances_for_group(&remote, &session, group_id).await {
            Ok(refresh) => {
                if !background_capability_session_matches(context.session_slot, &session) {
                    return;
                }
                if let Some(event) = context.runtime_context.group_instance_monitor().scan(
                    &scope_key,
                    group_id,
                    &refresh.fetched_at,
                    &refresh.instances,
                ) {
                    context.runtime_context.activity_router().ingest(event);
                }
            }
            Err(error) => {
                failed = failed.saturating_add(1);
                tracing::warn!(group_id, error = %error, "saved group instance refresh failed");
            }
        }
    }
    if failed == 0 {
        context.background_jobs.mark_completed(
            BACKGROUND_GROUP_INSTANCE_NOTIFICATION_REFRESH_JOB,
            format!("checked {} saved groups for new instances", group_ids.len()),
        );
    } else {
        context.background_jobs.mark_failed(
            BACKGROUND_GROUP_INSTANCE_NOTIFICATION_REFRESH_JOB,
            format!("failed to check {failed} saved groups for new instances"),
        );
    }
    context.background_jobs.mark_scheduled(
        BACKGROUND_GROUP_INSTANCE_NOTIFICATION_REFRESH_JOB,
        "Next saved group instance notification check is waiting.",
        BACKGROUND_GROUP_INSTANCE_NOTIFICATION_CADENCE_SECONDS,
    );
}

fn emit_stale_group_instance_refresh_idle(
    session_slot: &Arc<Mutex<AuthenticatedSessionProjection>>,
    runtime_context: &Arc<RuntimeHostContext>,
    session: &BackgroundCapabilitySessionIdentity,
) {
    let same_scope = background_capability_session_identity(session_slot)
        .map(|current| {
            current.current_user_id == session.current_user_id
                && current.endpoint == session.endpoint
        })
        .unwrap_or(false);
    if same_scope {
        runtime_context
            .event_bus
            .emit(RuntimeGroupInstancesProjection::idle_preserving_entries(
                session.current_user_id.clone(),
                session.endpoint.clone(),
            ));
        return;
    }
    runtime_context
        .event_bus
        .emit(RuntimeGroupInstancesProjection::idle_clearing_entries(
            session.current_user_id.clone(),
            session.endpoint.clone(),
        ));
}
