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

    if !state
        .user_rate_limiter
        .allow(auth.trader_id, state.config.per_user_requests_per_second)
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(RateLimitError {
                error: format!(
                    "per-user rate limit exceeded: max {} ops/sec",
                    state.config.per_user_requests_per_second
                ),
            }),
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
}
