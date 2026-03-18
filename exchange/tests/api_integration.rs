use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use exchange::{
    accounts::UserProfile,
    auth::{AuthService, ProvisionUserRequest, ProvisionUserResponse},
    build_app,
    config::Config,
    orderbook::{Fill, Order, Side},
    rest::HealthResponse,
    settlement::SettlementEngine,
    state::{AppState, Balance, PortfolioSnapshot},
    trading::{
        AmendOrderRequest, AmendOrderResponse, CancelOrderResponse, SubmitOrderRequest,
        SubmitOrderResponse,
    },
};
use tower::ServiceExt;

fn test_state() -> AppState {
    AppState::new(Config {
        bind_addr: "127.0.0.1:0".to_string(),
        database_url: "postgres://test".to_string(),
        storage_backend: exchange::storage::StorageBackendKind::InMemory,
        ws_broadcast_buffer: 64,
        per_user_requests_per_second: 100,
        admin_api_token: "test-admin-token".to_string(),
        postgres_write_batch_size: 128,
        postgres_write_flush_interval_ms: 25,
        postgres_write_queue_capacity: 4_096,
        postgres_write_retry_backoff_ms: 250,
    })
}

fn rate_limited_state(per_user_requests_per_second: u64) -> AppState {
    AppState::new(Config {
        bind_addr: "127.0.0.1:0".to_string(),
        database_url: "postgres://test".to_string(),
        storage_backend: exchange::storage::StorageBackendKind::InMemory,
        ws_broadcast_buffer: 64,
        per_user_requests_per_second,
        admin_api_token: "test-admin-token".to_string(),
        postgres_write_batch_size: 128,
        postgres_write_flush_interval_ms: 25,
        postgres_write_queue_capacity: 4_096,
        postgres_write_retry_backoff_ms: 250,
    })
}

fn api_key_request(method: Method, uri: &str, api_key: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-api-key", api_key)
        .body(Body::empty())
        .expect("request")
}

fn api_key_json_request<T: serde::Serialize>(
    method: Method,
    uri: impl AsRef<str>,
    api_key: &str,
    body: &T,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri.as_ref())
        .header("x-api-key", api_key)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).expect("json body")))
        .expect("request")
}

fn admin_request(method: Method, uri: &str, admin_token: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {admin_token}"))
        .body(body)
        .expect("request")
}

async fn json_body<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&body).expect("json body")
}

fn provision_user(state: &AppState, username: &str) -> ProvisionUserResponse {
    AuthService::provision_user(
        state,
        ProvisionUserRequest {
            username: username.to_string(),
        },
    )
    .expect("provision user")
}

#[tokio::test]
async fn health_endpoint_returns_ok_payload() {
    let app = build_app(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: HealthResponse = json_body(response).await;
    assert_eq!(payload.status, "ok");
    assert_eq!(payload.service, "exchange");
    assert_eq!(
        payload.persistence.backend,
        exchange::storage::StorageBackendKind::InMemory
    );
    assert_eq!(
        payload.persistence.mode,
        exchange::storage::PersistenceMode::Disabled
    );
}

#[tokio::test]
async fn balance_endpoint_requires_authentication() {
    let app = build_app(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/balance")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_login_endpoint_is_not_exposed() {
    let app = build_app(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn register_endpoint_is_not_exposed() {
    let app = build_app(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_can_provision_competition_user() {
    let state = test_state();
    let app = build_app(state.clone());
    let response = app
        .oneshot(admin_request(
            Method::POST,
            "/api/v1/admin/users",
            "test-admin-token",
            Body::from(
                serde_json::to_vec(&ProvisionUserRequest {
                    username: "comp-user".to_string(),
                })
                .expect("provision json"),
            ),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::CREATED);
    let provisioned: ProvisionUserResponse = json_body(response).await;
    assert_eq!(provisioned.profile.username, "comp-user");
    assert!(provisioned.profile.api_key.starts_with("exch_"));
    assert!(state.storage.get_user_by_username("comp-user").is_some());
    assert_eq!(state.storage.list_admin_audit_logs().len(), 1);
}

#[tokio::test]
async fn admin_provision_requires_valid_admin_token() {
    let app = build_app(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/admin/users")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ProvisionUserRequest {
                        username: "comp-user".to_string(),
                    })
                    .expect("provision json"),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_provision_rejects_invalid_admin_token() {
    let app = build_app(test_state());
    let response = app
        .oneshot(admin_request(
            Method::POST,
            "/api/v1/admin/users",
            "wrong-admin-token",
            Body::from(
                serde_json::to_vec(&ProvisionUserRequest {
                    username: "comp-user".to_string(),
                })
                .expect("provision json"),
            ),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_provision_rejects_duplicate_username() {
    let state = test_state();
    let app = build_app(state.clone());
    provision_user(&state, "duplicate-user");

    let response = app
        .oneshot(admin_request(
            Method::POST,
            "/api/v1/admin/users",
            "test-admin-token",
            Body::from(
                serde_json::to_vec(&ProvisionUserRequest {
                    username: "duplicate-user".to_string(),
                })
                .expect("provision json"),
            ),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn provisioned_api_key_can_read_profile() {
    let state = test_state();
    let app = build_app(state.clone());
    let registered = provision_user(&state, "alice");

    let profile_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/user",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(profile_response.status(), StatusCode::OK);
    let profile: UserProfile = json_body(profile_response).await;
    assert_eq!(profile.username, "alice");
    assert_eq!(profile.api_key, registered.profile.api_key);
}

#[tokio::test]
async fn api_key_request_can_access_balance() {
    let state = test_state();
    let app = build_app(state.clone());
    let registered = provision_user(&state, "maker");
    state.storage.put_balance(
        registered.profile.trader_id,
        Balance {
            asset: "USD".to_string(),
            free: 250_000,
            locked: 10_000,
        },
    );

    let response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/balance",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let balances: Vec<Balance> = json_body(response).await;
    assert_eq!(balances.len(), 1);
    assert_eq!(balances[0].asset, "USD");
}

#[tokio::test]
async fn balance_and_portfolio_endpoints_return_trader_state() {
    let state = test_state();
    let app = build_app(state.clone());
    let registered = provision_user(&state, "portfolio-user");
    state.storage.put_balance(
        registered.profile.trader_id,
        Balance {
            asset: "USD".to_string(),
            free: 250_000,
            locked: 10_000,
        },
    );

    let balance_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/balance",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(balance_response.status(), StatusCode::OK);
    let balances: Vec<Balance> = json_body(balance_response).await;
    assert_eq!(balances.len(), 1);
    assert_eq!(balances[0].asset, "USD");
    assert_eq!(balances[0].free, 250_000);

    let portfolio_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/portfolio",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(portfolio_response.status(), StatusCode::OK);
    let portfolio: PortfolioSnapshot = json_body(portfolio_response).await;
    assert_eq!(portfolio.trader_id, registered.profile.trader_id);
    assert_eq!(portfolio.balances.len(), 1);
}

#[tokio::test]
async fn submit_amend_cancel_order_flow_updates_open_orders_and_balances() {
    let state = test_state();
    let app = build_app(state.clone());
    let registered = provision_user(&state, "trader-a");
    SettlementEngine::seed_balance(&state, registered.profile.trader_id, "USD", 1_000);

    let submit_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &registered.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 5,
            },
        ))
        .await
        .expect("response");
    assert_eq!(submit_response.status(), StatusCode::CREATED);
    let submitted: SubmitOrderResponse = json_body(submit_response).await;
    assert!(submitted.resting);
    assert_eq!(submitted.order.remaining, 5);

    let amend_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::PATCH,
            format!("/api/v1/orders/{}", submitted.order.id),
            &registered.profile.api_key,
            &AmendOrderRequest { remaining: 2 },
        ))
        .await
        .expect("response");
    assert_eq!(amend_response.status(), StatusCode::OK);
    let amended: AmendOrderResponse = json_body(amend_response).await;
    assert_eq!(amended.order.remaining, 2);

    let open_orders_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/open-orders",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(open_orders_response.status(), StatusCode::OK);
    let open_orders: Vec<Order> = json_body(open_orders_response).await;
    assert_eq!(open_orders.len(), 1);
    assert_eq!(open_orders[0].remaining, 2);

    let cancel_response = app
        .clone()
        .oneshot(api_key_request(
            Method::DELETE,
            &format!("/api/v1/orders/{}", submitted.order.id),
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(cancel_response.status(), StatusCode::OK);
    let canceled: CancelOrderResponse = json_body(cancel_response).await;
    assert_eq!(canceled.order.id, submitted.order.id);

    let balances_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/balance",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    let balances: Vec<Balance> = json_body(balances_response).await;
    let usd = balances
        .iter()
        .find(|balance| balance.asset == "USD")
        .expect("usd balance");
    assert_eq!(usd.free, 1_000);
    assert_eq!(usd.locked, 0);

    let open_orders_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/open-orders",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    let open_orders: Vec<Order> = json_body(open_orders_response).await;
    assert!(open_orders.is_empty());
}

#[tokio::test]
async fn matching_order_flow_updates_fills_balances_and_open_orders() {
    let state = test_state();
    let app = build_app(state.clone());
    let maker = provision_user(&state, "maker-user");
    let taker = provision_user(&state, "taker-user");
    SettlementEngine::seed_balance(&state, maker.profile.trader_id, "BTC", 5);
    SettlementEngine::seed_balance(&state, taker.profile.trader_id, "USD", 1_000);

    let maker_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &maker.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                price: 100,
                quantity: 2,
            },
        ))
        .await
        .expect("response");
    assert_eq!(maker_response.status(), StatusCode::CREATED);

    let taker_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &taker.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 105,
                quantity: 2,
            },
        ))
        .await
        .expect("response");
    assert_eq!(taker_response.status(), StatusCode::CREATED);
    let taker_submit: SubmitOrderResponse = json_body(taker_response).await;
    assert_eq!(taker_submit.fills.len(), 1);
    assert!(!taker_submit.resting);

    let maker_fills_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/fills",
            &maker.profile.api_key,
        ))
        .await
        .expect("response");
    let maker_fills: Vec<Fill> = json_body(maker_fills_response).await;
    assert_eq!(maker_fills.len(), 1);
    assert_eq!(maker_fills[0].price, 100);

    let taker_fills_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/fills?market=BTC-USD",
            &taker.profile.api_key,
        ))
        .await
        .expect("response");
    let taker_fills: Vec<Fill> = json_body(taker_fills_response).await;
    assert_eq!(taker_fills.len(), 1);
    assert_eq!(taker_fills[0].quantity, 2);

    let maker_balances_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/balance",
            &maker.profile.api_key,
        ))
        .await
        .expect("response");
    let maker_balances: Vec<Balance> = json_body(maker_balances_response).await;
    assert_eq!(
        maker_balances
            .iter()
            .find(|balance| balance.asset == "BTC")
            .expect("maker btc"),
        &Balance {
            asset: "BTC".to_string(),
            free: 3,
            locked: 0,
        }
    );
    assert_eq!(
        maker_balances
            .iter()
            .find(|balance| balance.asset == "USD")
            .expect("maker usd"),
        &Balance {
            asset: "USD".to_string(),
            free: 200,
            locked: 0,
        }
    );

    let taker_balances_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/balance",
            &taker.profile.api_key,
        ))
        .await
        .expect("response");
    let taker_balances: Vec<Balance> = json_body(taker_balances_response).await;
    assert_eq!(
        taker_balances
            .iter()
            .find(|balance| balance.asset == "USD")
            .expect("taker usd"),
        &Balance {
            asset: "USD".to_string(),
            free: 800,
            locked: 0,
        }
    );
    assert_eq!(
        taker_balances
            .iter()
            .find(|balance| balance.asset == "BTC")
            .expect("taker btc"),
        &Balance {
            asset: "BTC".to_string(),
            free: 2,
            locked: 0,
        }
    );

    let maker_open_orders_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/open-orders",
            &maker.profile.api_key,
        ))
        .await
        .expect("response");
    let maker_open_orders: Vec<Order> = json_body(maker_open_orders_response).await;
    assert!(maker_open_orders.is_empty());

    let taker_open_orders_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/open-orders",
            &taker.profile.api_key,
        ))
        .await
        .expect("response");
    let taker_open_orders: Vec<Order> = json_body(taker_open_orders_response).await;
    assert!(taker_open_orders.is_empty());
}

#[tokio::test]
async fn api_key_order_flow_supports_submit_and_account_queries() {
    let state = test_state();
    let app = build_app(state.clone());
    let trader = provision_user(&state, "api-trader");
    SettlementEngine::seed_balance(&state, trader.profile.trader_id, "USD", 2_000);

    let submit_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &trader.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 4,
            },
        ))
        .await
        .expect("response");
    assert_eq!(submit_response.status(), StatusCode::CREATED);
    let submitted: SubmitOrderResponse = json_body(submit_response).await;
    assert!(submitted.resting);

    let open_orders_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/open-orders?market=BTC-USD",
            &trader.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(open_orders_response.status(), StatusCode::OK);
    let open_orders: Vec<Order> = json_body(open_orders_response).await;
    assert_eq!(open_orders.len(), 1);
    assert_eq!(open_orders[0].id, submitted.order.id);

    let balance_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/balance",
            &trader.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(balance_response.status(), StatusCode::OK);
    let balances: Vec<Balance> = json_body(balance_response).await;
    assert_eq!(
        balances.iter().find(|balance| balance.asset == "USD"),
        Some(&Balance {
            asset: "USD".to_string(),
            free: 1_600,
            locked: 400,
        })
    );
}

#[tokio::test]
async fn trader_cannot_amend_or_cancel_another_traders_order() {
    let state = test_state();
    let app = build_app(state.clone());
    let owner = provision_user(&state, "owner-user");
    let intruder = provision_user(&state, "intruder-user");
    SettlementEngine::seed_balance(&state, owner.profile.trader_id, "USD", 1_000);
    SettlementEngine::seed_balance(&state, intruder.profile.trader_id, "USD", 1_000);

    let submit_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &owner.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 2,
            },
        ))
        .await
        .expect("response");
    assert_eq!(submit_response.status(), StatusCode::CREATED);
    let submitted: SubmitOrderResponse = json_body(submit_response).await;

    let amend_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::PATCH,
            format!("/api/v1/orders/{}", submitted.order.id),
            &intruder.profile.api_key,
            &AmendOrderRequest { remaining: 1 },
        ))
        .await
        .expect("response");
    assert_eq!(amend_response.status(), StatusCode::NOT_FOUND);

    let cancel_response = app
        .clone()
        .oneshot(api_key_request(
            Method::DELETE,
            &format!("/api/v1/orders/{}", submitted.order.id),
            &intruder.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(cancel_response.status(), StatusCode::NOT_FOUND);

    let owner_open_orders_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/open-orders",
            &owner.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(owner_open_orders_response.status(), StatusCode::OK);
    let owner_open_orders: Vec<Order> = json_body(owner_open_orders_response).await;
    assert_eq!(owner_open_orders.len(), 1);
    assert_eq!(owner_open_orders[0].id, submitted.order.id);
}

#[tokio::test]
async fn per_user_rate_limit_is_enforced_on_authenticated_routes() {
    let state = rate_limited_state(2);
    let app = build_app(state.clone());
    let first = provision_user(&state, "rate-user-a");
    let second = provision_user(&state, "rate-user-b");
    SettlementEngine::seed_balance(&state, first.profile.trader_id, "USD", 100);
    SettlementEngine::seed_balance(&state, second.profile.trader_id, "USD", 100);

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(api_key_request(
                Method::GET,
                "/api/v1/balance",
                &first.profile.api_key,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let limited = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/balance",
            &first.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);

    let second_user = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/balance",
            &second.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(second_user.status(), StatusCode::OK);
}

#[tokio::test]
async fn openapi_document_is_served() {
    let app = build_app(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api-doc/openapi.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}
