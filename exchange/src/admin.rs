use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminAuditEntry {
    pub audit_id: Uuid,
    pub actor_username: String,
    pub action: String,
    pub target_username: Option<String>,
    pub target_trader_id: Option<Uuid>,
    pub details: String,
    pub occurred_at: DateTime<Utc>,
}
