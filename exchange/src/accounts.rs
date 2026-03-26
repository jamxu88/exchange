use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    #[default]
    Trader,
    Admin,
}

impl UserRole {
    pub fn has_unlimited_position_power(self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserProfile {
    pub trader_id: Uuid,
    pub username: String,
    pub api_key: String,
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub profile: UserProfile,
}
