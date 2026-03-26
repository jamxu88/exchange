use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use chrono::Utc;
use exchange::{
    accounts::{UserProfile, UserRole},
    admin::{
        AdminMessageEntry, AdminMessageLevel, AdminStateResponse, CompetitionLeaderboardSnapshot,
        CompetitionSettlementRequest, DeleteMarketResponse, FinalizeCompetitionRequest,
        FinalizeCompetitionResponse, LeaderboardRow, LoadExchangeConfigResponse, MarketDefinition,
        MarketStatus, ProvisionedUsersResponse, ResetUsersResponse, SendAdminMessageRequest,
        SettleMarketRequest, SettleMarketResponse, TradingControlResponse, UpdateMarketRequest,
    },
    auth::{AuthService, ProvisionUserRequest, ProvisionUserResponse},
    build_app,
    config::Config,
    orderbook::{Fill, Order, Side},
    rest::HealthResponse,
    settlement::SettlementEngine,
    state::{AppState, NET_POSITION_LIMIT, PortfolioSnapshot, Position},
    trading::{
        AmendOrderRequest, AmendOrderResponse, CancelOrderResponse, OrderType, SubmitOrderRequest,
        SubmitOrderResponse,
    },
};
use tower::ServiceExt;

fn test_state() -> AppState {
    let state = AppState::new(Config {
        bind_addr: "127.0.0.1:0".to_string(),
        database_url: "postgres://test".to_string(),
        storage_backend: exchange::storage::StorageBackendKind::InMemory,
        ws_broadcast_buffer: 64,
        runtime_dispatch_queue_capacity: 4_096,
        account_dispatch_queue_capacity: 4_096,
        persistence_dispatch_queue_capacity: 4_096,
        per_user_requests_per_second: 100,
        admin_api_token: "test-admin-token".to_string(),
        postgres_write_batch_size: 128,
        postgres_write_flush_interval_ms: 25,
        postgres_write_queue_capacity: 4_096,
        postgres_write_retry_backoff_ms: 250,
    });
    seed_market(&state, "BTC-USD", "BTC", "USD");
    seed_market(&state, "ETH-USD", "ETH", "USD");
    state
}

fn rate_limited_state(per_user_requests_per_second: u64) -> AppState {
    let state = AppState::new(Config {
        bind_addr: "127.0.0.1:0".to_string(),
        database_url: "postgres://test".to_string(),
        storage_backend: exchange::storage::StorageBackendKind::InMemory,
        ws_broadcast_buffer: 64,
        runtime_dispatch_queue_capacity: 4_096,
        account_dispatch_queue_capacity: 4_096,
        persistence_dispatch_queue_capacity: 4_096,
        per_user_requests_per_second,
        admin_api_token: "test-admin-token".to_string(),
        postgres_write_batch_size: 128,
        postgres_write_flush_interval_ms: 25,
        postgres_write_queue_capacity: 4_096,
        postgres_write_retry_backoff_ms: 250,
    });
    seed_market(&state, "BTC-USD", "BTC", "USD");
    seed_market(&state, "ETH-USD", "ETH", "USD");
    state
}

fn seed_market(state: &AppState, market_id: &str, base_asset: &str, quote_asset: &str) {
    let now = Utc::now();
    state.storage.upsert_market(MarketDefinition {
        market_id: market_id.to_string(),
        display_name: market_id.to_string(),
        base_asset: base_asset.to_string(),
        quote_asset: quote_asset.to_string(),
        tick_size: 1,
        min_order_quantity: 1,
        reference_price: None,
        settlement_price: None,
        status: MarketStatus::Enabled,
        created_at: now,
        updated_at: now,
    });
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

fn admin_json_request<T: serde::Serialize>(
    method: Method,
    uri: &str,
    admin_token: &str,
    body: &T,
) -> Request<Body> {
    admin_request(
        method,
        uri,
        admin_token,
        Body::from(serde_json::to_vec(body).expect("admin json")),
    )
}

async fn json_body<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&body).expect("json body")
}

async fn text_body(response: axum::response::Response) -> String {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    String::from_utf8(body.to_vec()).expect("utf8 body")
}

fn provision_user(state: &AppState, username: &str) -> ProvisionUserResponse {
    AuthService::provision_user(
        state,
        ProvisionUserRequest {
            username: username.to_string(),
            role: None,
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
                    role: None,
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
async fn admin_can_list_provisioned_users_with_filters() {
    let state = test_state();
    let app = build_app(state.clone());
    provision_user(&state, "batch-a-001");
    provision_user(&state, "batch-a-002");
    AuthService::provision_user(
        &state,
        ProvisionUserRequest {
            username: "desk-admin".to_string(),
            role: Some(UserRole::Admin),
        },
    )
    .expect("admin user");

    let response = app
        .oneshot(admin_request(
            Method::GET,
            "/api/v1/admin/users?username_prefix=batch-a-&role=trader",
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: ProvisionedUsersResponse = json_body(response).await;
    assert_eq!(payload.users.len(), 2);
    assert_eq!(payload.users[0].username, "batch-a-001");
    assert_eq!(payload.users[1].username, "batch-a-002");
    assert!(
        payload
            .users
            .iter()
            .all(|user| user.role == UserRole::Trader)
    );
    assert!(
        payload
            .users
            .iter()
            .all(|user| user.position_limit == Some(NET_POSITION_LIMIT))
    );
}

#[tokio::test]
async fn admin_can_export_provisioned_users_as_csv() {
    let state = test_state();
    let app = build_app(state.clone());
    let alpha = provision_user(&state, "export-a");
    let bravo = provision_user(&state, "export-b");
    AuthService::provision_user(
        &state,
        ProvisionUserRequest {
            username: "desk-admin".to_string(),
            role: Some(UserRole::Admin),
        },
    )
    .expect("admin user");

    let response = app
        .oneshot(admin_request(
            Method::GET,
            "/api/v1/admin/users/export.csv?username_prefix=export-&role=trader",
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/csv; charset=utf-8")
    );
    let csv = text_body(response).await;
    assert!(csv.starts_with("trader_id,username,api_key,role,position_limit,created_at\n"));
    assert!(csv.contains(&format!(
        "{},export-a,{},trader,{},",
        alpha.profile.trader_id, alpha.profile.api_key, NET_POSITION_LIMIT
    )));
    assert!(csv.contains(&format!(
        "{},export-b,{},trader,{},",
        bravo.profile.trader_id, bravo.profile.api_key, NET_POSITION_LIMIT
    )));
    assert!(!csv.contains("desk-admin"));
}

#[tokio::test]
async fn admin_role_trader_has_unlimited_position_power() {
    let state = test_state();
    let app = build_app(state.clone());
    let response = app
        .clone()
        .oneshot(admin_request(
            Method::POST,
            "/api/v1/admin/users",
            "test-admin-token",
            Body::from(
                serde_json::to_vec(&ProvisionUserRequest {
                    username: "desk-admin".to_string(),
                    role: Some(UserRole::Admin),
                })
                .expect("provision json"),
            ),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::CREATED);
    let provisioned: ProvisionUserResponse = json_body(response).await;
    assert_eq!(provisioned.profile.role, UserRole::Admin);

    let oversized_order = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &provisioned.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: (NET_POSITION_LIMIT + 500) as u64,
            },
        ))
        .await
        .expect("response");
    assert_eq!(oversized_order.status(), StatusCode::CREATED);
    let _: SubmitOrderResponse = json_body(oversized_order).await;

    let portfolio_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/portfolio",
            &provisioned.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(portfolio_response.status(), StatusCode::OK);
    let portfolio: PortfolioSnapshot = json_body(portfolio_response).await;
    assert_eq!(portfolio.position_limit, None);
    assert_eq!(portfolio.positions.len(), 0);
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
                        role: None,
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
                    role: None,
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
                    role: None,
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
async fn api_key_request_can_access_positions() {
    let state = test_state();
    let app = build_app(state.clone());
    let registered = provision_user(&state, "maker");
    SettlementEngine::seed_position(
        &state,
        registered.profile.trader_id,
        "BTC-USD",
        5,
        Some(100),
        10,
    );

    let response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/positions",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let positions: Vec<Position> = json_body(response).await;
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].market, "BTC-USD");
}

#[tokio::test]
async fn positions_and_portfolio_endpoints_return_trader_state() {
    let state = test_state();
    let app = build_app(state.clone());
    let registered = provision_user(&state, "portfolio-user");
    SettlementEngine::seed_position(
        &state,
        registered.profile.trader_id,
        "BTC-USD",
        -3,
        Some(110),
        25,
    );

    let positions_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/positions",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(positions_response.status(), StatusCode::OK);
    let positions: Vec<Position> = json_body(positions_response).await;
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].market, "BTC-USD");
    assert_eq!(positions[0].net_quantity, -3);

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
    assert_eq!(portfolio.position_limit, Some(NET_POSITION_LIMIT));
    assert_eq!(portfolio.positions.len(), 1);
}

#[tokio::test]
async fn submit_amend_cancel_order_flow_updates_open_orders_and_positions() {
    let state = test_state();
    let app = build_app(state.clone());
    let registered = provision_user(&state, "trader-a");

    let submit_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &registered.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
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

    let positions_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/positions",
            &registered.profile.api_key,
        ))
        .await
        .expect("response");
    let positions: Vec<Position> = json_body(positions_response).await;
    assert!(positions.is_empty());

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
async fn matching_order_flow_allows_short_seller_and_updates_positions_and_open_orders() {
    let state = test_state();
    let app = build_app(state.clone());
    let maker = provision_user(&state, "maker-user");
    let taker = provision_user(&state, "taker-user");

    let maker_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &maker.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
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
                order_type: OrderType::Limit,
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

    let maker_positions_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/positions",
            &maker.profile.api_key,
        ))
        .await
        .expect("response");
    let maker_positions: Vec<Position> = json_body(maker_positions_response).await;
    assert_eq!(
        maker_positions
            .iter()
            .find(|position| position.market == "BTC-USD")
            .expect("maker position")
            .net_quantity,
        -2
    );
    assert_eq!(
        maker_positions
            .iter()
            .find(|position| position.market == "BTC-USD")
            .expect("maker position")
            .average_entry_price,
        Some(100)
    );

    let taker_positions_response = app
        .clone()
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/positions",
            &taker.profile.api_key,
        ))
        .await
        .expect("response");
    let taker_positions: Vec<Position> = json_body(taker_positions_response).await;
    assert_eq!(
        taker_positions
            .iter()
            .find(|position| position.market == "BTC-USD")
            .expect("taker position")
            .net_quantity,
        2
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

    let submit_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &trader.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
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

    let positions_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/positions",
            &trader.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(positions_response.status(), StatusCode::OK);
    let positions: Vec<Position> = json_body(positions_response).await;
    assert!(positions.is_empty());
}

#[tokio::test]
async fn oversized_order_values_are_rejected_before_persistence() {
    let state = test_state();
    let app = build_app(state.clone());
    let trader = provision_user(&state, "oversized-order-user");

    let huge_price_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &trader.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: (i64::MAX as u64).saturating_add(1),
                quantity: 1,
            },
        ))
        .await
        .expect("response");
    assert_eq!(huge_price_response.status(), StatusCode::BAD_REQUEST);
    let huge_price_error: exchange::rest::ApiError = json_body(huge_price_response).await;
    assert_eq!(
        huge_price_error.error,
        format!("price must be at most {}", i64::MAX)
    );

    let huge_quantity_response = app
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &trader.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: (i64::MAX as u64).saturating_add(1),
            },
        ))
        .await
        .expect("response");
    assert_eq!(huge_quantity_response.status(), StatusCode::BAD_REQUEST);
    let huge_quantity_error: exchange::rest::ApiError = json_body(huge_quantity_response).await;
    assert_eq!(
        huge_quantity_error.error,
        format!("quantity must be at most {}", i64::MAX)
    );

    assert!(
        state
            .storage
            .list_open_orders(trader.profile.trader_id, None)
            .is_empty()
    );
}

#[tokio::test]
async fn trader_cannot_amend_or_cancel_another_traders_order() {
    let state = test_state();
    let app = build_app(state.clone());
    let owner = provision_user(&state, "owner-user");
    let intruder = provision_user(&state, "intruder-user");

    let submit_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &owner.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
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

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(api_key_request(
                Method::GET,
                "/api/v1/positions",
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
            "/api/v1/positions",
            &first.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);

    let second_user = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/positions",
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

#[tokio::test]
async fn public_markets_endpoint_returns_configured_markets() {
    let app = build_app(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/markets")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let markets: Vec<MarketDefinition> = json_body(response).await;
    assert!(markets.iter().any(|market| market.market_id == "BTC-USD"));
    assert!(markets.iter().any(|market| market.market_id == "ETH-USD"));
}

#[tokio::test]
async fn admin_can_stop_and_start_trading() {
    let state = test_state();
    let app = build_app(state.clone());
    let trader = provision_user(&state, "trade-toggle-user");

    let stop_response = app
        .clone()
        .oneshot(admin_request(
            Method::POST,
            "/api/v1/admin/trading/stop",
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");
    assert_eq!(stop_response.status(), StatusCode::OK);
    let stopped: TradingControlResponse = json_body(stop_response).await;
    assert!(!stopped.controls.trading_enabled);

    let rejected = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &trader.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 1,
            },
        ))
        .await
        .expect("response");
    assert_eq!(rejected.status(), StatusCode::CONFLICT);

    let start_response = app
        .clone()
        .oneshot(admin_request(
            Method::POST,
            "/api/v1/admin/trading/start",
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");
    assert_eq!(start_response.status(), StatusCode::OK);
    let started: TradingControlResponse = json_body(start_response).await;
    assert!(started.controls.trading_enabled);

    let accepted = app
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &trader.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 1,
            },
        ))
        .await
        .expect("response");
    assert_eq!(accepted.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn admin_can_manage_market_lifecycle_and_load_config() {
    let state = test_state();
    let app = build_app(state.clone());
    let trader = provision_user(&state, "market-admin-user");

    let create_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::POST,
            "/api/v1/admin/markets",
            "test-admin-token",
            &serde_json::json!({
                "display_name": "Solana",
                "base_asset": "sol",
                "tick_size": 5,
                "min_order_quantity": 2,
                "reference_price": 25,
                "enabled": true
            }),
        ))
        .await
        .expect("response");
    assert_eq!(create_response.status(), StatusCode::OK);
    let created: MarketDefinition = json_body(create_response).await;
    assert_eq!(created.market_id, "SOL-USD");
    assert_eq!(created.base_asset, "SOL");
    assert_eq!(created.quote_asset, "USD");
    assert_eq!(created.tick_size, 5);

    let patch_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::PATCH,
            "/api/v1/admin/markets/SOL-USD",
            "test-admin-token",
            &UpdateMarketRequest {
                display_name: None,
                tick_size: None,
                min_order_quantity: None,
                reference_price: None,
                enabled: Some(false),
            },
        ))
        .await
        .expect("response");
    assert_eq!(patch_response.status(), StatusCode::OK);
    let disabled: MarketDefinition = json_body(patch_response).await;
    assert_eq!(disabled.status, MarketStatus::Disabled);

    let disabled_order = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &trader.profile.api_key,
            &SubmitOrderRequest {
                market: "SOL-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 25,
                quantity: 2,
            },
        ))
        .await
        .expect("response");
    assert_eq!(disabled_order.status(), StatusCode::CONFLICT);

    let delete_response = app
        .clone()
        .oneshot(admin_request(
            Method::DELETE,
            "/api/v1/admin/markets/SOL-USD",
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");
    assert_eq!(delete_response.status(), StatusCode::OK);
    let deleted: DeleteMarketResponse = json_body(delete_response).await;
    assert_eq!(deleted.market_id, "SOL-USD");

    let load_response = app
        .oneshot(admin_json_request(
            Method::POST,
            "/api/v1/admin/config/load",
            "test-admin-token",
            &serde_json::json!({
                "trading_enabled": false,
                "markets": [
                    {
                        "display_name": "Dogecoin",
                        "base_asset": "doge",
                        "tick_size": 1,
                        "min_order_quantity": 10,
                        "reference_price": 1,
                        "enabled": true
                    }
                ]
            }),
        ))
        .await
        .expect("response");
    assert_eq!(load_response.status(), StatusCode::OK);
    let loaded: LoadExchangeConfigResponse = json_body(load_response).await;
    assert!(!loaded.controls.trading_enabled);
    assert!(
        loaded
            .markets
            .iter()
            .any(|market| market.market_id == "DOGE-USD")
    );
}

#[tokio::test]
async fn admin_messages_and_state_endpoint_round_trip() {
    let state = test_state();
    let app = build_app(state.clone());
    provision_user(&state, "message-user");

    let send_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::POST,
            "/api/v1/admin/messages",
            "test-admin-token",
            &SendAdminMessageRequest {
                target_username: Some("message-user".to_string()),
                market: Some("BTC-USD".to_string()),
                level: AdminMessageLevel::Warning,
                title: Some("Desk notice".to_string()),
                body: "Reduce size ahead of settlement.".to_string(),
            },
        ))
        .await
        .expect("response");
    assert_eq!(send_response.status(), StatusCode::OK);
    let sent: AdminMessageEntry = json_body(send_response).await;
    assert_eq!(sent.target_username.as_deref(), Some("message-user"));

    let state_response = app
        .oneshot(admin_request(
            Method::GET,
            "/api/v1/admin/state",
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");
    assert_eq!(state_response.status(), StatusCode::OK);
    let admin_state: AdminStateResponse = json_body(state_response).await;
    assert!(
        admin_state
            .recent_messages
            .iter()
            .any(|message| message.message_id == sent.message_id)
    );
}

#[tokio::test]
async fn admin_can_reset_all_user_trading_state() {
    let state = test_state();
    let app = build_app(state.clone());
    let maker = provision_user(&state, "reset-maker");
    let taker = provision_user(&state, "reset-taker");

    let maker_submit = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &maker.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 2,
            },
        ))
        .await
        .expect("response");
    assert_eq!(maker_submit.status(), StatusCode::CREATED);

    let taker_submit = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &taker.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 1,
            },
        ))
        .await
        .expect("response");
    assert_eq!(taker_submit.status(), StatusCode::CREATED);

    let reset_response = app
        .oneshot(admin_request(
            Method::POST,
            "/api/v1/admin/users/reset",
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");
    assert_eq!(reset_response.status(), StatusCode::OK);
    let reset: ResetUsersResponse = json_body(reset_response).await;
    assert_eq!(reset.cleared_orders, 1);
    assert_eq!(reset.cleared_positions, 2);
    assert_eq!(reset.cleared_fills, 2);

    assert!(state.storage.list_all_open_orders().is_empty());
    assert!(
        state
            .storage
            .list_all_positions()
            .into_iter()
            .all(|(_, positions)| positions.is_empty())
    );
    assert!(
        state
            .storage
            .list_fills(maker.profile.trader_id, None)
            .is_empty()
    );
    assert!(
        state
            .storage
            .list_fills(taker.profile.trader_id, None)
            .is_empty()
    );
}

#[tokio::test]
async fn admin_can_settle_market_and_leaderboard_reflects_result() {
    let state = test_state();
    let app = build_app(state.clone());
    let maker = provision_user(&state, "settle-maker");
    SettlementEngine::seed_position(&state, maker.profile.trader_id, "BTC-USD", 3, Some(100), 0);

    let submit_response = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &maker.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 120,
                quantity: 1,
            },
        ))
        .await
        .expect("response");
    assert_eq!(submit_response.status(), StatusCode::CREATED);

    let settle_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::POST,
            "/api/v1/admin/markets/BTC-USD/settle",
            "test-admin-token",
            &SettleMarketRequest {
                settlement_price: 150,
                announcement: None,
            },
        ))
        .await
        .expect("response");
    assert_eq!(settle_response.status(), StatusCode::OK);
    let settled: SettleMarketResponse = json_body(settle_response).await;
    assert_eq!(settled.market.status, MarketStatus::Settled);
    assert_eq!(settled.canceled_orders, 1);

    let maker_positions = state.storage.list_positions(maker.profile.trader_id);
    assert_eq!(maker_positions.len(), 1);
    assert_eq!(maker_positions[0].market, "BTC-USD");
    assert_eq!(maker_positions[0].net_quantity, 0);
    assert_eq!(maker_positions[0].realized_pnl, 150);

    let leaderboard_response = app
        .oneshot(api_key_request(
            Method::GET,
            "/api/v1/leaderboard",
            &maker.profile.api_key,
        ))
        .await
        .expect("response");
    assert_eq!(leaderboard_response.status(), StatusCode::OK);
    let leaderboard: Vec<LeaderboardRow> = json_body(leaderboard_response).await;
    assert_eq!(leaderboard[0].username, "settle-maker");
    assert_eq!(leaderboard[0].net_pnl, 150);
}

#[tokio::test]
async fn admin_can_finalize_competition_and_export_snapshot() {
    let state = test_state();
    let app = build_app(state.clone());
    let alice = provision_user(&state, "alice");
    let bob = provision_user(&state, "bob");
    let charlie = provision_user(&state, "charlie");

    SettlementEngine::seed_position(&state, alice.profile.trader_id, "BTC-USD", 2, Some(100), 0);
    SettlementEngine::seed_position(&state, bob.profile.trader_id, "BTC-USD", -1, Some(110), 0);
    SettlementEngine::seed_position(&state, charlie.profile.trader_id, "BTC-USD", 5, Some(50), 0);

    let resting = app
        .clone()
        .oneshot(api_key_json_request(
            Method::POST,
            "/api/v1/orders",
            &alice.profile.api_key,
            &SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 130,
                quantity: 1,
            },
        ))
        .await
        .expect("response");
    assert_eq!(resting.status(), StatusCode::CREATED);

    let finalize_response = app
        .clone()
        .oneshot(admin_json_request(
            Method::POST,
            "/api/v1/admin/competition/finalize",
            "test-admin-token",
            &FinalizeCompetitionRequest {
                competition_id: "spring-competition".to_string(),
                label: Some("Spring Finals".to_string()),
                settlements: vec![CompetitionSettlementRequest {
                    market_id: "BTC-USD".to_string(),
                    settlement_price: 120,
                }],
                eligible_usernames: vec!["alice".to_string(), "bob".to_string()],
                eligible_trader_ids: vec![],
                include_all_traders: false,
            },
        ))
        .await
        .expect("response");
    assert_eq!(finalize_response.status(), StatusCode::OK);
    let finalized: FinalizeCompetitionResponse = json_body(finalize_response).await;
    assert!(!finalized.controls.trading_enabled);
    assert_eq!(finalized.settled_markets.len(), 1);
    assert_eq!(finalized.settled_markets[0].canceled_orders, 1);
    assert_eq!(finalized.snapshot.competition_id, "spring-competition");
    assert_eq!(finalized.snapshot.label, "Spring Finals");
    assert_eq!(finalized.snapshot.entrants, 2);
    assert_eq!(
        finalized
            .snapshot
            .leaderboard
            .iter()
            .map(|row| row.username.as_str())
            .collect::<Vec<_>>(),
        vec!["alice", "bob"]
    );
    assert_eq!(finalized.snapshot.leaderboard[0].net_pnl, 40);
    assert_eq!(finalized.snapshot.leaderboard[1].net_pnl, -10);

    let latest_response = app
        .clone()
        .oneshot(admin_request(
            Method::GET,
            "/api/v1/admin/competition/snapshots/latest?competition_id=spring-competition",
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");
    assert_eq!(latest_response.status(), StatusCode::OK);
    let latest: CompetitionLeaderboardSnapshot = json_body(latest_response).await;
    assert_eq!(latest.snapshot_id, finalized.snapshot.snapshot_id);

    let by_id_response = app
        .clone()
        .oneshot(admin_request(
            Method::GET,
            &format!(
                "/api/v1/admin/competition/snapshots/{}",
                finalized.snapshot.snapshot_id
            ),
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");
    assert_eq!(by_id_response.status(), StatusCode::OK);
    let by_id: CompetitionLeaderboardSnapshot = json_body(by_id_response).await;
    assert_eq!(by_id, finalized.snapshot);

    let export_response = app
        .oneshot(admin_request(
            Method::GET,
            &format!(
                "/api/v1/admin/competition/snapshots/{}/export.csv",
                finalized.snapshot.snapshot_id
            ),
            "test-admin-token",
            Body::empty(),
        ))
        .await
        .expect("response");
    assert_eq!(export_response.status(), StatusCode::OK);
    assert_eq!(
        export_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/csv; charset=utf-8")
    );
    let export_csv = text_body(export_response).await;
    assert!(
        export_csv
            .contains("rank,trader_id,username,net_pnl,realized_pnl,unrealized_pnl,gross_exposure")
    );
    assert!(export_csv.contains(",alice,40,40,0,0"));
    assert!(export_csv.contains(",bob,-10,-10,0,0"));

    let alice_positions = state.storage.list_positions(alice.profile.trader_id);
    assert_eq!(alice_positions[0].net_quantity, 0);
    assert_eq!(alice_positions[0].realized_pnl, 40);
    let bob_positions = state.storage.list_positions(bob.profile.trader_id);
    assert_eq!(bob_positions[0].net_quantity, 0);
    assert_eq!(bob_positions[0].realized_pnl, -10);
}
