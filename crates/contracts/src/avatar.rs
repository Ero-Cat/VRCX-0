use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AvatarTimeSpentOutput {
    pub avatar_id: String,
    pub time_spent: i64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AvatarTagOutput {
    pub avatar_id: String,
    pub tag: String,
    pub color: Value,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AvatarUsageRow {
    pub avatar_id: String,
    pub name: String,
    pub thumbnail_image_url: String,
    pub image_url: String,
    pub time_spent: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AvatarWearSegment {
    pub avatar_id: String,
    pub name: String,
    pub thumbnail_image_url: String,
    pub image_url: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
}
