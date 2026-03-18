use exchange::{
    auth::{AuthService, ProvisionUserRequest, ProvisionUserResponse},
    build_app,
    config::Config,
    marketdata::{OrderStateStatus, ServerMessage},
    settlement::SettlementEngine,
    state::AppState,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

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

fn provision_user(state: &AppState, username: &str) -> ProvisionUserResponse {
    AuthService::provision_user(
        state,
        ProvisionUserRequest {
            username: username.to_string(),
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

#[tokio::test]
async fn websocket_authenticate_and_subscribe_round_trip() {
    let state = test_state();
    let trader = provision_user(&state, "socket-user");
    let response = exchange::trading::TradingService::submit_limit_order(
        &state,
        trader.profile.trader_id,
        exchange::trading::SubmitOrderRequest {
            market: "BTC-USD".to_string(),
            side: exchange::orderbook::Side::Buy,
            price: 100,
            quantity: 2,
        },
    )
    .await;
    assert!(response.is_err(), "balance seed should be required");
    SettlementEngine::seed_balance(&state, trader.profile.trader_id, "USD", 1_000);
    exchange::trading::TradingService::submit_limit_order(
        &state,
        trader.profile.trader_id,
        exchange::trading::SubmitOrderRequest {
            market: "BTC-USD".to_string(),
            side: exchange::orderbook::Side::Buy,
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
            "channel": "l3",
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
            assert_eq!(channel, "l3");
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
async fn websocket_submit_amend_cancel_flow_is_end_to_end() {
    let state = test_state();
    let trader = provision_user(&state, "edit-user");
    SettlementEngine::seed_balance(&state, trader.profile.trader_id, "USD", 1_000);

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
async fn websocket_crossing_trade_delivers_fill_and_order_state_to_both_sockets() {
    let state = test_state();
    let maker = provision_user(&state, "maker");
    let taker = provision_user(&state, "taker");
    SettlementEngine::seed_balance(&state, maker.profile.trader_id, "BTC", 2);
    SettlementEngine::seed_balance(&state, taker.profile.trader_id, "USD", 500);

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
