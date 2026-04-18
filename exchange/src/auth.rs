use crate::accounts::{UserProfile, UserRecord, UserRole};
use crate::admin::AdminAuditEntry;
use crate::state::AppState;
use crate::storage::StorageError;
use axum::extract::FromRequestParts;
use axum::http::{StatusCode, header, request::Parts};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;
use utoipa::ToSchema;
use uuid::Uuid;

const API_KEY_HEADER: &str = "x-api-key";
const API_KEY_LENGTH: usize = 7;
const API_KEY_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn generate_api_key() -> String {
    let uuid = Uuid::new_v4();
    let bytes = uuid.as_bytes();
    let mut key = String::with_capacity(API_KEY_LENGTH);
    for index in 0..API_KEY_LENGTH {
        let sample = bytes[index] as usize;
        let pick = API_KEY_ALPHABET[sample % API_KEY_ALPHABET.len()];
        key.push(char::from(pick));
    }
    key
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub trader_id: Uuid,
    pub username: String,
    pub team_number: String,
    pub role: UserRole,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedAdmin {
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProvisionUserRequest {
    pub username: String,
    pub team_number: Option<String>,
    pub role: Option<UserRole>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProvisionUserResponse {
    pub profile: UserProfile,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("username is required")]
    MissingUsername,
    #[error("username already exists")]
    UsernameTaken,
    #[error("missing authentication")]
    MissingAuthentication,
    #[error("invalid api key")]
    InvalidApiKey,
    #[error("missing admin authentication")]
    MissingAdminAuthentication,
    #[error("invalid admin token")]
    InvalidAdminToken,
    #[error("failed to generate unique api key")]
    ApiKeyGenerationFailed,
}

impl AuthError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::MissingUsername => StatusCode::BAD_REQUEST,
            Self::UsernameTaken => StatusCode::CONFLICT,
            Self::MissingAuthentication
            | Self::InvalidApiKey
            | Self::MissingAdminAuthentication
            | Self::InvalidAdminToken => StatusCode::UNAUTHORIZED,
            Self::ApiKeyGenerationFailed => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub struct AuthService;

impl AuthService {
    pub fn provision_user(
        state: &AppState,
        request: ProvisionUserRequest,
    ) -> Result<ProvisionUserResponse, AuthError> {
        let username = request.username.trim().to_string();
        let team_number = request
            .team_number
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&username)
            .to_string();
        let role = request.role.unwrap_or_default();
        if username.is_empty() {
            return Err(AuthError::MissingUsername);
        }

        for _ in 0..4 {
            let profile = UserProfile {
                trader_id: Uuid::new_v4(),
                username: username.clone(),
                team_number: team_number.clone(),
                api_key: generate_api_key(),
                role,
                created_at: Utc::now(),
            };
            let record = UserRecord {
                profile: profile.clone(),
            };

            match state.storage.create_user(record) {
                Ok(()) => return Ok(ProvisionUserResponse { profile }),
                Err(StorageError::UsernameTaken) => return Err(AuthError::UsernameTaken),
                Err(StorageError::ApiKeyTaken) => continue,
            }
        }

        Err(AuthError::ApiKeyGenerationFailed)
    }

    pub fn provision_user_as_admin(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        request: ProvisionUserRequest,
    ) -> Result<ProvisionUserResponse, AuthError> {
        let target_username = request.username.trim().to_string();
        let result = Self::provision_user(state, request);

        match &result {
            Ok(response) => Self::record_admin_audit(
                state,
                admin.username.clone(),
                "provision_competition_user_succeeded",
                Some(response.profile.username.clone()),
                Some(response.profile.trader_id),
                format!(
                    "competition account provisioned with role {:?} and api key {}",
                    response.profile.role, response.profile.api_key
                ),
            ),
            Err(error) => Self::record_admin_audit(
                state,
                admin.username.clone(),
                "provision_competition_user_failed",
                Some(target_username),
                None,
                error.to_string(),
            ),
        }

        if result.is_ok() {
            state.request_checkpoint_save();
        }

        result
    }

    pub fn authenticate_api_key(
        state: &AppState,
        api_key: &str,
    ) -> Result<AuthenticatedUser, AuthError> {
        if let Some(user) = state.storage.get_user_by_api_key(api_key) {
            return Ok(authenticated_user_from_record(user));
        }

        Err(AuthError::InvalidApiKey)
    }

    pub fn authenticate_request(
        parts: &Parts,
        state: &AppState,
    ) -> Result<AuthenticatedUser, AuthError> {
        if let Some(auth) = parts.extensions.get::<AuthenticatedUser>() {
            return Ok(auth.clone());
        }

        let api_key = parts
            .headers
            .get(API_KEY_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or(AuthError::MissingAuthentication)?;

        Self::authenticate_api_key(state, api_key)
    }

    fn record_admin_audit(
        state: &AppState,
        actor_username: String,
        action: &str,
        target_username: Option<String>,
        target_trader_id: Option<Uuid>,
        details: impl Into<String>,
    ) {
        let details = details.into();
        let entry = AdminAuditEntry {
            audit_id: Uuid::new_v4(),
            actor_username: actor_username.clone(),
            action: action.to_string(),
            target_username: target_username.clone(),
            target_trader_id,
            details: details.clone(),
            occurred_at: Utc::now(),
        };

        info!(
            actor_username,
            action,
            target_username = ?target_username,
            target_trader_id = ?entry.target_trader_id,
            details,
            "admin audit event"
        );
        state.storage.append_admin_audit_log(entry);
    }

}

fn authenticated_user_from_record(user: UserRecord) -> AuthenticatedUser {
    let profile = user.profile;
    let team_number = profile.public_team_number().to_string();
    AuthenticatedUser {
        trader_id: profile.trader_id,
        username: profile.username,
        team_number,
        role: profile.role,
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        AuthService::authenticate_request(parts, state)
            .map_err(|err| (err.status_code(), err.to_string()))
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthenticatedAdmin {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = admin_bearer_token(parts).ok_or_else(|| {
            (
                AuthError::MissingAdminAuthentication.status_code(),
                AuthError::MissingAdminAuthentication.to_string(),
            )
        })?;

        if token != state.config.admin_api_token {
            return Err((
                AuthError::InvalidAdminToken.status_code(),
                AuthError::InvalidAdminToken.to_string(),
            ));
        }

        Ok(AuthenticatedAdmin {
            username: "admin-token".to_string(),
        })
    }
}

fn admin_bearer_token<'a>(parts: &'a Parts) -> Option<&'a str> {
    parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use axum::http::Request;

    fn test_state() -> AppState {
        AppState::new(Config {
            bind_addr: "127.0.0.1:0".to_string(),
            checkpoint_path: None,
            checkpoint_interval_seconds: 5,
            ws_broadcast_buffer: 64,
            ws_market_delta_batch_interval_ms: 10,
            ws_market_broadcast_workers: 1,
            market_data_service_socket: None,
            market_data_service_retry_backoff_ms: 250,
            runtime_dispatch_queue_capacity: 4_096,
            account_dispatch_queue_capacity: 4_096,
            per_user_rate_limit_burst_capacity: 500,
            per_user_rate_limit_burst_window_seconds: 10,
            admin_api_token: "test-admin-token".to_string(),
        })
    }

    #[test]
    fn provision_returns_profile_with_api_key() {
        let state = test_state();
        let provisioned = AuthService::provision_user(
            &state,
            ProvisionUserRequest {
                username: "alice".to_string(),
                team_number: None,
                role: None,
            },
        )
        .expect("provision should succeed");
        assert_eq!(provisioned.profile.username, "alice");
        assert_eq!(provisioned.profile.api_key.len(), API_KEY_LENGTH);
        assert!(
            provisioned
                .profile
                .api_key
                .chars()
                .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit()),
            "api key should be 7-char alphanumeric, got {}",
            provisioned.profile.api_key
        );
    }

    #[tokio::test]
    async fn extracts_user_from_api_key_header() {
        let state = test_state();
        let provisioned = AuthService::provision_user(
            &state,
            ProvisionUserRequest {
                username: "bob".to_string(),
                team_number: None,
                role: None,
            },
        )
        .expect("provision should succeed");

        let request = Request::builder()
            .uri("/api/v1/balance")
            .header(API_KEY_HEADER, provisioned.profile.api_key.clone())
            .body(())
            .expect("request");
        let (mut parts, _) = request.into_parts();

        let auth = AuthenticatedUser::from_request_parts(&mut parts, &state)
            .await
            .expect("auth should parse");
        assert_eq!(auth.username, "bob");
    }

    #[test]
    fn authenticate_api_key_rejects_unknown_keys() {
        let state = test_state();

        let error = AuthService::authenticate_api_key(&state, "UNKNOWN")
            .expect_err("unknown api key should be rejected");

        assert_eq!(error, AuthError::InvalidApiKey);
    }

    #[tokio::test]
    async fn extracts_admin_from_bearer_token() {
        let state = test_state();
        let request = Request::builder()
            .uri("/api/v1/admin/users")
            .header(header::AUTHORIZATION, "Bearer test-admin-token")
            .body(())
            .expect("request");
        let (mut parts, _) = request.into_parts();

        let admin = AuthenticatedAdmin::from_request_parts(&mut parts, &state)
            .await
            .expect("admin auth should parse");
        assert_eq!(admin.username, "admin-token");
    }
}
