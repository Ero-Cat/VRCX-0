use super::{AuthenticatedRuntimeSession, AuthenticatedSessionProjection, RuntimeHostState};
use vrcx_0_application::auth::{
    clear_authenticated_session_projection, establish_authenticated_session_projection,
};
use vrcx_0_application::game::RuntimeGroupInstancesProjection;

impl RuntimeHostState {
    pub fn authenticated_session_projection(&self) -> AuthenticatedSessionProjection {
        self.authenticated_session_projection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn clear_authenticated_session_projection(&self) {
        let cleared = {
            let mut current = self
                .authenticated_session_projection
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            current.session.clone().map(|previous| {
                *current = clear_authenticated_session_projection(&current);
                (previous, current.clone())
            })
        };
        if let Some((_, projection)) = &cleared {
            self.runtime_context.event_bus.emit(projection.clone());
        }
        self.authenticated_runtime.stop();
        self.runtime_context.clear_activity_runtime_state();
        self.runtime_context.session.clear_realtime_context();
        if let Some((previous, _)) = cleared {
            self.runtime_context
                .event_bus
                .emit(RuntimeGroupInstancesProjection::cleared_session(
                    previous.user_id,
                    previous.endpoint,
                ));
        }
    }

    pub(super) fn establish_authenticated_session_projection(
        &self,
        session: &AuthenticatedRuntimeSession,
        auth_scope_generation: u64,
    ) {
        let published = {
            let mut current = self
                .authenticated_session_projection
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let next = establish_authenticated_session_projection(
                &current,
                session,
                auth_scope_generation,
            );
            let scope_changed = current
                .session
                .as_ref()
                .zip(next.session.as_ref())
                .map(|current| {
                    current.0.user_id != current.1.user_id
                        || current.0.endpoint != current.1.endpoint
                        || current.0.websocket != current.1.websocket
                })
                .unwrap_or(true);
            if scope_changed {
                self.runtime_context.clear_activity_runtime_state();
            }
            *current = next;
            current.clone()
        };
        self.runtime_context.event_bus.emit(published);
    }
}
