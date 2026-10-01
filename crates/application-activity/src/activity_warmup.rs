use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use vrcx_0_application_core::{Result, RuntimeAuthScope, TaskSupervisor};
use vrcx_0_core::OwnerId;

const ACTIVITY_WARMUP_RANGE_DAYS: i64 = 365;
const ACTIVITY_PAGE_WARMUP_RANGE_DAYS: [i64; 5] = [30, 90, 180, 365, 0];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivitySessionWarmupOutput {
    pub cached_range_days: i64,
    pub source_count: usize,
    pub session_count: usize,
}

pub trait ActivitySessionWarmupStore: Send + Sync {
    fn warm_self_sessions(
        &self,
        owner_user_id: OwnerId,
        range_days: i64,
    ) -> Result<ActivitySessionWarmupOutput>;
}

pub trait ActivityPageWarmupStore: Send + Sync {
    fn warm_activity_page(&self, owner_user_id: OwnerId, range_days: i64) -> Result<()>;
}

pub struct ActivityWarmupRuntime {
    auth_scope: RuntimeAuthScope,
    tasks: TaskSupervisor,
    store: Arc<dyn ActivitySessionWarmupStore>,
    page_store: Arc<dyn ActivityPageWarmupStore>,
    scheduled_generation: Arc<AtomicU64>,
}

impl ActivityWarmupRuntime {
    pub fn new(
        auth_scope: RuntimeAuthScope,
        tasks: TaskSupervisor,
        store: Arc<dyn ActivitySessionWarmupStore>,
        page_store: Arc<dyn ActivityPageWarmupStore>,
    ) -> Self {
        Self {
            auth_scope,
            tasks,
            store,
            page_store,
            scheduled_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn schedule(&self, user_id: String, auth_generation: u64) {
        if !activity_warmup_scope_matches(&self.auth_scope, &user_id, auth_generation)
            || !claim_activity_warmup_generation(
                self.scheduled_generation.as_ref(),
                auth_generation,
            )
        {
            return;
        }
        let auth_scope = self.auth_scope.clone();
        let store = Arc::clone(&self.store);
        let page_store = Arc::clone(&self.page_store);
        let scheduled_generation = Arc::clone(&self.scheduled_generation);
        self.tasks.spawn_thread("activity-session-warmup", move || {
            if !activity_warmup_scope_matches(&auth_scope, &user_id, auth_generation) {
                release_activity_warmup_generation(scheduled_generation.as_ref(), auth_generation);
                return;
            }
            match store
                .warm_self_sessions(OwnerId::new(user_id.clone()), ACTIVITY_WARMUP_RANGE_DAYS)
            {
                Ok(output) => tracing::debug!(
                    user_id = %user_id,
                    cached_range_days = output.cached_range_days,
                    source_count = output.source_count,
                    session_count = output.session_count,
                    "activity session warmup completed"
                ),
                Err(error) => {
                    tracing::warn!(
                        user_id = %user_id,
                        error = %error,
                        "activity session warmup failed"
                    );
                    release_activity_warmup_generation(
                        scheduled_generation.as_ref(),
                        auth_generation,
                    );
                }
            }
            warm_activity_pages(page_store.as_ref(), &auth_scope, &user_id, auth_generation);
        });
    }
}

fn warm_activity_pages(
    page_store: &dyn ActivityPageWarmupStore,
    auth_scope: &RuntimeAuthScope,
    user_id: &str,
    auth_generation: u64,
) {
    for range_days in ACTIVITY_PAGE_WARMUP_RANGE_DAYS {
        if !activity_warmup_scope_matches(auth_scope, user_id, auth_generation) {
            return;
        }
        match page_store.warm_activity_page(OwnerId::new(user_id.to_string()), range_days) {
            Ok(()) => tracing::debug!(
                user_id = %user_id,
                range_days,
                "activity page warmup completed"
            ),
            Err(error) => tracing::warn!(
                user_id = %user_id,
                range_days,
                error = %error,
                "activity page warmup failed"
            ),
        }
    }
}

fn claim_activity_warmup_generation(scheduled: &AtomicU64, auth_generation: u64) -> bool {
    if auth_generation == 0 {
        return false;
    }
    let mut current = scheduled.load(Ordering::Acquire);
    loop {
        if current >= auth_generation {
            return false;
        }
        match scheduled.compare_exchange(
            current,
            auth_generation,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
}

fn release_activity_warmup_generation(scheduled: &AtomicU64, auth_generation: u64) {
    let _ = scheduled.compare_exchange(auth_generation, 0, Ordering::AcqRel, Ordering::Acquire);
}

fn activity_warmup_scope_matches(
    auth_scope: &RuntimeAuthScope,
    user_id: &str,
    auth_generation: u64,
) -> bool {
    let current = auth_scope.snapshot();
    current.active && current.current_user_id == user_id && current.generation == auth_generation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_generation_is_claimed_once_per_auth_scope() {
        let scheduled = AtomicU64::new(0);

        assert!(claim_activity_warmup_generation(&scheduled, 1));
        assert!(!claim_activity_warmup_generation(&scheduled, 1));
        assert!(claim_activity_warmup_generation(&scheduled, 2));
        assert!(!claim_activity_warmup_generation(&scheduled, 1));
        release_activity_warmup_generation(&scheduled, 1);
        assert_eq!(scheduled.load(Ordering::Acquire), 2);
    }

    #[test]
    fn warmup_scope_rejects_account_switches_and_cleared_auth() {
        let auth_scope = RuntimeAuthScope::new();
        let first = auth_scope.set("usr_first", "");
        assert!(activity_warmup_scope_matches(
            &auth_scope,
            "usr_first",
            first.generation
        ));

        auth_scope.set("usr_second", "");
        assert!(!activity_warmup_scope_matches(
            &auth_scope,
            "usr_first",
            first.generation
        ));

        let cleared = auth_scope.set("", "");
        assert!(!activity_warmup_scope_matches(
            &auth_scope,
            "",
            cleared.generation
        ));
    }
}
