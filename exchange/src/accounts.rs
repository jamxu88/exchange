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

fn default_team_number() -> String {
    String::new()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserProfile {
    pub trader_id: Uuid,
    pub username: String,
    #[serde(default = "default_team_number")]
    pub team_number: String,
    pub api_key: String,
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
}

impl UserProfile {
    pub fn public_team_number(&self) -> &str {
        if self.team_number.trim().is_empty() {
            &self.username
        } else {
            &self.team_number
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct PublicUserProfile {
    pub trader_id: Uuid,
    pub team_number: String,
}

impl From<&UserProfile> for PublicUserProfile {
    fn from(profile: &UserProfile) -> Self {
        Self {
            trader_id: profile.trader_id,
            team_number: profile.public_team_number().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub profile: UserProfile,
}
