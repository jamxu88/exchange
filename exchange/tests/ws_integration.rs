use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use chrono::Utc;
use exchange::{
    admin::{
        AdminMessageLevel, AdminService, MarketDefinition, MarketStatus, SendAdminMessageRequest,
        SettleMarketRequest, UpdateMarketRequest,
    },
    auth::{AuthService, AuthenticatedAdmin, ProvisionUserRequest, ProvisionUserResponse},
    build_app,
    config::Config,
    marketdata::{BookDelta, OrderStateStatus, ServerMessage},
    state::AppState,
    trading::OrderType,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};
use tower::ServiceExt;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn test_state() -> AppState {
    test_state_with_rate_limit(100)
}

fn test_state_with_rate_limit(per_user_burst_capacity: u64) -> AppState {
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
        per_user_rate_limit_burst_capacity: per_user_burst_capacity,
        per_user_rate_limit_burst_window_seconds: 1,
        admin_api_token: "test-admin-token".to_string(),
    });
    seed_market(&state, "BTC-USD", "BTC", "USD");
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
        min_price: None,
        max_price: None,
        reference_price: None,
        settlement_price: None,
        status: MarketStatus::Enabled,
        created_at: now,
        updated_at: now,
    });
}

fn provision_user(state: &AppState, username: &str) -> ProvisionUserResponse {
    AuthService::provision_user(
        state,
        ProvisionUserRequest {
            username: username.to_string(),
            team_number: None,
            role: None,
        },
    )
    .expect("provision user")
}

async fn spawn_server(state: AppState) -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let app = build_app(state);
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve app");
    });
    (format!("ws://{addr}/ws"), handle)
}

async fn connect_socket(url: &str) -> WsStream {
    let (socket, _) = connect_async(url).await.expect("connect websocket");
    socket
}

async fn send_json(socket: &mut WsStream, payload: serde_json::Value) {
    socket
        .send(Message::Text(payload.to_string().into()))
        .await
        .expect("send websocket message");
}

async fn next_server_message(socket: &mut WsStream) -> ServerMessage {
    loop {
        let frame = timeout(Duration::from_secs(1), socket.next())
            .await
            .expect("timed out waiting for websocket frame")
            .expect("socket should stay open")
            .expect("websocket frame");

        match frame {
            Message::Text(text) => {
                let message: ServerMessage =
                    serde_json::from_str(&text).expect("server json message");
                if matches!(message, ServerMessage::Heartbeat) {
                    continue;
                }
                return message;
            }
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => continue,
            Message::Close(frame) => panic!("unexpected websocket close: {frame:?}"),
            _ => continue,
        }
    }
}

async fn authenticate(socket: &mut WsStream, api_key: &str) {
    send_json(
        socket,
        json!({
            "op": "authenticate",
            "api_key": api_key,
        }),
    )
    .await;

    match next_server_message(socket).await {
        ServerMessage::Authenticated { .. } => {}
        other => panic!("unexpected auth reply: {other:?}"),
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
struct ObservedL2Book {
    sequence: u64,
    bids: BTreeMap<u64, u64>,
    asks: BTreeMap<u64, u64>,
}

fn replace_l2_snapshot(
    book: &mut ObservedL2Book,
    sequence: u64,
    bids: &[exchange::orderbook::BookLevel],
    asks: &[exchange::orderbook::BookLevel],
) {
    book.sequence = sequence;
    book.bids = bids
        .iter()
        .map(|level| (level.price, level.quantity))
        .collect();
    book.asks = asks
        .iter()
        .map(|level| (level.price, level.quantity))
        .collect();
}

fn apply_l2_delta(
    book: &mut ObservedL2Book,
    start_sequence: u64,
    sequence: u64,
    events: &[BookDelta],
) {
    assert_eq!(start_sequence, book.sequence.saturating_add(1));
    for event in events {
        match event {
            BookDelta::LevelUpdated {
                side,
                price,
                quantity,
            } => {
                let levels = match side {
                    exchange::orderbook::Side::Buy => &mut book.bids,
                    exchange::orderbook::Side::Sell => &mut book.asks,
                };
                if *quantity == 0 {
                    levels.remove(price);
                } else {
                    levels.insert(*price, *quantity);
                }
            }
            BookDelta::Trade { .. } => {}
        }
    }
    book.sequence = sequence;
}

async fn maybe_next_server_message(socket: &mut WsStream, wait: Duration) -> Option<ServerMessage> {
    loop {
        let frame = timeout(wait, socket.next()).await.ok()??.ok()?;
        match frame {
            Message::Text(text) => {
                let message: ServerMessage =
                    serde_json::from_str(&text).expect("server json message");
                if matches!(message, ServerMessage::Heartbeat) {
                    continue;
                }
                return Some(message);
            }
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => continue,
            Message::Close(frame) => panic!("unexpected websocket close: {frame:?}"),
            _ => continue,
        }
    }
}

async fn fetch_fresh_l2_snapshot(url: &str, market: &str) -> ObservedL2Book {
    let mut socket = connect_socket(url).await;
    send_json(
        &mut socket,
        json!({
            "op": "subscribe",
            "channel": "data",
            "market": market,
        }),
    )
    .await;

    match next_server_message(&mut socket).await {
        ServerMessage::Snapshot {
            sequence,
            bids,
            asks,
            ..
        } => {
            let mut book = ObservedL2Book::default();
            replace_l2_snapshot(&mut book, sequence, &bids, &asks);
            book
        }
        other => panic!("unexpected fresh l2 snapshot: {other:?}"),
    }
}

#[tokio::test]
async fn websocket_authenticate_and_subscribe_data_stream_round_trip() {
    let state = test_state();
    let trader = provision_user(&state, "socket-user");
    exchange::trading::TradingService::submit_limit_order(
        &state,
        trader.profile.trader_id,
        exchange::trading::SubmitOrderRequest {
            market: "BTC-USD".to_string(),
            side: exchange::orderbook::Side::Buy,
            order_type: OrderType::Limit,
            price: 100,
            quantity: 2,
        },
    )
    .await
    .expect("resting order");

    let (url, server) = spawn_server(state).await;
    let mut socket = connect_socket(&url).await;
    authenticate(&mut socket, &trader.profile.api_key).await;

    send_json(
        &mut socket,
        json!({
            "op": "subscribe",
            "channel": "data",
            "market": "BTC-USD",
        }),
    )
    .await;

    match next_server_message(&mut socket).await {
        ServerMessage::Snapshot {
            channel,
            market,
            bids,
            asks,
            ..
        } => {
            assert_eq!(channel, "data");
            assert_eq!(market, "BTC-USD");
            assert_eq!(bids.len(), 1);
            assert!(asks.is_empty());
        }
        other => panic!("unexpected snapshot reply: {other:?}"),
    }

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_data_stream_matches_fresh_snapshot_after_burst_load() {
    let state = test_state();
    let traders = [
        provision_user(&state, "burst-a"),
        provision_user(&state, "burst-b"),
        provision_user(&state, "burst-c"),
        provision_user(&state, "burst-d"),
    ];

    let (url, server) = spawn_server(state.clone()).await;
    let mut data_socket = connect_socket(&url).await;

    send_json(
        &mut data_socket,
        json!({
            "op": "subscribe",
            "channel": "data",
            "market": "BTC-USD",
        }),
    )
    .await;

    let mut observed_book = match next_server_message(&mut data_socket).await {
        ServerMessage::Snapshot {
            sequence,
            bids,
            asks,
            ..
        } => {
            let mut book = ObservedL2Book::default();
            replace_l2_snapshot(&mut book, sequence, &bids, &asks);
            book
        }
        other => panic!("unexpected initial data snapshot: {other:?}"),
    };

    for round in 0..24_u64 {
        let maker = &traders[(round as usize) % traders.len()];
        let taker = &traders[((round as usize) + 1) % traders.len()];
        let resting_sell_price = 101 + (round % 3);
        let resting_buy_price = 99 - (round % 3);
        let crossing_buy_price = 104;
        let crossing_sell_price = 96;
        let quantity = 1 + (round % 3);

        exchange::trading::TradingService::submit_limit_order(
            &state,
            maker.profile.trader_id,
            exchange::trading::SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: exchange::orderbook::Side::Sell,
                order_type: OrderType::Limit,
                price: resting_sell_price,
                quantity,
            },
        )
        .await
        .expect("resting sell");
        exchange::trading::TradingService::submit_limit_order(
            &state,
            maker.profile.trader_id,
            exchange::trading::SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: exchange::orderbook::Side::Buy,
                order_type: OrderType::Limit,
                price: resting_buy_price,
                quantity,
            },
        )
        .await
        .expect("resting buy");
        exchange::trading::TradingService::submit_limit_order(
            &state,
            taker.profile.trader_id,
            exchange::trading::SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: exchange::orderbook::Side::Buy,
                order_type: OrderType::Limit,
                price: crossing_buy_price,
                quantity,
            },
        )
        .await
        .expect("crossing buy");
        exchange::trading::TradingService::submit_limit_order(
            &state,
            taker.profile.trader_id,
            exchange::trading::SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: exchange::orderbook::Side::Sell,
                order_type: OrderType::Limit,
                price: crossing_sell_price,
                quantity,
            },
        )
        .await
        .expect("crossing sell");
    }

    tokio::time::sleep(Duration::from_millis(80)).await;

    let mut saw_batched_l2_delta = false;
    loop {
        let mut progressed = false;

        while let Some(message) =
            maybe_next_server_message(&mut data_socket, Duration::from_millis(20)).await
        {
            progressed = true;
            match message {
                ServerMessage::Delta {
                    start_sequence,
                    sequence,
                    events,
                    ..
                } => {
                    if events.len() > 1 {
                        saw_batched_l2_delta = true;
                    }
                    apply_l2_delta(&mut observed_book, start_sequence, sequence, &events);
                }
                ServerMessage::ResyncRequired { .. } => {
                    panic!("unexpected data stream resync under burst load")
                }
                other => panic!("unexpected data stream message under burst load: {other:?}"),
            }
        }

        if !progressed {
            break;
        }
    }

    assert!(
        saw_batched_l2_delta,
        "expected at least one batched data-stream delta"
    );

    let fresh_book = fetch_fresh_l2_snapshot(&url, "BTC-USD").await;
    assert_eq!(observed_book, fresh_book);

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_reports_invalid_messages_and_recovers_for_valid_authentication() {
    let state = test_state();
    let trader = provision_user(&state, "recovering-socket-user");
    let (url, server) = spawn_server(state).await;
    let mut socket = connect_socket(&url).await;

    socket
        .send(Message::Text(r#"{"op":"submit_order""#.to_string().into()))
        .await
        .expect("send malformed text");
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Error {
            code: "invalid_message".to_string(),
            message: "invalid websocket message".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "authenticate",
            "api_key": "invalid",
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Error {
            code: "invalid_api_key".to_string(),
            message: "invalid websocket api key".to_string(),
        }
    );

    authenticate(&mut socket, &trader.profile.api_key).await;

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_submit_amend_cancel_flow_is_end_to_end() {
    let state = test_state();
    let trader = provision_user(&state, "edit-user");

    let (url, server) = spawn_server(state).await;
    let mut socket = connect_socket(&url).await;
    authenticate(&mut socket, &trader.profile.api_key).await;

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "submit-1",
            "market": "BTC-USD",
            "side": "BUY",
            "price": 100,
            "quantity": 3,
        }),
    )
    .await;

    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Ack {
            op: "submit_order".to_string(),
            request_id: Some("submit-1".to_string()),
        }
    );

    let order_id = match next_server_message(&mut socket).await {
        ServerMessage::OrderState { order, status } => {
            assert_eq!(status, OrderStateStatus::Open);
            assert_eq!(order.remaining, 3);
            order.id
        }
        other => panic!("unexpected submit state: {other:?}"),
    };

    send_json(
        &mut socket,
        json!({
            "op": "amend_order",
            "request_id": "amend-1",
            "order_id": order_id,
            "remaining": 1,
        }),
    )
    .await;

    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Ack {
            op: "amend_order".to_string(),
            request_id: Some("amend-1".to_string()),
        }
    );
    match next_server_message(&mut socket).await {
        ServerMessage::OrderState { order, status } => {
            assert_eq!(status, OrderStateStatus::Open);
            assert_eq!(order.id, order_id);
            assert_eq!(order.remaining, 1);
        }
        other => panic!("unexpected amend state: {other:?}"),
    }

    send_json(
        &mut socket,
        json!({
            "op": "cancel_order",
            "request_id": "cancel-1",
            "order_id": order_id,
        }),
    )
    .await;

    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Ack {
            op: "cancel_order".to_string(),
            request_id: Some("cancel-1".to_string()),
        }
    );
    match next_server_message(&mut socket).await {
        ServerMessage::OrderState { order, status } => {
            assert_eq!(status, OrderStateStatus::Canceled);
            assert_eq!(order.id, order_id);
        }
        other => panic!("unexpected cancel state: {other:?}"),
    }

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_rejects_invalid_payloads_and_market_state_conflicts() {
    let state = test_state();
    let trader = provision_user(&state, "invalid-ws-order-user");
    let now = Utc::now();
    state.storage.upsert_market(MarketDefinition {
        market_id: "SOL-USD".to_string(),
        display_name: "Solana".to_string(),
        base_asset: "SOL".to_string(),
        quote_asset: "USD".to_string(),
        tick_size: 5,
        min_order_quantity: 10,
        min_price: None,
        max_price: None,
        reference_price: Some(25),
        settlement_price: None,
        status: MarketStatus::Enabled,
        created_at: now,
        updated_at: now,
    });
    state.storage.upsert_market(MarketDefinition {
        market_id: "DOGE-USD".to_string(),
        display_name: "Dogecoin".to_string(),
        base_asset: "DOGE".to_string(),
        quote_asset: "USD".to_string(),
        tick_size: 1,
        min_order_quantity: 1,
        min_price: None,
        max_price: None,
        reference_price: Some(1),
        settlement_price: None,
        status: MarketStatus::Disabled,
        created_at: now,
        updated_at: now,
    });
    state.storage.upsert_market(MarketDefinition {
        market_id: "ADA-USD".to_string(),
        display_name: "Cardano".to_string(),
        base_asset: "ADA".to_string(),
        quote_asset: "USD".to_string(),
        tick_size: 1,
        min_order_quantity: 1,
        min_price: None,
        max_price: None,
        reference_price: Some(2),
        settlement_price: Some(3),
        status: MarketStatus::Settled,
        created_at: now,
        updated_at: now,
    });

    let (url, server) = spawn_server(state).await;
    let mut socket = connect_socket(&url).await;
    authenticate(&mut socket, &trader.profile.api_key).await;

    socket
        .send(Message::Text(
            r#"{"op":"submit_order","request_id":"bad-type","market":"BTC-USD","side":"BUY","order_type":"stop","price":100,"quantity":1}"#
                .to_string()
                .into(),
        ))
        .await
        .expect("send invalid order_type");
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Error {
            code: "invalid_message".to_string(),
            message: "invalid websocket message".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "bad-symbol",
            "market": "not a symbol",
            "side": "BUY",
            "price": 100,
            "quantity": 1,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("bad-symbol".to_string()),
            code: "invalid_market".to_string(),
            message: "invalid market symbol".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "not-configured",
            "market": "XRP-USD",
            "side": "BUY",
            "price": 100,
            "quantity": 1,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("not-configured".to_string()),
            code: "market_not_configured".to_string(),
            message: "market is not configured".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "disabled-market",
            "market": "DOGE-USD",
            "side": "BUY",
            "price": 1,
            "quantity": 1,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("disabled-market".to_string()),
            code: "market_disabled".to_string(),
            message: "market is disabled".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "settled-market",
            "market": "ADA-USD",
            "side": "BUY",
            "price": 3,
            "quantity": 1,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("settled-market".to_string()),
            code: "market_settled".to_string(),
            message: "market has already been settled".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "zero-quantity",
            "market": "SOL-USD",
            "side": "BUY",
            "price": 25,
            "quantity": 0,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("zero-quantity".to_string()),
            code: "invalid_quantity".to_string(),
            message: "quantity must be greater than zero".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "tick-violation",
            "market": "SOL-USD",
            "side": "BUY",
            "price": 26,
            "quantity": 10,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("tick-violation".to_string()),
            code: "tick_size_violation".to_string(),
            message: "price must align to tick size 5".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "below-minimum",
            "market": "SOL-USD",
            "side": "BUY",
            "price": 25,
            "quantity": 9,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("below-minimum".to_string()),
            code: "quantity_below_minimum".to_string(),
            message: "quantity must be at least 10".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "no-liquidity",
            "market": "BTC-USD",
            "side": "BUY",
            "order_type": "market",
            "price": 0,
            "quantity": 1,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("no-liquidity".to_string()),
            code: "no_liquidity".to_string(),
            message:
                "market order could not be filled because no opposite-side liquidity is available"
                    .to_string(),
        }
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_amend_and_cancel_reject_missing_orders() {
    let state = test_state();
    let trader = provision_user(&state, "missing-order-user");
    let (url, server) = spawn_server(state).await;
    let mut socket = connect_socket(&url).await;
    authenticate(&mut socket, &trader.profile.api_key).await;
    let missing_order_id = uuid::Uuid::new_v4();

    send_json(
        &mut socket,
        json!({
            "op": "amend_order",
            "request_id": "missing-amend",
            "order_id": missing_order_id,
            "remaining": 1,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "amend_order".to_string(),
            request_id: Some("missing-amend".to_string()),
            code: "order_not_found".to_string(),
            message: "order not found".to_string(),
        }
    );

    send_json(
        &mut socket,
        json!({
            "op": "cancel_order",
            "request_id": "missing-cancel",
            "order_id": missing_order_id,
        }),
    )
    .await;
    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "cancel_order".to_string(),
            request_id: Some("missing-cancel".to_string()),
            code: "order_not_found".to_string(),
            message: "order not found".to_string(),
        }
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_market_order_submits_without_resting() {
    let state = test_state();
    let maker = provision_user(&state, "market-maker");
    let taker = provision_user(&state, "market-taker");

    exchange::trading::TradingService::submit_limit_order(
        &state,
        maker.profile.trader_id,
        exchange::trading::SubmitOrderRequest {
            market: "BTC-USD".to_string(),
            side: exchange::orderbook::Side::Sell,
            order_type: OrderType::Limit,
            price: 100,
            quantity: 2,
        },
    )
    .await
    .expect("maker order should rest");

    let (url, server) = spawn_server(state).await;
    let mut socket = connect_socket(&url).await;
    authenticate(&mut socket, &taker.profile.api_key).await;

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "market-1",
            "market": "BTC-USD",
            "side": "BUY",
            "order_type": "market",
            "price": 0,
            "quantity": 2,
        }),
    )
    .await;

    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Ack {
            op: "submit_order".to_string(),
            request_id: Some("market-1".to_string()),
        }
    );

    match next_server_message(&mut socket).await {
        ServerMessage::Fill { fill } => {
            assert_eq!(fill.price, 100);
            assert_eq!(fill.quantity, 2);
        }
        other => panic!("unexpected fill event: {other:?}"),
    }

    match next_server_message(&mut socket).await {
        ServerMessage::OrderState { order, status } => {
            assert_eq!(status, OrderStateStatus::Filled);
            assert_eq!(order.remaining, 0);
            assert_eq!(order.price, 100);
        }
        other => panic!("unexpected order state: {other:?}"),
    }

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_crossing_trade_delivers_fill_and_order_state_to_both_sockets() {
    let state = test_state();
    let maker = provision_user(&state, "maker");
    let taker = provision_user(&state, "taker");

    let (url, server) = spawn_server(state).await;
    let mut maker_socket = connect_socket(&url).await;
    let mut taker_socket = connect_socket(&url).await;
    authenticate(&mut maker_socket, &maker.profile.api_key).await;
    authenticate(&mut taker_socket, &taker.profile.api_key).await;

    send_json(
        &mut maker_socket,
        json!({
            "op": "submit_order",
            "request_id": "maker-1",
            "market": "BTC-USD",
            "side": "SELL",
            "price": 100,
            "quantity": 2,
        }),
    )
    .await;

    assert_eq!(
        next_server_message(&mut maker_socket).await,
        ServerMessage::Ack {
            op: "submit_order".to_string(),
            request_id: Some("maker-1".to_string()),
        }
    );
    let maker_order_id = match next_server_message(&mut maker_socket).await {
        ServerMessage::OrderState { order, status } => {
            assert_eq!(status, OrderStateStatus::Open);
            order.id
        }
        other => panic!("unexpected maker open state: {other:?}"),
    };

    send_json(
        &mut taker_socket,
        json!({
            "op": "submit_order",
            "request_id": "taker-1",
            "market": "BTC-USD",
            "side": "BUY",
            "price": 100,
            "quantity": 2,
        }),
    )
    .await;

    assert_eq!(
        next_server_message(&mut taker_socket).await,
        ServerMessage::Ack {
            op: "submit_order".to_string(),
            request_id: Some("taker-1".to_string()),
        }
    );
    match next_server_message(&mut taker_socket).await {
        ServerMessage::Fill { fill } => {
            assert_eq!(fill.price, 100);
            assert_eq!(fill.quantity, 2);
            assert_eq!(fill.maker_order_id, maker_order_id);
        }
        other => panic!("unexpected taker fill: {other:?}"),
    }
    match next_server_message(&mut taker_socket).await {
        ServerMessage::OrderState { order, status } => {
            assert_eq!(status, OrderStateStatus::Filled);
            assert_eq!(order.remaining, 0);
        }
        other => panic!("unexpected taker order state: {other:?}"),
    }

    match next_server_message(&mut maker_socket).await {
        ServerMessage::Fill { fill } => {
            assert_eq!(fill.maker_order_id, maker_order_id);
            assert_eq!(fill.quantity, 2);
        }
        other => panic!("unexpected maker fill: {other:?}"),
    }
    match next_server_message(&mut maker_socket).await {
        ServerMessage::OrderState { order, status } => {
            assert_eq!(status, OrderStateStatus::Filled);
            assert_eq!(order.id, maker_order_id);
            assert_eq!(order.remaining, 0);
        }
        other => panic!("unexpected maker order state: {other:?}"),
    }

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_delivers_broadcast_admin_messages_to_authenticated_clients() {
    let state = test_state();
    let user = provision_user(&state, "message-recipient");
    let (url, server) = spawn_server(state.clone()).await;
    let mut socket = connect_socket(&url).await;
    authenticate(&mut socket, &user.profile.api_key).await;

    let sent = AdminService::send_message(
        &state,
        &AuthenticatedAdmin {
            username: "ops".to_string(),
        },
        SendAdminMessageRequest {
            target_username: None,
            market: Some("BTC-USD".to_string()),
            level: AdminMessageLevel::Info,
            title: Some("Desk notice".to_string()),
            body: "Trading will pause soon.".to_string(),
        },
    )
    .expect("send admin message");

    match next_server_message(&mut socket).await {
        ServerMessage::AdminMessage { message } => {
            assert_eq!(message.message_id, sent.message_id);
            assert_eq!(message.market.as_deref(), Some("BTC-USD"));
            assert_eq!(message.body, "Trading will pause soon.");
        }
        other => panic!("unexpected admin message event: {other:?}"),
    }

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_delivers_market_state_updates_without_authentication() {
    let state = test_state();
    let (url, server) = spawn_server(state.clone()).await;
    let mut socket = connect_socket(&url).await;

    let updated = AdminService::update_market(
        &state,
        &AuthenticatedAdmin {
            username: "ops".to_string(),
        },
        "BTC-USD",
        UpdateMarketRequest {
            display_name: Some("Bitcoin".to_string()),
            tick_size: None,
            min_order_quantity: None,
            min_price: None,
            max_price: None,
            reference_price: None,
            enabled: Some(false),
        },
    )
    .expect("update market");

    match next_server_message(&mut socket).await {
        ServerMessage::MarketState { market } => {
            assert_eq!(market.market_id, updated.market_id);
            assert_eq!(market.display_name, "Bitcoin");
            assert_eq!(market.status, MarketStatus::Disabled);
        }
        other => panic!("unexpected market state event: {other:?}"),
    }

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_delivers_market_delete_events_without_authentication() {
    let state = test_state();
    let (url, server) = spawn_server(state.clone()).await;
    let mut socket = connect_socket(&url).await;

    let deleted = AdminService::delete_market(
        &state,
        &AuthenticatedAdmin {
            username: "ops".to_string(),
        },
        "BTC-USD",
    )
    .expect("delete market");

    match next_server_message(&mut socket).await {
        ServerMessage::MarketDeleted { market_id } => {
            assert_eq!(market_id, deleted.market_id);
        }
        other => panic!("unexpected market deleted event: {other:?}"),
    }

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_delivers_settlement_market_state_transitions_without_authentication() {
    let state = test_state();
    let (url, server) = spawn_server(state.clone()).await;
    let mut socket = connect_socket(&url).await;

    let settled = AdminService::settle_market(
        &state,
        &AuthenticatedAdmin {
            username: "ops".to_string(),
        },
        "BTC-USD",
        SettleMarketRequest {
            settlement_price: 123,
            announcement: None,
        },
    )
    .await
    .expect("settle market");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let mut observed_statuses = Vec::new();
    while observed_statuses.len() < 2 {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Some(message) = maybe_next_server_message(&mut socket, remaining).await else {
            break;
        };
        if let ServerMessage::MarketState { market } = message {
            observed_statuses.push((market.status, market.settlement_price));
        }
    }

    assert_eq!(
        observed_statuses,
        vec![
            (MarketStatus::Disabled, None),
            (MarketStatus::Settled, Some(settled.settlement_price)),
        ]
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn reset_all_users_broadcasts_market_resync_and_clears_follow_up_snapshots() {
    let state = test_state();
    let trader = provision_user(&state, "reset-book-user");
    exchange::trading::TradingService::submit_limit_order(
        &state,
        trader.profile.trader_id,
        exchange::trading::SubmitOrderRequest {
            market: "BTC-USD".to_string(),
            side: exchange::orderbook::Side::Buy,
            order_type: OrderType::Limit,
            price: 100,
            quantity: 3,
        },
    )
    .await
    .expect("seed open order");

    let (url, server) = spawn_server(state.clone()).await;
    let mut socket = connect_socket(&url).await;

    send_json(
        &mut socket,
        json!({
            "op": "subscribe",
            "channel": "data",
            "market": "BTC-USD",
        }),
    )
    .await;

    match next_server_message(&mut socket).await {
        ServerMessage::Snapshot {
            market,
            sequence,
            bids,
            asks,
            ..
        } => {
            assert_eq!(market, "BTC-USD");
            assert!(sequence > 0);
            assert_eq!(bids.len(), 1);
            assert_eq!(bids[0].price, 100);
            assert_eq!(bids[0].quantity, 3);
            assert!(asks.is_empty());
        }
        other => panic!("unexpected initial snapshot: {other:?}"),
    }

    let reset = AdminService::reset_all_users(
        &state,
        &AuthenticatedAdmin {
            username: "ops".to_string(),
        },
    );
    assert_eq!(reset.cleared_orders, 1);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let mut saw_data_resync = false;
    while tokio::time::Instant::now() < deadline && !saw_data_resync {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Some(message) = maybe_next_server_message(&mut socket, remaining).await else {
            break;
        };
        if let ServerMessage::ResyncRequired {
            channel,
            market,
            reason,
            ..
        } = message
        {
            if channel == "data" && market.as_deref() == Some("BTC-USD") {
                assert!(reason.contains("fresh snapshot"));
                saw_data_resync = true;
            }
        }
    }

    assert!(
        saw_data_resync,
        "expected BTC-USD data stream resync after reset"
    );

    send_json(
        &mut socket,
        json!({
            "op": "subscribe",
            "channel": "data",
            "market": "BTC-USD",
        }),
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Some(message) = maybe_next_server_message(&mut socket, remaining).await else {
            panic!("timed out waiting for post-reset snapshot");
        };
        match message {
            ServerMessage::Snapshot {
                market, bids, asks, ..
            } => {
                assert_eq!(market, "BTC-USD");
                assert!(bids.is_empty());
                assert!(asks.is_empty());
                break;
            }
            ServerMessage::ResyncRequired { .. } => continue,
            other => panic!("unexpected post-reset message: {other:?}"),
        }
    }

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_trading_shares_the_per_user_rate_limit_budget() {
    let state = test_state_with_rate_limit(1);
    let app = build_app(state.clone());
    let trader = provision_user(&state, "rate-limited-ws-user");
    let (url, server) = spawn_server(state).await;

    let rest_response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/positions")
                .header("x-api-key", trader.profile.api_key.clone())
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(rest_response.status(), StatusCode::OK);

    let mut socket = connect_socket(&url).await;
    authenticate(&mut socket, &trader.profile.api_key).await;

    send_json(
        &mut socket,
        json!({
            "op": "submit_order",
            "request_id": "submit-rate-limited",
            "market": "BTC-USD",
            "side": "BUY",
            "price": 100,
            "quantity": 1,
        }),
    )
    .await;

    assert_eq!(
        next_server_message(&mut socket).await,
        ServerMessage::Reject {
            op: "submit_order".to_string(),
            request_id: Some("submit-rate-limited".to_string()),
            code: "rate_limit_exceeded".to_string(),
            message: "per-user rate limit exceeded: max 1 ops per 1s".to_string(),
        }
    );

    server.abort();
    let _ = server.await;
}
