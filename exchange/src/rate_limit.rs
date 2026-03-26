use crate::auth::{AuthService, AuthenticatedUser};
use crate::state::AppState;
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone)]
pub struct PerUserRateLimiter {
    windows: Arc<DashMap<Uuid, Arc<Mutex<UserWindow>>>>,
}

#[derive(Debug)]
struct UserWindow {
    started_at: Instant,
    count: u64,
}

#[derive(Debug, Serialize)]
struct RateLimitError {
    error: String,
}

impl PerUserRateLimiter {
    pub fn new() -> Self {
        Self {
            windows: Arc::new(DashMap::new()),
        }
    }

    pub fn allow(&self, trader_id: Uuid, max_per_second: u64) -> bool {
        let window = self
            .windows
            .entry(trader_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(UserWindow {
                    started_at: Instant::now(),
                    count: 0,
                }))
            })
            .clone();

        let mut state = window.lock().expect("user rate limiter lock");
        if state.started_at.elapsed() >= Duration::from_secs(1) {
            state.started_at = Instant::now();
            state.count = 0;
        }

        if state.count >= max_per_second {
            return false;
        }

        state.count += 1;
        true
    }
}

pub fn enforce_authenticated_user_rate_limit(
    state: &AppState,
    trader_id: Uuid,
) -> Result<(), String> {
    if state
        .user_rate_limiter
        .allow(trader_id, state.config.per_user_requests_per_second)
    {
        Ok(())
    } else {
        Err(format!(
            "per-user rate limit exceeded: max {} ops/sec",
            state.config.per_user_requests_per_second
        ))
    }
}

pub async fn authenticated_user_rate_limit(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = request.into_parts();
    let auth = match AuthService::authenticate_request(&parts, &state) {
        Ok(auth) => auth,
        Err(err) => {
            return (
                err.status_code(),
                Json(RateLimitError {
                    error: err.to_string(),
                }),
            )
                .into_response();
        }
    };

    if let Err(error) = enforce_authenticated_user_rate_limit(&state, auth.trader_id) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(RateLimitError { error }),
        )
            .into_response();
    }

    parts.extensions.insert::<AuthenticatedUser>(auth);
    next.run(Request::from_parts(parts, body)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_enforces_max_requests_per_second() {
        let limiter = PerUserRateLimiter::new();
        let trader_id = Uuid::new_v4();
        assert!(limiter.allow(trader_id, 2));
        assert!(limiter.allow(trader_id, 2));
        assert!(!limiter.allow(trader_id, 2));
    }

    #[test]
    fn limiter_isolated_per_user() {
        let limiter = PerUserRateLimiter::new();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();

        assert!(limiter.allow(first, 1));
        assert!(!limiter.allow(first, 1));
        assert!(limiter.allow(second, 1));
    }

    #[test]
    fn helper_uses_app_state_limit_configuration() {
        let state = crate::state::AppState::new(crate::config::Config {
            bind_addr: "127.0.0.1:0".to_string(),
            database_url: "postgres://test".to_string(),
            storage_backend: crate::storage::StorageBackendKind::InMemory,
            ws_broadcast_buffer: 64,
            ws_market_delta_batch_interval_ms: 10,
            ws_market_broadcast_workers: 1,
            market_data_service_socket: None,
            market_data_service_retry_backoff_ms: 250,
            runtime_dispatch_queue_capacity: 4_096,
            account_dispatch_queue_capacity: 4_096,
            persistence_dispatch_queue_capacity: 4_096,
            per_user_requests_per_second: 1,
            admin_api_token: "test-admin-token".to_string(),
            postgres_write_batch_size: 128,
            postgres_write_flush_interval_ms: 25,
            postgres_write_queue_capacity: 4_096,
            postgres_write_retry_backoff_ms: 250,
        });
        let trader_id = Uuid::new_v4();

        assert!(enforce_authenticated_user_rate_limit(&state, trader_id).is_ok());
        assert_eq!(
            enforce_authenticated_user_rate_limit(&state, trader_id),
            Err("per-user rate limit exceeded: max 1 ops/sec".to_string())
        );
    }
}
