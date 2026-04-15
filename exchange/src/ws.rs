use crate::auth::{AuthService, AuthenticatedUser};
use crate::marketdata::{ClientMessage, DATA_STREAM_CHANNEL, ServerMessage, UserBroadcastEvent};
use crate::rate_limit::enforce_authenticated_user_rate_limit;
use crate::state::AppState;
use crate::trading::{AmendOrderRequest, SubmitOrderRequest, TradingError, TradingService};
use axum::{
    extract::{State, WebSocketUpgrade, ws::Message, ws::WebSocket},
    response::IntoResponse,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast::error::RecvError, mpsc as tokio_mpsc};

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client_loop(socket, state))
}

struct ClientConnection {
    book_stream_id: uuid::Uuid,
    authenticated_user: Option<AuthenticatedUser>,
    subscription: Option<MarketSubscription>,
}

struct MarketSubscription {
    channel: String,
    market: String,
}

async fn client_loop(mut socket: WebSocket, state: AppState) {
    state.operator_telemetry().record_ws_connection_open();
    let (market_tx, mut market_rx) = tokio_mpsc::unbounded_channel::<Arc<ServerMessage>>();
    let book_stream_id = state.register_book_stream(market_tx);
    let mut public_rx = state.public_events_tx.subscribe();
    let mut user_rx = state.user_events_tx.subscribe();
    let mut system_rx = state.system_events_tx.subscribe();
    let mut ping_interval = tokio::time::interval(Duration::from_secs(15));
    let mut connection = ClientConnection {
        book_stream_id,
        authenticated_user: None,
        subscription: None,
    };

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if send_server_message(&mut socket, &ServerMessage::Heartbeat).await.is_err() {
                    break;
                }
            }
            message = market_rx.recv() => {
                match message {
                    Some(message) => {
                        if send_server_message(&mut socket, message.as_ref()).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            event = public_rx.recv() => {
                match event {
                    Ok(message) => {
                        if send_server_message(&mut socket, &message).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        let message = public_resync_required(skipped);
                        if send_server_message(&mut socket, &message).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            event = user_rx.recv() => {
                match event {
                    Ok(UserBroadcastEvent { trader_id, message }) => {
                        if connection
                            .authenticated_user
                            .as_ref()
                            .map(|user| user.trader_id)
                            == Some(trader_id)
                            && send_server_message(&mut socket, &message).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        if let Some(message) = user_resync_required(&state, &connection, skipped) {
                            if send_server_message(&mut socket, &message).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            event = system_rx.recv() => {
                match event {
                    Ok(message) => {
                        if connection.authenticated_user.is_some()
                            && send_server_message(&mut socket, &message).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        if let Some(message) = system_resync_required(&state, &connection, skipped) {
                            if send_server_message(&mut socket, &message).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        let replies = handle_client_text(&state, &mut connection, &text).await;
                        for reply in replies {
                            if send_server_message(&mut socket, &reply).await.is_err() {
                                cleanup_connection(&state, &connection);
                                return;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }

    cleanup_connection(&state, &connection);
}

async fn handle_client_text(
    state: &AppState,
    connection: &mut ClientConnection,
    text: &str,
) -> Vec<ServerMessage> {
    let message = match serde_json::from_str::<ClientMessage>(text) {
        Ok(message) => message,
        Err(_) => {
            return vec![ServerMessage::Error {
                code: "invalid_message".to_string(),
                message: "invalid websocket message".to_string(),
            }];
        }
    };

    match message {
        ClientMessage::Authenticate { api_key } => {
            match AuthService::authenticate_api_key(state, &api_key) {
                Ok(user) => {
                    if connection.authenticated_user.is_none() {
                        state.operator_telemetry().record_ws_authenticated_open();
                    }
                    connection.authenticated_user = Some(user.clone());
                    vec![ServerMessage::Authenticated {
                        trader_id: user.trader_id,
                        team_number: user.team_number,
                    }]
                }
                Err(_) => vec![ServerMessage::Error {
                    code: "invalid_api_key".to_string(),
                    message: "invalid websocket api key".to_string(),
                }],
            }
        }
        ClientMessage::Subscribe {
            channel,
            market,
            last_sequence: _,
        } => match channel.as_str() {
            DATA_STREAM_CHANNEL => {
                update_subscription_telemetry(state, connection.subscription.as_ref(), None);
                let next_subscription = Some(MarketSubscription {
                    channel: channel.clone(),
                    market: market.clone(),
                });
                update_subscription_telemetry(state, None, next_subscription.as_ref());
                connection.subscription = next_subscription;
                state
                    .begin_book_stream_bootstrap(connection.book_stream_id, market.clone())
                    .await;
                let snapshot = build_book_snapshot_message(state, &market).await;
                let snapshot_sequence = match &snapshot {
                    ServerMessage::Snapshot { sequence, .. } => *sequence,
                    other => panic!("expected data stream snapshot response, got {other:?}"),
                };
                state.complete_book_stream_bootstrap(
                    connection.book_stream_id,
                    market.clone(),
                    snapshot_sequence,
                );
                vec![snapshot]
            }
            _ => vec![unsupported_channel_error()],
        },
        ClientMessage::Unsubscribe { channel, market } => {
            if channel != DATA_STREAM_CHANNEL {
                return vec![unsupported_channel_error()];
            }

            if connection
                .subscription
                .as_ref()
                .is_some_and(|subscription| {
                    subscription.channel == channel && subscription.market == market
                })
            {
                update_subscription_telemetry(state, connection.subscription.as_ref(), None);
                connection.subscription = None;
                state.update_book_stream_subscription(connection.book_stream_id, None, None);
            }
            vec![ServerMessage::Unsubscribed { channel, market }]
        }
        ClientMessage::SubmitOrder {
            request_id,
            market,
            side,
            order_type,
            price,
            quantity,
        } => {
            let Some(user) = connection.authenticated_user.clone() else {
                return vec![reject(
                    "submit_order",
                    request_id,
                    "unauthenticated",
                    "authenticate before trading",
                )];
            };
            if let Err(message) = enforce_authenticated_user_rate_limit(state, user.trader_id) {
                return vec![reject(
                    "submit_order",
                    request_id,
                    "rate_limit_exceeded",
                    &message,
                )];
            }

            match TradingService::submit_order(
                state,
                user.trader_id,
                SubmitOrderRequest {
                    market,
                    side,
                    order_type,
                    price,
                    quantity,
                },
            )
            .await
            {
                Ok(_) => vec![ack("submit_order", request_id)],
                Err(error) => vec![trading_reject("submit_order", request_id, error)],
            }
        }
        ClientMessage::CancelOrder {
            request_id,
            order_id,
        } => {
            let Some(user) = connection.authenticated_user.clone() else {
                return vec![reject(
                    "cancel_order",
                    request_id,
                    "unauthenticated",
                    "authenticate before trading",
                )];
            };
            if let Err(message) = enforce_authenticated_user_rate_limit(state, user.trader_id) {
                return vec![reject(
                    "cancel_order",
                    request_id,
                    "rate_limit_exceeded",
                    &message,
                )];
            }

            match TradingService::cancel_order(state, user.trader_id, order_id).await {
                Ok(_) => vec![ack("cancel_order", request_id)],
                Err(error) => vec![trading_reject("cancel_order", request_id, error)],
            }
        }
        ClientMessage::AmendOrder {
            request_id,
            order_id,
            remaining,
        } => {
            let Some(user) = connection.authenticated_user.clone() else {
                return vec![reject(
                    "amend_order",
                    request_id,
                    "unauthenticated",
                    "authenticate before trading",
                )];
            };
            if let Err(message) = enforce_authenticated_user_rate_limit(state, user.trader_id) {
                return vec![reject(
                    "amend_order",
                    request_id,
                    "rate_limit_exceeded",
                    &message,
                )];
            }

            match TradingService::amend_order(
                state,
                user.trader_id,
                order_id,
                AmendOrderRequest { remaining },
            )
            .await
            {
                Ok(_) => vec![ack("amend_order", request_id)],
                Err(error) => vec![trading_reject("amend_order", request_id, error)],
            }
        }
    }
}

fn unsupported_channel_error() -> ServerMessage {
    ServerMessage::Error {
        code: "unsupported_channel".to_string(),
        message: "supported market-data channel is data".to_string(),
    }
}

fn cleanup_connection(state: &AppState, connection: &ClientConnection) {
    update_subscription_telemetry(state, connection.subscription.as_ref(), None);
    if connection.authenticated_user.is_some() {
        state.operator_telemetry().record_ws_authenticated_close();
    }
    state.operator_telemetry().record_ws_connection_close();
    state.unregister_book_stream(connection.book_stream_id);
}

fn update_subscription_telemetry(
    state: &AppState,
    previous: Option<&MarketSubscription>,
    next: Option<&MarketSubscription>,
) {
    if let Some(subscription) = previous {
        if subscription.channel == DATA_STREAM_CHANNEL {
            state
                .operator_telemetry()
                .record_data_stream_subscriber_close();
        }
    }

    if let Some(subscription) = next {
        if subscription.channel == DATA_STREAM_CHANNEL {
            state
                .operator_telemetry()
                .record_data_stream_subscriber_open();
        }
    }
}

fn user_resync_required(
    state: &AppState,
    connection: &ClientConnection,
    skipped: u64,
) -> Option<ServerMessage> {
    connection.authenticated_user.as_ref()?;
    state.operator_telemetry().record_user_resync();
    Some(ServerMessage::ResyncRequired {
        channel: "user".to_string(),
        market: None,
        expected_sequence: None,
        current_sequence: None,
        reason: format!(
            "user event stream lagged by {skipped} messages; refresh account state and reconnect if needed"
        ),
    })
}

fn public_resync_required(skipped: u64) -> ServerMessage {
    ServerMessage::ResyncRequired {
        channel: "markets".to_string(),
        market: None,
        expected_sequence: None,
        current_sequence: None,
        reason: format!(
            "market event stream lagged by {skipped} messages; refresh market state from REST if needed"
        ),
    }
}

fn system_resync_required(
    state: &AppState,
    connection: &ClientConnection,
    skipped: u64,
) -> Option<ServerMessage> {
    connection.authenticated_user.as_ref()?;
    state.operator_telemetry().record_system_resync();
    Some(ServerMessage::ResyncRequired {
        channel: "system".to_string(),
        market: None,
        expected_sequence: None,
        current_sequence: None,
        reason: format!(
            "system event stream lagged by {skipped} messages; refresh state if needed"
        ),
    })
}

fn ack(op: &str, request_id: Option<String>) -> ServerMessage {
    ServerMessage::Ack {
        op: op.to_string(),
        request_id,
    }
}

fn reject(op: &str, request_id: Option<String>, code: &str, message: &str) -> ServerMessage {
    ServerMessage::Reject {
        op: op.to_string(),
        request_id,
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn trading_reject(op: &str, request_id: Option<String>, error: TradingError) -> ServerMessage {
    reject(
        op,
        request_id,
        trading_error_code(&error),
        &error.to_string(),
    )
}

fn trading_error_code(error: &TradingError) -> &'static str {
    match error {
        TradingError::TradingDisabled => "trading_disabled",
        TradingError::InvalidMarket => "invalid_market",
        TradingError::MarketNotConfigured => "market_not_configured",
        TradingError::MarketDisabled => "market_disabled",
        TradingError::MarketSettled => "market_settled",
        TradingError::InvalidPrice => "invalid_price",
        TradingError::PriceTooLarge { .. } => "price_too_large",
        TradingError::TickSizeViolation { .. } => "tick_size_violation",
        TradingError::PriceBelowMinimum { .. } => "price_below_minimum",
        TradingError::PriceAboveMaximum { .. } => "price_above_maximum",
        TradingError::NoLiquidity => "no_liquidity",
        TradingError::InvalidQuantity => "invalid_quantity",
        TradingError::QuantityTooLarge { .. } => "quantity_too_large",
        TradingError::QuantityBelowMinimum { .. } => "quantity_below_minimum",
        TradingError::InvalidRemaining => "invalid_remaining",
        TradingError::InvalidAmend => "invalid_amend",
        TradingError::OrderNotFound => "order_not_found",
        TradingError::OrderNotOwned => "order_not_owned",
        TradingError::EngineUnavailable => "engine_unavailable",
        TradingError::PositionLimitExceeded { .. } => "position_limit_exceeded",
        TradingError::Overflow => "overflow",
    }
}

async fn build_book_snapshot_message(state: &AppState, market: &str) -> ServerMessage {
    let (snapshot, sequence) = state.market_book_snapshot_with_sequence(market).await;

    ServerMessage::Snapshot {
        channel: DATA_STREAM_CHANNEL.to_string(),
        market: market.to_string(),
        sequence,
        bids: snapshot.bids,
        asks: snapshot.asks,
    }
}

async fn send_server_message(
    socket: &mut WebSocket,
    message: &ServerMessage,
) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(message).expect("server message should serialize");
    socket.send(Message::Text(payload.into())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{MarketDefinition, MarketStatus};
    use crate::config::Config;
    use crate::marketdata::{BookDelta, OrderStateStatus};
    use crate::marketdata_ipc::{MarketDataRequest, MarketDataResponse};
    use crate::orderbook::{BookLevel, Order, Side};
    use crate::state::AppState;
    use chrono::{TimeZone, Utc};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;
    use tokio::sync::{mpsc as tokio_mpsc, oneshot};
    use tokio::time::timeout;
    use uuid::Uuid;

    fn test_state() -> AppState {
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
            per_user_rate_limit_burst_capacity: 500,
            per_user_rate_limit_burst_window_seconds: 10,
            admin_api_token: "test-admin-token".to_string(),
        });
        let now = Utc::now();
        state.storage.upsert_market(MarketDefinition {
            market_id: "BTC-USD".to_string(),
            display_name: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USD".to_string(),
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
        state
    }

    fn test_state_with_market_data_socket(socket_path: String) -> AppState {
        let state = AppState::new(Config {
            bind_addr: "127.0.0.1:0".to_string(),
            checkpoint_path: None,
            checkpoint_interval_seconds: 5,
            ws_broadcast_buffer: 64,
            ws_market_delta_batch_interval_ms: 10,
            ws_market_broadcast_workers: 1,
            market_data_service_socket: Some(socket_path),
            market_data_service_retry_backoff_ms: 1,
            runtime_dispatch_queue_capacity: 4_096,
            account_dispatch_queue_capacity: 4_096,
            per_user_rate_limit_burst_capacity: 500,
            per_user_rate_limit_burst_window_seconds: 10,
            admin_api_token: "test-admin-token".to_string(),
        });
        let now = Utc::now();
        state.storage.upsert_market(MarketDefinition {
            market_id: "BTC-USD".to_string(),
            display_name: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USD".to_string(),
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
        state
    }

    fn stable_order(id: u128, side: Side, price: u64, quantity: u64) -> Order {
        Order {
            id: Uuid::from_u128(id),
            trader_id: Uuid::from_u128(id + 10_000),
            market: "BTC-USD".to_string(),
            side,
            price,
            quantity,
            remaining: quantity,
            created_at: Utc.timestamp_opt(0, 0).single().expect("epoch"),
        }
    }

    fn test_connection() -> ClientConnection {
        ClientConnection {
            book_stream_id: Uuid::new_v4(),
            authenticated_user: None,
            subscription: None,
        }
    }

    #[tokio::test]
    async fn subscribe_returns_data_stream_snapshot_for_requested_market() {
        let state = test_state();
        state
            .storage
            .upsert_open_order(Uuid::from_u128(10_001), stable_order(1, Side::Buy, 100, 3));
        state
            .storage
            .upsert_open_order(Uuid::from_u128(10_002), stable_order(2, Side::Sell, 101, 2));
        state.rebuild_derived_market_data();
        let mut connection = test_connection();

        let reply = handle_client_text(
            &state,
            &mut connection,
            r#"{"op":"subscribe","channel":"data","market":"BTC-USD"}"#,
        )
        .await;

        assert_eq!(
            connection.subscription.as_ref().map(|subscription| {
                (subscription.channel.as_str(), subscription.market.as_str())
            }),
            Some(("data", "BTC-USD"))
        );
        assert_eq!(reply.len(), 1);
        match &reply[0] {
            ServerMessage::Snapshot {
                channel,
                market,
                sequence,
                bids,
                asks,
            } => {
                assert_eq!(channel, "data");
                assert_eq!(market, "BTC-USD");
                assert_eq!(*sequence, 0);
                assert_eq!(
                    *bids,
                    vec![BookLevel {
                        price: 100,
                        quantity: 3
                    }]
                );
                assert_eq!(
                    *asks,
                    vec![BookLevel {
                        price: 101,
                        quantity: 2
                    }]
                );
            }
            other => panic!("unexpected reply: {other:?}"),
        }
    }

    #[tokio::test]
    async fn l2_subscribe_keeps_sequence_continuity_when_snapshot_is_in_flight() {
        let socket_path = std::env::temp_dir().join(format!("exchange-ws-{}.sock", Uuid::new_v4()));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind market-data socket");
        let (bootstrap_tx, bootstrap_rx) = oneshot::channel();
        let (snapshot_tx, snapshot_rx) = oneshot::channel();
        let (respond_tx, respond_rx) = oneshot::channel();

        let service = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept market-data bridge");
            let (read_half, mut write_half) = stream.into_split();
            let mut lines = BufReader::new(read_half).lines();

            let bootstrap = serde_json::from_str::<MarketDataRequest>(
                &lines
                    .next_line()
                    .await
                    .expect("bootstrap line io")
                    .expect("bootstrap line"),
            )
            .expect("decode bootstrap request");
            assert!(matches!(bootstrap, MarketDataRequest::Bootstrap { .. }));
            let _ = bootstrap_tx.send(());

            let snapshot_request = serde_json::from_str::<MarketDataRequest>(
                &lines
                    .next_line()
                    .await
                    .expect("snapshot line io")
                    .expect("snapshot line"),
            )
            .expect("decode snapshot request");
            let MarketDataRequest::SnapshotRequest { request_id, market } = snapshot_request else {
                panic!("expected snapshot request");
            };
            assert_eq!(market, "BTC-USD");
            let _ = snapshot_tx.send(request_id);

            let request_id = respond_rx.await.expect("snapshot response signal");
            let payload = serde_json::to_vec(&MarketDataResponse::Snapshot {
                request_id,
                market: "BTC-USD".to_string(),
                sequence: 0,
                bids: vec![],
                asks: vec![],
            })
            .expect("snapshot json");
            write_half
                .write_all(&payload)
                .await
                .expect("write snapshot");
            write_half.write_all(b"\n").await.expect("write newline");
            write_half.flush().await.expect("flush snapshot");
        });

        let state = test_state_with_market_data_socket(
            socket_path.to_str().expect("socket path utf8").to_string(),
        );
        timeout(Duration::from_secs(1), bootstrap_rx)
            .await
            .expect("bridge did not bootstrap")
            .expect("bootstrap signal");

        let (book_tx, mut book_rx) = tokio_mpsc::unbounded_channel();
        let mut connection = ClientConnection {
            book_stream_id: state.register_book_stream(book_tx),
            authenticated_user: None,
            subscription: None,
        };

        let state_for_subscribe = state.clone();
        let subscribe = tokio::spawn(async move {
            let replies = handle_client_text(
                &state_for_subscribe,
                &mut connection,
                r#"{"op":"subscribe","channel":"data","market":"BTC-USD"}"#,
            )
            .await;
            (replies, connection)
        });

        let request_id = timeout(Duration::from_secs(1), snapshot_rx)
            .await
            .expect("snapshot request was not sent")
            .expect("snapshot request signal");

        state.dispatch_market_delta(
            "BTC-USD",
            BookDelta::Trade {
                price: 101,
                quantity: 1,
            },
        );

        assert!(
            timeout(Duration::from_millis(50), book_rx.recv())
                .await
                .is_err(),
            "snapshot bootstrap should not deliver live deltas before the snapshot reply"
        );

        respond_tx
            .send(request_id)
            .expect("respond to snapshot request");
        let (replies, connection) = subscribe.await.expect("subscribe task");
        assert_eq!(
            connection.subscription.as_ref().map(|subscription| {
                (subscription.channel.as_str(), subscription.market.as_str())
            }),
            Some(("data", "BTC-USD"))
        );
        assert_eq!(
            replies,
            vec![ServerMessage::Snapshot {
                channel: "data".to_string(),
                market: "BTC-USD".to_string(),
                sequence: 0,
                bids: vec![],
                asks: vec![],
            }]
        );

        match timeout(Duration::from_secs(1), book_rx.recv())
            .await
            .expect("timed out waiting for buffered post-snapshot delta")
            .expect("delta message")
            .as_ref()
        {
            ServerMessage::Delta {
                start_sequence,
                sequence,
                ..
            } => {
                assert_eq!(*start_sequence, 1_u64);
                assert_eq!(*sequence, 1_u64);
            }
            other => panic!("unexpected buffered post-snapshot message: {other:?}"),
        }

        state.dispatch_market_delta(
            "BTC-USD",
            BookDelta::Trade {
                price: 102,
                quantity: 1,
            },
        );

        match timeout(Duration::from_secs(1), book_rx.recv())
            .await
            .expect("timed out waiting for post-snapshot delta")
            .expect("post-snapshot message")
            .as_ref()
        {
            ServerMessage::Delta {
                start_sequence,
                sequence,
                ..
            } => {
                assert_eq!(*start_sequence, 2_u64);
                assert_eq!(*sequence, 2_u64);
            }
            other => panic!("unexpected post-snapshot message: {other:?}"),
        }

        state.unregister_book_stream(connection.book_stream_id);
        service.await.expect("market-data service task");
        let _ = std::fs::remove_file(&socket_path);
    }

    #[tokio::test]
    async fn invalid_channel_returns_error() {
        let state = test_state();
        let mut connection = test_connection();

        let reply = handle_client_text(
            &state,
            &mut connection,
            r#"{"op":"subscribe","channel":"trades","market":"BTC-USD"}"#,
        )
        .await;

        assert_eq!(
            reply,
            vec![ServerMessage::Error {
                code: "unsupported_channel".to_string(),
                message: "supported market-data channel is data".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn authenticate_message_returns_authenticated_ack() {
        let state = test_state();
        let provisioned = crate::auth::AuthService::provision_user(
            &state,
            crate::auth::ProvisionUserRequest {
                username: "ws-user".to_string(),
                team_number: None,
                role: None,
            },
        )
        .expect("provision user");
        let mut connection = test_connection();

        let reply = handle_client_text(
            &state,
            &mut connection,
            &format!(
                "{{\"op\":\"authenticate\",\"api_key\":\"{}\"}}",
                provisioned.profile.api_key
            ),
        )
        .await;

        assert_eq!(reply.len(), 1);
        match &reply[0] {
            ServerMessage::Authenticated {
                trader_id,
                team_number,
            } => {
                assert_eq!(*trader_id, provisioned.profile.trader_id);
                assert_eq!(team_number, "ws-user");
            }
            other => panic!("unexpected reply: {other:?}"),
        }
        assert_eq!(
            connection
                .authenticated_user
                .as_ref()
                .map(|user| user.username.as_str()),
            Some("ws-user")
        );
    }

    #[tokio::test]
    async fn invalid_authenticate_message_returns_error() {
        let state = test_state();
        let mut connection = test_connection();

        let reply = handle_client_text(
            &state,
            &mut connection,
            r#"{"op":"authenticate","api_key":"invalid"}"#,
        )
        .await;

        assert_eq!(
            reply,
            vec![ServerMessage::Error {
                code: "invalid_api_key".to_string(),
                message: "invalid websocket api key".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn submit_order_requires_authentication() {
        let state = test_state();
        let mut connection = test_connection();

        let reply = handle_client_text(
            &state,
            &mut connection,
            r#"{"op":"submit_order","request_id":"req-1","market":"BTC-USD","side":"BUY","price":100,"quantity":1}"#,
        )
        .await;

        assert_eq!(
            reply,
            vec![ServerMessage::Reject {
                op: "submit_order".to_string(),
                request_id: Some("req-1".to_string()),
                code: "unauthenticated".to_string(),
                message: "authenticate before trading".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn submit_order_acks_and_emits_open_state() {
        let state = test_state();
        let provisioned = crate::auth::AuthService::provision_user(
            &state,
            crate::auth::ProvisionUserRequest {
                username: "ws-trader".to_string(),
                team_number: None,
                role: None,
            },
        )
        .expect("provision user");
        let mut user_rx = state.user_events_tx.subscribe();
        let mut connection = ClientConnection {
            book_stream_id: Uuid::new_v4(),
            authenticated_user: Some(AuthenticatedUser {
                trader_id: provisioned.profile.trader_id,
                username: provisioned.profile.username.clone(),
                team_number: provisioned.profile.public_team_number().to_string(),
                role: provisioned.profile.role,
            }),
            subscription: None,
        };

        let reply = handle_client_text(
            &state,
            &mut connection,
            r#"{"op":"submit_order","request_id":"req-2","market":"BTC-USD","side":"BUY","price":100,"quantity":2}"#,
        )
        .await;

        assert_eq!(
            reply,
            vec![ServerMessage::Ack {
                op: "submit_order".to_string(),
                request_id: Some("req-2".to_string()),
            }]
        );

        let event = user_rx.recv().await.expect("user event");
        assert_eq!(event.trader_id, provisioned.profile.trader_id);
        match event.message {
            ServerMessage::OrderState { order, status } => {
                assert_eq!(status, OrderStateStatus::Open);
                assert_eq!(order.market, "BTC-USD");
                assert_eq!(order.remaining, 2);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn crossing_submit_emits_fill_and_filled_states_for_both_traders() {
        let state = test_state();
        let maker = crate::auth::AuthService::provision_user(
            &state,
            crate::auth::ProvisionUserRequest {
                username: "maker".to_string(),
                team_number: None,
                role: None,
            },
        )
        .expect("maker");
        let taker = crate::auth::AuthService::provision_user(
            &state,
            crate::auth::ProvisionUserRequest {
                username: "taker".to_string(),
                team_number: None,
                role: None,
            },
        )
        .expect("taker");

        let mut maker_connection = ClientConnection {
            book_stream_id: Uuid::new_v4(),
            authenticated_user: Some(AuthenticatedUser {
                trader_id: maker.profile.trader_id,
                username: maker.profile.username.clone(),
                team_number: maker.profile.public_team_number().to_string(),
                role: maker.profile.role,
            }),
            subscription: None,
        };
        let mut user_rx = state.user_events_tx.subscribe();

        let maker_reply = handle_client_text(
            &state,
            &mut maker_connection,
            r#"{"op":"submit_order","request_id":"maker-1","market":"BTC-USD","side":"SELL","price":100,"quantity":2}"#,
        )
        .await;
        assert_eq!(maker_reply.len(), 1);
        let _ = user_rx.recv().await.expect("maker open state");

        let mut taker_connection = ClientConnection {
            book_stream_id: Uuid::new_v4(),
            authenticated_user: Some(AuthenticatedUser {
                trader_id: taker.profile.trader_id,
                username: taker.profile.username.clone(),
                team_number: taker.profile.public_team_number().to_string(),
                role: taker.profile.role,
            }),
            subscription: None,
        };
        let taker_reply = handle_client_text(
            &state,
            &mut taker_connection,
            r#"{"op":"submit_order","request_id":"taker-1","market":"BTC-USD","side":"BUY","price":100,"quantity":2}"#,
        )
        .await;

        assert_eq!(
            taker_reply,
            vec![ServerMessage::Ack {
                op: "submit_order".to_string(),
                request_id: Some("taker-1".to_string()),
            }]
        );

        let mut saw_taker_fill = false;
        let mut saw_maker_fill = false;
        let mut saw_taker_filled = false;
        let mut saw_maker_filled = false;

        for _ in 0..4 {
            let event = user_rx.recv().await.expect("fill/state event");
            match (event.trader_id, event.message) {
                (trader_id, ServerMessage::Fill { fill })
                    if trader_id == taker.profile.trader_id =>
                {
                    saw_taker_fill = true;
                    assert_eq!(fill.quantity, 2);
                }
                (trader_id, ServerMessage::Fill { fill })
                    if trader_id == maker.profile.trader_id =>
                {
                    saw_maker_fill = true;
                    assert_eq!(fill.price, 100);
                }
                (trader_id, ServerMessage::OrderState { order, status })
                    if trader_id == taker.profile.trader_id =>
                {
                    saw_taker_filled = true;
                    assert_eq!(status, OrderStateStatus::Filled);
                    assert_eq!(order.remaining, 0);
                }
                (trader_id, ServerMessage::OrderState { order, status })
                    if trader_id == maker.profile.trader_id =>
                {
                    saw_maker_filled = true;
                    assert_eq!(status, OrderStateStatus::Filled);
                    assert_eq!(
                        order.id,
                        state.storage.list_fills(maker.profile.trader_id, None)[0].maker_order_id
                    );
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }

        assert!(saw_taker_fill);
        assert!(saw_maker_fill);
        assert!(saw_taker_filled);
        assert!(saw_maker_filled);
    }

    #[tokio::test]
    async fn cancel_and_amend_ack_and_emit_order_states() {
        let state = test_state();
        let trader = crate::auth::AuthService::provision_user(
            &state,
            crate::auth::ProvisionUserRequest {
                username: "edit-user".to_string(),
                team_number: None,
                role: None,
            },
        )
        .expect("trader");
        let mut user_rx = state.user_events_tx.subscribe();
        let mut connection = ClientConnection {
            book_stream_id: Uuid::new_v4(),
            authenticated_user: Some(AuthenticatedUser {
                trader_id: trader.profile.trader_id,
                username: trader.profile.username.clone(),
                team_number: trader.profile.public_team_number().to_string(),
                role: trader.profile.role,
            }),
            subscription: None,
        };

        let submit_reply = handle_client_text(
            &state,
            &mut connection,
            r#"{"op":"submit_order","request_id":"submit-1","market":"BTC-USD","side":"BUY","price":100,"quantity":3}"#,
        )
        .await;
        assert_eq!(submit_reply.len(), 1);
        let open_event = user_rx.recv().await.expect("open state");
        let order_id = match open_event.message {
            ServerMessage::OrderState { order, status } => {
                assert_eq!(status, OrderStateStatus::Open);
                order.id
            }
            other => panic!("unexpected open event: {other:?}"),
        };

        let amend_reply = handle_client_text(
            &state,
            &mut connection,
            &format!(
                "{{\"op\":\"amend_order\",\"request_id\":\"amend-1\",\"order_id\":\"{}\",\"remaining\":1}}",
                order_id
            ),
        )
        .await;
        assert_eq!(
            amend_reply,
            vec![ServerMessage::Ack {
                op: "amend_order".to_string(),
                request_id: Some("amend-1".to_string()),
            }]
        );
        let amended_event = user_rx.recv().await.expect("amended state");
        match amended_event.message {
            ServerMessage::OrderState { order, status } => {
                assert_eq!(status, OrderStateStatus::Open);
                assert_eq!(order.remaining, 1);
            }
            other => panic!("unexpected amend event: {other:?}"),
        }

        let cancel_reply = handle_client_text(
            &state,
            &mut connection,
            &format!(
                "{{\"op\":\"cancel_order\",\"request_id\":\"cancel-1\",\"order_id\":\"{}\"}}",
                order_id
            ),
        )
        .await;
        assert_eq!(
            cancel_reply,
            vec![ServerMessage::Ack {
                op: "cancel_order".to_string(),
                request_id: Some("cancel-1".to_string()),
            }]
        );
        let canceled_event = user_rx.recv().await.expect("canceled state");
        match canceled_event.message {
            ServerMessage::OrderState { order, status } => {
                assert_eq!(status, OrderStateStatus::Canceled);
                assert_eq!(order.id, order_id);
            }
            other => panic!("unexpected cancel event: {other:?}"),
        }
    }

    #[test]
    fn delta_messages_serialize_with_sequence_and_event() {
        let message = ServerMessage::Delta {
            channel: "data".to_string(),
            market: "BTC-USD".to_string(),
            start_sequence: 7,
            sequence: 7,
            events: vec![BookDelta::LevelUpdated {
                side: Side::Sell,
                price: 101,
                quantity: 0,
            }],
        };

        let json = serde_json::to_value(message).expect("delta json");
        assert_eq!(json["type"], "delta");
        assert_eq!(json["start_sequence"], 7);
        assert_eq!(json["sequence"], 7);
        assert_eq!(json["events"][0]["kind"], "level_updated");
    }
}
