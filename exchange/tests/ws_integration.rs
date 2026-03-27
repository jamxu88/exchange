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
    marketdata::{BookDelta, MarketEvent, MarketL3Order, OrderStateStatus, ServerMessage},
    state::AppState,
    trading::OrderType,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
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
        database_url: "postgres://test".to_string(),
        storage_backend: exchange::storage::StorageBackendKind::InMemory,
        ws_broadcast_buffer: 64,
        ws_market_delta_batch_interval_ms: 10,
        ws_market_broadcast_workers: 1,
        market_data_service_socket: None,
        market_data_service_retry_backoff_ms: 250,
        runtime_dispatch_queue_capacity: 4_096,
        account_dispatch_queue_capacity: 4_096,
        persistence_dispatch_queue_capacity: 4_096,
        per_user_rate_limit_burst_capacity: per_user_burst_capacity,
        per_user_rate_limit_burst_window_seconds: 1,
        admin_api_token: "test-admin-token".to_string(),
        postgres_write_batch_size: 128,
        postgres_write_flush_interval_ms: 25,
        postgres_write_queue_capacity: 4_096,
        postgres_write_retry_backoff_ms: 250,
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

#[derive(Debug, PartialEq, Eq)]
struct ObservedL3Order {
    side: exchange::orderbook::Side,
    price: u64,
    remaining: u64,
}

#[derive(Default, Debug, PartialEq, Eq)]
struct ObservedL3Book {
    sequence: u64,
    orders: HashMap<uuid::Uuid, ObservedL3Order>,
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

fn replace_l3_snapshot(
    book: &mut ObservedL3Book,
    sequence: u64,
    bids: &[MarketL3Order],
    asks: &[MarketL3Order],
) {
    book.sequence = sequence;
    book.orders.clear();
    for order in bids {
        book.orders.insert(
            order.order_id,
            ObservedL3Order {
                side: exchange::orderbook::Side::Buy,
                price: order.price,
                remaining: order.remaining,
            },
        );
    }
    for order in asks {
        book.orders.insert(
            order.order_id,
            ObservedL3Order {
                side: exchange::orderbook::Side::Sell,
                price: order.price,
                remaining: order.remaining,
            },
        );
    }
}

fn apply_l3_delta(
    book: &mut ObservedL3Book,
    start_sequence: u64,
    sequence: u64,
    events: &[MarketEvent],
) {
    assert_eq!(start_sequence, book.sequence.saturating_add(1));
    for event in events {
        match event {
            MarketEvent::OrderAdded {
                order_id,
                side,
                price,
                remaining,
                ..
            }
            | MarketEvent::OrderUpdated {
                order_id,
                side,
                price,
                remaining,
            } => {
                book.orders.insert(
                    *order_id,
                    ObservedL3Order {
                        side: *side,
                        price: *price,
                        remaining: *remaining,
                    },
                );
            }
            MarketEvent::OrderRemoved { order_id, .. } => {
                book.orders.remove(order_id);
            }
            MarketEvent::Trade { .. } => {}
        }
    }
    book.sequence = sequence;
}

fn aggregate_l3_book(book: &ObservedL3Book) -> ObservedL2Book {
    let mut aggregate = ObservedL2Book {
        sequence: book.sequence,
        ..ObservedL2Book::default()
    };
    for order in book.orders.values() {
        let levels = match order.side {
            exchange::orderbook::Side::Buy => &mut aggregate.bids,
            exchange::orderbook::Side::Sell => &mut aggregate.asks,
        };
        *levels.entry(order.price).or_insert(0) += order.remaining;
    }
    aggregate
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
            "channel": "l2",
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

async fn fetch_fresh_l3_snapshot(url: &str, api_key: &str, market: &str) -> ObservedL3Book {
    let mut socket = connect_socket(url).await;
    authenticate(&mut socket, api_key).await;
    send_json(
        &mut socket,
        json!({
            "op": "subscribe",
            "channel": "l3",
            "market": market,
        }),
    )
    .await;

    match next_server_message(&mut socket).await {
        ServerMessage::L3Snapshot {
            sequence,
            bids,
            asks,
            ..
        } => {
            let mut book = ObservedL3Book::default();
            replace_l3_snapshot(&mut book, sequence, &bids, &asks);
            book
        }
        other => panic!("unexpected fresh l3 snapshot: {other:?}"),
    }
}

#[tokio::test]
async fn websocket_authenticate_and_subscribe_l2_round_trip() {
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
            "channel": "l2",
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
            assert_eq!(channel, "l2");
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
async fn websocket_authenticated_l3_subscribe_streams_raw_order_events() {
    let state = test_state();
    let viewer = provision_user(&state, "l3-viewer");
    let maker = provision_user(&state, "l3-maker");
    let taker = provision_user(&state, "l3-taker");

    let (url, server) = spawn_server(state.clone()).await;
    let mut l3_socket = connect_socket(&url).await;
    authenticate(&mut l3_socket, &viewer.profile.api_key).await;

    send_json(
        &mut l3_socket,
        json!({
            "op": "subscribe",
            "channel": "l3",
            "market": "BTC-USD",
        }),
    )
    .await;

    match next_server_message(&mut l3_socket).await {
        ServerMessage::L3Snapshot {
            channel,
            market,
            sequence,
            bids,
            asks,
        } => {
            assert_eq!(channel, "l3");
            assert_eq!(market, "BTC-USD");
            assert_eq!(sequence, 0);
            assert!(bids.is_empty());
            assert!(asks.is_empty());
        }
        other => panic!("unexpected l3 snapshot reply: {other:?}"),
    }

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
    .expect("maker order");

    match next_server_message(&mut l3_socket).await {
        ServerMessage::L3Delta {
            channel,
            market,
            start_sequence,
            sequence,
            events,
        } => {
            assert_eq!(channel, "l3");
            assert_eq!(market, "BTC-USD");
            assert_eq!(start_sequence, sequence);
            assert!(matches!(
                events.as_slice(),
                [exchange::marketdata::MarketEvent::OrderAdded {
                    price: 100,
                    remaining: 2,
                    ..
                }]
            ));
        }
        other => panic!("unexpected l3 add delta: {other:?}"),
    }

    exchange::trading::TradingService::submit_limit_order(
        &state,
        taker.profile.trader_id,
        exchange::trading::SubmitOrderRequest {
            market: "BTC-USD".to_string(),
            side: exchange::orderbook::Side::Buy,
            order_type: OrderType::Limit,
            price: 100,
            quantity: 2,
        },
    )
    .await
    .expect("taker order");

    let first_trade_delta = next_server_message(&mut l3_socket).await;
    let second_trade_delta = next_server_message(&mut l3_socket).await;
    let mut saw_trade = false;
    let mut saw_remove = false;
    for message in [first_trade_delta, second_trade_delta] {
        match message {
            ServerMessage::L3Delta {
                channel,
                market,
                events,
                ..
            } => {
                assert_eq!(channel, "l3");
                assert_eq!(market, "BTC-USD");
                match events.as_slice() {
                    [
                        exchange::marketdata::MarketEvent::Trade {
                            price: 100,
                            quantity: 2,
                            ..
                        },
                    ] => {
                        saw_trade = true;
                    }
                    [exchange::marketdata::MarketEvent::OrderRemoved { reason, .. }] => {
                        assert_eq!(
                            *reason,
                            exchange::marketdata::MarketEventRemoveReason::Filled
                        );
                        saw_remove = true;
                    }
                    other => panic!("unexpected l3 trade batch: {other:?}"),
                }
            }
            other => panic!("unexpected post-trade l3 message: {other:?}"),
        }
    }
    assert!(saw_trade);
    assert!(saw_remove);

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_l2_and_l3_match_fresh_snapshots_after_burst_load() {
    let state = test_state();
    let l3_viewer = provision_user(&state, "l3-volume-viewer");
    let traders = [
        provision_user(&state, "burst-a"),
        provision_user(&state, "burst-b"),
        provision_user(&state, "burst-c"),
        provision_user(&state, "burst-d"),
    ];

    let (url, server) = spawn_server(state.clone()).await;
    let mut l2_socket = connect_socket(&url).await;
    let mut l3_socket = connect_socket(&url).await;
    authenticate(&mut l3_socket, &l3_viewer.profile.api_key).await;

    send_json(
        &mut l2_socket,
        json!({
            "op": "subscribe",
            "channel": "l2",
            "market": "BTC-USD",
        }),
    )
    .await;
    send_json(
        &mut l3_socket,
        json!({
            "op": "subscribe",
            "channel": "l3",
            "market": "BTC-USD",
        }),
    )
    .await;

    let mut observed_l2 = match next_server_message(&mut l2_socket).await {
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
        other => panic!("unexpected initial l2 snapshot: {other:?}"),
    };
    let mut observed_l3 = match next_server_message(&mut l3_socket).await {
        ServerMessage::L3Snapshot {
            sequence,
            bids,
            asks,
            ..
        } => {
            let mut book = ObservedL3Book::default();
            replace_l3_snapshot(&mut book, sequence, &bids, &asks);
            book
        }
        other => panic!("unexpected initial l3 snapshot: {other:?}"),
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
            maybe_next_server_message(&mut l2_socket, Duration::from_millis(20)).await
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
                    apply_l2_delta(&mut observed_l2, start_sequence, sequence, &events);
                }
                ServerMessage::ResyncRequired { .. } => {
                    panic!("unexpected l2 resync under burst load")
                }
                other => panic!("unexpected l2 message under burst load: {other:?}"),
            }
        }

        while let Some(message) =
            maybe_next_server_message(&mut l3_socket, Duration::from_millis(20)).await
        {
            progressed = true;
            match message {
                ServerMessage::L3Delta {
                    start_sequence,
                    sequence,
                    events,
                    ..
                } => {
                    apply_l3_delta(&mut observed_l3, start_sequence, sequence, &events);
                }
                ServerMessage::ResyncRequired { .. } => {
                    panic!("unexpected l3 resync under burst load")
                }
                other => panic!("unexpected l3 message under burst load: {other:?}"),
            }
        }

        if !progressed {
            break;
        }
    }

    assert!(
        saw_batched_l2_delta,
        "expected at least one batched l2 delta"
    );

    let fresh_l2 = fetch_fresh_l2_snapshot(&url, "BTC-USD").await;
    let fresh_l3 = fetch_fresh_l3_snapshot(&url, &l3_viewer.profile.api_key, "BTC-USD").await;

    assert_eq!(observed_l2, fresh_l2);
    assert_eq!(observed_l3, fresh_l3);
    let aggregated_l3 = aggregate_l3_book(&observed_l3);
    assert_eq!(aggregated_l3.bids, fresh_l2.bids);
    assert_eq!(aggregated_l3.asks, fresh_l2.asks);

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
