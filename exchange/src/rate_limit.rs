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
    buckets: Arc<DashMap<Uuid, Arc<Mutex<UserBucket>>>>,
}

#[derive(Debug)]
struct UserBucket {
    last_refill_at: Instant,
    tokens: f64,
}

#[derive(Debug, Serialize)]
struct RateLimitError {
    error: String,
}

impl PerUserRateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
        }
    }

    pub fn allow(&self, trader_id: Uuid, burst_capacity: u64, burst_window_seconds: u64) -> bool {
        self.allow_at(
            trader_id,
            burst_capacity,
            burst_window_seconds,
            Instant::now(),
        )
    }

    fn allow_at(
        &self,
        trader_id: Uuid,
        burst_capacity: u64,
        burst_window_seconds: u64,
        now: Instant,
    ) -> bool {
        if burst_capacity == 0 || burst_window_seconds == 0 {
            return false;
        }

        let bucket = self
            .buckets
            .entry(trader_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(UserBucket {
                    last_refill_at: now,
                    tokens: burst_capacity as f64,
                }))
            })
            .clone();

        let mut state = bucket.lock().expect("user rate limiter lock");
        let elapsed = now
            .checked_duration_since(state.last_refill_at)
            .unwrap_or_else(|| Duration::from_secs(0));
        let refill_rate = burst_capacity as f64 / burst_window_seconds as f64;
        state.tokens =
            (state.tokens + elapsed.as_secs_f64() * refill_rate).min(burst_capacity as f64);
        state.last_refill_at = now;

        if state.tokens < 1.0 {
            return false;
        }

        state.tokens -= 1.0;
        true
    }
}

pub fn enforce_authenticated_user_rate_limit(
    state: &AppState,
    trader_id: Uuid,
) -> Result<(), String> {
    if state.user_rate_limiter.allow(
        trader_id,
        state.config.per_user_rate_limit_burst_capacity,
        state.config.per_user_rate_limit_burst_window_seconds,
    ) {
        Ok(())
    } else {
        state.operator_telemetry().record_rate_limit_reject();
        Err(format!(
            "per-user rate limit exceeded: max {} ops per {}s",
            state.config.per_user_rate_limit_burst_capacity,
            state.config.per_user_rate_limit_burst_window_seconds,
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
    fn limiter_enforces_burst_capacity_before_refill() {
        let limiter = PerUserRateLimiter::new();
        let trader_id = Uuid::new_v4();
        let now = Instant::now();

        assert!(limiter.allow_at(trader_id, 2, 1, now));
        assert!(limiter.allow_at(trader_id, 2, 1, now));
        assert!(!limiter.allow_at(trader_id, 2, 1, now));
    }

    #[test]
    fn limiter_refills_tokens_over_time() {
        let limiter = PerUserRateLimiter::new();
        let trader_id = Uuid::new_v4();
        let start = Instant::now();

        assert!(limiter.allow_at(trader_id, 2, 2, start));
        assert!(limiter.allow_at(trader_id, 2, 2, start));
        assert!(!limiter.allow_at(trader_id, 2, 2, start));
        assert!(limiter.allow_at(trader_id, 2, 2, start + Duration::from_secs(1)));
        assert!(!limiter.allow_at(trader_id, 2, 2, start + Duration::from_secs(1)));
    }

    #[test]
    fn limiter_isolated_per_user() {
        let limiter = PerUserRateLimiter::new();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();

        assert!(limiter.allow(first, 1, 1));
        assert!(!limiter.allow(first, 1, 1));
        assert!(limiter.allow(second, 1, 1));
    }

    #[test]
    fn helper_uses_app_state_bucket_configuration() {
        let state = crate::state::AppState::new(crate::config::Config {
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
            per_user_rate_limit_burst_capacity: 1,
            per_user_rate_limit_burst_window_seconds: 1,
            admin_api_token: "test-admin-token".to_string(),
        });
        let trader_id = Uuid::new_v4();

        assert!(enforce_authenticated_user_rate_limit(&state, trader_id).is_ok());
        assert_eq!(
            enforce_authenticated_user_rate_limit(&state, trader_id),
            Err("per-user rate limit exceeded: max 1 ops per 1s".to_string())
        );
    }
}
