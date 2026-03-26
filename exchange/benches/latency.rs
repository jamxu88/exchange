use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{TimeZone, Utc};
use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use exchange::{
    auth::{AuthService, ProvisionUserRequest},
    build_app,
    config::Config,
    matching::MatchingEngine,
    orderbook::{Order, OrderBook, Side},
    state::{AppState, Balance},
};
use tower::ServiceExt;
use uuid::Uuid;

fn make_order(side: Side, price: u64, quantity: u64) -> Order {
    Order {
        id: Uuid::new_v4(),
        trader_id: Uuid::new_v4(),
        market: "BTC-USD".to_string(),
        side,
        price,
        quantity,
        remaining: quantity,
        created_at: Utc::now(),
    }
}

fn make_bench_order(id: u128, side: Side, price: u64, quantity: u64) -> Order {
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

fn matching_engine_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine");

    for levels in [10_u64, 100_u64, 1_000_u64] {
        group.throughput(Throughput::Elements(levels));
        group.bench_with_input(
            BenchmarkId::new("limit_order_cross", levels),
            &levels,
            |b, &levels| {
                b.iter(|| {
                    let mut book = OrderBook::default();
                    for i in 0..levels {
                        book.add_order(make_order(Side::Sell, 100 + i, 1));
                    }
                    let incoming = make_order(Side::Buy, 100 + levels, levels);
                    let fills = MatchingEngine::process_limit_order(&mut book, incoming);
                    black_box(fills.len());
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("limit_order_cross_prebuilt", levels),
            &levels,
            |b, &levels| {
                let mut template = OrderBook::default();
                for i in 0..levels {
                    template.add_order(make_bench_order(i as u128 + 1, Side::Sell, 100 + i, 1));
                }
                let incoming =
                    make_bench_order(levels as u128 + 10_000, Side::Buy, 100 + levels, levels);

                b.iter_batched(
                    || template.clone(),
                    |mut book| {
                        let executions = MatchingEngine::process_limit_order_executions(
                            &mut book,
                            incoming.clone(),
                        );
                        black_box(executions.len());
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn rest_router_latency(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build runtime");

    let state = AppState::new(Config {
        bind_addr: "127.0.0.1:0".to_string(),
        database_url: "postgres://bench".to_string(),
        storage_backend: exchange::storage::StorageBackendKind::InMemory,
        ws_broadcast_buffer: 256,
        ws_market_delta_batch_interval_ms: 10,
        ws_market_broadcast_workers: 1,
        per_user_rate_limit_burst_capacity: u64::MAX,
        per_user_rate_limit_burst_window_seconds: 1,
        admin_api_token: "test-admin-token".to_string(),
        ..Config::from_env()
    });
    let provisioned = AuthService::provision_user(
        &state,
        ProvisionUserRequest {
            username: "bench-user".to_string(),
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

    let mut group = c.benchmark_group("rest_router");
    group.bench_function("health", |b| {
        b.to_async(&rt).iter(|| async {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/health")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            black_box(response.status());
        });
    });

    group.bench_function("balance_authenticated", |b| {
        b.to_async(&rt).iter(|| async {
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
        });
    });

    group.finish();
}

criterion_group!(
    latency_benches,
    matching_engine_latency,
    rest_router_latency
);
criterion_main!(latency_benches);
