use std::sync::Arc;
use vrcx_0_application::social::SavedGroupFavoritesPort;
use vrcx_0_application_activity::ActivityRouter;
use vrcx_0_application_core::Result;
use vrcx_0_contracts::SavedGroupFavoritesSnapshot;
use vrcx_0_core::OwnerId;
use vrcx_0_persistence::{saved_group_favorites, DatabaseService};

pub struct LocalSavedGroupFavoritesAdapter {
    db: Arc<DatabaseService>,
    activity_router: ActivityRouter,
}
impl LocalSavedGroupFavoritesAdapter {
    pub fn new(db: Arc<DatabaseService>, activity_router: ActivityRouter) -> Self {
        Self {
            db,
            activity_router,
        }
    }
}
impl SavedGroupFavoritesPort for LocalSavedGroupFavoritesAdapter {
    fn snapshot(&self, owner: &OwnerId) -> Result<SavedGroupFavoritesSnapshot> {
        Ok(saved_group_favorites::snapshot(&self.db, owner)?)
    }
    fn create_collection(&self, owner: &OwnerId, collection_id: &str, name: &str) -> Result<i64> {
        Ok(saved_group_favorites::create_collection(
            &self.db,
            owner,
            collection_id,
            name,
        )?)
    }
    fn delete_collection(&self, owner: &OwnerId, collection_id: &str) -> Result<i64> {
        Ok(saved_group_favorites::delete_collection(
            &self.db,
            owner,
            collection_id,
        )?)
    }
    fn add_group(&self, owner: &OwnerId, collection_id: &str, group_id: &str) -> Result<i64> {
        Ok(saved_group_favorites::add_group(
            &self.db,
            owner,
            collection_id,
            group_id,
        )?)
    }
    fn remove_group(&self, owner: &OwnerId, group_id: &str) -> Result<i64> {
        Ok(saved_group_favorites::remove_group(
            &self.db, owner, group_id,
        )?)
    }
    fn favorites_changed(&self) {
        self.activity_router.invalidate_group_notification_inputs();
    }
}
