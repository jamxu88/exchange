use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{TimeZone, Utc};
use exchange::{
    auth::{AuthService, ProvisionUserRequest},
    build_app,
    config::Config,
    matching::MatchingEngine,
    orderbook::{Order, OrderBook, Side},
    state::{AppState, Balance},
};
use std::time::Instant;
use tower::ServiceExt;
use uuid::Uuid;

fn percentile_us(samples: &mut [u128], numerator: usize, denominator: usize) -> u128 {
    samples.sort_unstable();
    let idx = (samples.len() * numerator) / denominator;
    samples[idx.min(samples.len().saturating_sub(1))]
}

fn make_stable_order(id: u128, side: Side, price: u64, quantity: u64) -> Order {
    Order {
        id: Uuid::from_u128(id),
        trader_id: Uuid::from_u128(id + 1_000_000),
        market: "BTC-USD".to_string(),
        side,
        price,
        quantity,
        remaining: quantity,
        created_at: Utc.timestamp_opt(0, 0).single().expect("unix epoch"),
    }
}

#[tokio::test]
#[ignore = "Runtime-sensitive latency smoke test"]
async fn balance_endpoint_p95_latency_smoke() {
    let state = AppState::new(Config {
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
        per_user_rate_limit_burst_capacity: 1_000_000,
        per_user_rate_limit_burst_window_seconds: 1,
        admin_api_token: "test-admin-token".to_string(),
    });
    let provisioned = AuthService::provision_user(
        &state,
        ProvisionUserRequest {
            username: "latency-user".to_string(),
            team_number: None,
            role: None,
        },
    )
    .expect("provision user");
    let trader_id = provisioned.profile.trader_id;
    state.storage.put_balance(
        trader_id,
        Balance {
            asset: "USD".to_string(),
            free: 1_000_000,
            locked: 0,
        },
    );
    let app = build_app(state);

    let mut samples_us = Vec::with_capacity(500);
    for _ in 0..500 {
        let started = Instant::now();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/balance")
                    .header("x-api-key", provisioned.profile.api_key.clone())
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        samples_us.push(started.elapsed().as_micros());
    }

    let p95 = percentile_us(&mut samples_us, 95, 100);
    assert!(
        p95 < 25_000,
        "p95 too high for in-memory request path: {p95}us"
    );
}

#[test]
#[ignore = "Runtime-sensitive latency smoke test"]
fn matching_engine_bulk_latency_smoke() {
    let runs = 2_000;
    let mut total_fill_count = 0usize;
    let mut template = OrderBook::default();
    for i in 0..200_u64 {
        template.add_order(make_stable_order(i as u128 + 1, Side::Sell, 100 + i, 1));
    }
    let incoming = make_stable_order(10_000, Side::Buy, 10_000, 200);
    let started = Instant::now();

    for _ in 0..runs {
        let mut book = template.clone();
        let executions =
            MatchingEngine::process_limit_order_executions(&mut book, incoming.clone());
        total_fill_count += executions.len();
    }

    let elapsed_ms = started.elapsed().as_millis();
    assert_eq!(total_fill_count, runs * 200);
    assert!(
        elapsed_ms < 2_000,
        "matching runtime too slow in debug build: {elapsed_ms}ms"
    );
}
