use crate::admin::{MarketDefinition, MarketStatus};
use crate::marketdata::{
    BookDelta, BroadcastEvent, L3Order, OrderStateStatus, ServerMessage, UserBroadcastEvent,
};
use crate::matching::MatchingEngine;
use crate::orderbook::{Fill, Order, OrderBook, Side};
use crate::settlement::{SettlementEngine, SettlementError};
use crate::state::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubmitOrderRequest {
    pub market: String,
    pub side: Side,
    pub price: u64,
    pub quantity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AmendOrderRequest {
    pub remaining: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubmitOrderResponse {
    pub order: Order,
    pub fills: Vec<Fill>,
    pub resting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CancelOrderResponse {
    pub order: Order,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AmendOrderResponse {
    pub order: Order,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TradingError {
    #[error("trading is currently disabled")]
    TradingDisabled,
    #[error("invalid market symbol")]
    InvalidMarket,
    #[error("market is not configured")]
    MarketNotConfigured,
    #[error("market is disabled")]
    MarketDisabled,
    #[error("market has already been settled")]
    MarketSettled,
    #[error("price must be greater than zero")]
    InvalidPrice,
    #[error("price must align to tick size {tick_size}")]
    TickSizeViolation { tick_size: u64 },
    #[error("quantity must be greater than zero")]
    InvalidQuantity,
    #[error("quantity must be at least {minimum}")]
    QuantityBelowMinimum { minimum: u64 },
    #[error("remaining quantity must be greater than zero")]
    InvalidRemaining,
    #[error("cannot increase remaining quantity")]
    InvalidAmend,
    #[error("order not found")]
    OrderNotFound,
    #[error("order does not belong to trader")]
    OrderNotOwned,
    #[error("insufficient balance for asset {asset}")]
    InsufficientBalance { asset: String },
    #[error("numeric overflow")]
    Overflow,
}

impl From<SettlementError> for TradingError {
    fn from(value: SettlementError) -> Self {
        match value {
            SettlementError::InvalidMarket => Self::InvalidMarket,
            SettlementError::InvalidSettlementPrice => Self::InvalidPrice,
            SettlementError::InsufficientFreeBalance { asset }
            | SettlementError::InsufficientLockedBalance { asset } => {
                Self::InsufficientBalance { asset }
            }
            SettlementError::RecoveryBalanceMismatch { .. } => Self::Overflow,
            SettlementError::Overflow => Self::Overflow,
        }
    }
}

pub struct TradingService;

impl TradingService {
    pub async fn submit_limit_order(
        state: &AppState,
        trader_id: Uuid,
        request: SubmitOrderRequest,
    ) -> Result<SubmitOrderResponse, TradingError> {
        let market = validate_submit_market(state, &request)?;
        if request.price == 0 {
            return Err(TradingError::InvalidPrice);
        }
        if request.quantity == 0 {
            return Err(TradingError::InvalidQuantity);
        }

        let order = Order {
            id: Uuid::new_v4(),
            trader_id,
            market: market.market_id.clone(),
            side: request.side,
            price: request.price,
            quantity: request.quantity,
            remaining: request.quantity,
            created_at: Utc::now(),
        };

        state.storage.upsert_order_ledger(order.clone());
        SettlementEngine::lock_order(state, &order)?;

        let book_handle = market_orderbook(state, &order.market);
        let executions;
        let resting_order;
        let maker_orders;
        {
            let mut book = book_handle.lock().await;
            executions = MatchingEngine::process_limit_order_executions(&mut book, order.clone());

            let maker_ids: HashSet<Uuid> = executions
                .iter()
                .map(|execution| execution.maker_order_id)
                .collect();
            maker_orders = maker_ids
                .into_iter()
                .map(|order_id| (order_id, book.get_order(order_id).cloned()))
                .collect::<HashMap<_, _>>();
            resting_order = book.get_order(order.id).cloned();
        }

        let occurred_at = Utc::now();
        let fills = executions
            .iter()
            .map(|execution| Fill {
                fill_id: Uuid::new_v4(),
                market: order.market.clone(),
                maker_order_id: execution.maker_order_id,
                taker_order_id: order.id,
                price: execution.price,
                quantity: execution.quantity,
                occurred_at,
            })
            .collect::<Vec<_>>();

        for (execution, fill) in executions.iter().zip(fills.iter()) {
            SettlementEngine::apply_fill(
                state,
                trader_id,
                order.side,
                &order.market,
                order.price,
                execution.price,
                execution.quantity,
                fill.fill_id,
            )?;
            SettlementEngine::apply_fill(
                state,
                execution.maker_trader_id,
                execution.maker_side,
                &order.market,
                execution.maker_limit_price,
                execution.price,
                execution.quantity,
                fill.fill_id,
            )?;

            state.storage.append_fill(trader_id, fill.clone());
            state
                .storage
                .append_fill(execution.maker_trader_id, fill.clone());
            publish_user_event(state, trader_id, ServerMessage::Fill { fill: fill.clone() });
            publish_user_event(
                state,
                execution.maker_trader_id,
                ServerMessage::Fill { fill: fill.clone() },
            );

            publish_market_delta(
                state,
                &order.market,
                if let Some(resting_maker) = maker_orders
                    .get(&execution.maker_order_id)
                    .cloned()
                    .flatten()
                {
                    BookDelta::OrderUpdated {
                        order: L3Order::from(&resting_maker),
                    }
                } else {
                    BookDelta::OrderRemoved {
                        order_id: execution.maker_order_id,
                        side: execution.maker_side,
                        price: execution.maker_limit_price,
                    }
                },
            );
            publish_market_delta(
                state,
                &order.market,
                BookDelta::Trade {
                    maker_order_id: fill.maker_order_id,
                    taker_order_id: fill.taker_order_id,
                    price: fill.price,
                    quantity: fill.quantity,
                },
            );
        }

        for execution in &executions {
            let maker_resting_state = maker_orders
                .get(&execution.maker_order_id)
                .cloned()
                .flatten();
            let maker_order_state = maker_resting_state.clone().or_else(|| {
                state
                    .storage
                    .get_open_order(execution.maker_trader_id, execution.maker_order_id)
            });
            sync_open_order(
                state,
                execution.maker_trader_id,
                execution.maker_order_id,
                maker_resting_state.clone(),
                Some(0),
            );
            if let Some(mut order_state) = maker_order_state {
                let status = if maker_resting_state.is_some() {
                    OrderStateStatus::Open
                } else {
                    order_state.remaining = 0;
                    OrderStateStatus::Filled
                };
                publish_user_order_state(state, execution.maker_trader_id, order_state, status);
            }
        }
        sync_open_order(state, trader_id, order.id, resting_order.clone(), Some(0));
        if let Some(resting_order) = resting_order.as_ref() {
            publish_market_delta(
                state,
                &order.market,
                BookDelta::OrderAdded {
                    order: L3Order::from(resting_order),
                },
            );
        }
        publish_user_order_state(
            state,
            trader_id,
            resting_order.clone().unwrap_or(Order {
                remaining: 0,
                ..order.clone()
            }),
            if resting_order.is_some() {
                OrderStateStatus::Open
            } else {
                OrderStateStatus::Filled
            },
        );

        let resting = resting_order.is_some();
        Ok(SubmitOrderResponse {
            order: resting_order.unwrap_or(Order {
                remaining: 0,
                ..order
            }),
            fills,
            resting,
        })
    }

    pub async fn cancel_order(
        state: &AppState,
        trader_id: Uuid,
        order_id: Uuid,
    ) -> Result<CancelOrderResponse, TradingError> {
        let market = find_order_market(state, trader_id, order_id)?;
        let book_handle = market_orderbook(state, &market);
        let removed = {
            let mut book = book_handle.lock().await;
            let Some(order) = book.get_order(order_id).cloned() else {
                return Err(TradingError::OrderNotFound);
            };
            if order.trader_id != trader_id {
                return Err(TradingError::OrderNotOwned);
            }
            book.cancel_order(order_id)
                .expect("book order should still cancel after lookup")
        };

        SettlementEngine::release_order_hold(state, &removed)?;
        sync_open_order(state, trader_id, order_id, None, Some(removed.remaining));
        publish_user_order_state(
            state,
            trader_id,
            removed.clone(),
            OrderStateStatus::Canceled,
        );
        publish_market_delta(
            state,
            &removed.market,
            BookDelta::OrderRemoved {
                order_id: removed.id,
                side: removed.side,
                price: removed.price,
            },
        );

        Ok(CancelOrderResponse { order: removed })
    }

    pub async fn amend_order(
        state: &AppState,
        trader_id: Uuid,
        order_id: Uuid,
        request: AmendOrderRequest,
    ) -> Result<AmendOrderResponse, TradingError> {
        if request.remaining == 0 {
            return Err(TradingError::InvalidRemaining);
        }

        let market = find_order_market(state, trader_id, order_id)?;
        ensure_market_allows_entry(state, &market)?;
        let book_handle = market_orderbook(state, &market);
        let (before, after) = {
            let mut book = book_handle.lock().await;
            let Some(before) = book.get_order(order_id).cloned() else {
                return Err(TradingError::OrderNotFound);
            };
            if before.trader_id != trader_id {
                return Err(TradingError::OrderNotOwned);
            }
            if request.remaining > before.remaining {
                return Err(TradingError::InvalidAmend);
            }
            book.amend_order_remaining(order_id, request.remaining)
                .expect("amend should succeed after validation");
            let after = book
                .get_order(order_id)
                .cloned()
                .expect("order should remain after non-zero amend");
            (before, after)
        };

        if before.remaining > after.remaining {
            let mut released = before.clone();
            released.remaining = before.remaining - after.remaining;
            SettlementEngine::release_order_hold(state, &released)?;
        }
        sync_open_order(state, trader_id, order_id, Some(after.clone()), None);
        publish_user_order_state(state, trader_id, after.clone(), OrderStateStatus::Open);
        publish_market_delta(
            state,
            &after.market,
            BookDelta::OrderUpdated {
                order: L3Order::from(&after),
            },
        );

        Ok(AmendOrderResponse { order: after })
    }
}

fn validate_submit_market(
    state: &AppState,
    request: &SubmitOrderRequest,
) -> Result<MarketDefinition, TradingError> {
    let market = ensure_market_allows_entry(state, &request.market)?;
    if request.price % market.tick_size != 0 {
        return Err(TradingError::TickSizeViolation {
            tick_size: market.tick_size,
        });
    }
    if request.quantity < market.min_order_quantity {
        return Err(TradingError::QuantityBelowMinimum {
            minimum: market.min_order_quantity,
        });
    }
    Ok(market)
}

fn ensure_market_allows_entry(
    state: &AppState,
    market: &str,
) -> Result<MarketDefinition, TradingError> {
    validate_market_symbol(market)?;
    if !state.storage.get_exchange_controls().trading_enabled {
        return Err(TradingError::TradingDisabled);
    }
    let market = state
        .storage
        .get_market(market)
        .ok_or(TradingError::MarketNotConfigured)?;
    match market.status {
        MarketStatus::Enabled => Ok(market),
        MarketStatus::Disabled => Err(TradingError::MarketDisabled),
        MarketStatus::Settled => Err(TradingError::MarketSettled),
    }
}

fn validate_market_symbol(market: &str) -> Result<(), TradingError> {
    let Some((base, quote)) = market.split_once('-') else {
        return Err(TradingError::InvalidMarket);
    };
    if base.is_empty() || quote.is_empty() {
        return Err(TradingError::InvalidMarket);
    }
    Ok(())
}

fn market_orderbook(state: &AppState, market: &str) -> Arc<Mutex<OrderBook>> {
    state
        .orderbooks
        .entry(market.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(OrderBook::default())))
        .clone()
}

fn find_order_market(
    state: &AppState,
    trader_id: Uuid,
    order_id: Uuid,
) -> Result<String, TradingError> {
    state
        .storage
        .get_open_order(trader_id, order_id)
        .as_ref()
        .map(|order| order.market.clone())
        .ok_or(TradingError::OrderNotFound)
}

fn sync_open_order(
    state: &AppState,
    trader_id: Uuid,
    order_id: Uuid,
    order: Option<Order>,
    closed_remaining: Option<u64>,
) {
    if let Some(incoming) = order {
        state.storage.upsert_order_ledger(incoming.clone());
        state.storage.upsert_open_order(trader_id, incoming);
        return;
    }

    if let Some(remaining) = closed_remaining {
        state
            .storage
            .close_order_ledger(trader_id, order_id, remaining);
    }
    let _ = state.storage.delete_open_order(trader_id, order_id);
}

fn publish_market_delta(state: &AppState, market: &str, event: BookDelta) {
    let _ = state.events_tx.send(BroadcastEvent {
        market: market.to_string(),
        sequence: state.next_market_sequence(market),
        event,
    });
}

fn publish_user_event(state: &AppState, trader_id: Uuid, message: ServerMessage) {
    let _ = state
        .user_events_tx
        .send(UserBroadcastEvent { trader_id, message });
}

fn publish_user_order_state(
    state: &AppState,
    trader_id: Uuid,
    order: Order,
    status: OrderStateStatus,
) {
    publish_user_event(
        state,
        trader_id,
        ServerMessage::OrderState { order, status },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{MarketDefinition, MarketStatus};
    use crate::config::Config;
    use crate::marketdata::BookDelta;
    use crate::settlement::SettlementEngine;
    use crate::state::Balance;
    use chrono::Utc;

    fn test_state() -> AppState {
        let state = AppState::new(Config {
            bind_addr: "127.0.0.1:0".to_string(),
            database_url: "postgres://test".to_string(),
            storage_backend: crate::storage::StorageBackendKind::InMemory,
            ws_broadcast_buffer: 64,
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

    fn balance_for(state: &AppState, trader_id: Uuid, asset: &str) -> BalanceSnapshot {
        let balances = state.storage.list_balances(trader_id);
        let balance = balances
            .iter()
            .find(|balance| balance.asset == asset)
            .expect("asset balance");
        BalanceSnapshot {
            free: balance.free,
            locked: balance.locked,
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct BalanceSnapshot {
        free: u64,
        locked: u64,
    }

    #[tokio::test]
    async fn submit_resting_buy_order_locks_quote_and_tracks_open_order() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, trader_id, "USD", 1_000);

        let response = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 3,
            },
        )
        .await
        .expect("submit should succeed");

        assert!(response.resting);
        assert!(response.fills.is_empty());
        assert_eq!(
            balance_for(&state, trader_id, "USD"),
            BalanceSnapshot {
                free: 700,
                locked: 300,
            }
        );
        let orders = state.storage.list_open_orders(trader_id, None);
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].remaining, 3);
    }

    #[tokio::test]
    async fn submit_crossing_order_settles_both_traders_and_records_fills() {
        let state = test_state();
        let maker_id = Uuid::new_v4();
        let taker_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, maker_id, "BTC", 5);
        SettlementEngine::seed_balance(&state, taker_id, "USD", 1_000);

        TradingService::submit_limit_order(
            &state,
            maker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                price: 100,
                quantity: 2,
            },
        )
        .await
        .expect("maker order should rest");

        let response = TradingService::submit_limit_order(
            &state,
            taker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 105,
                quantity: 2,
            },
        )
        .await
        .expect("taker order should match");

        assert!(!response.resting);
        assert_eq!(response.fills.len(), 1);
        assert_eq!(response.fills[0].price, 100);
        assert_eq!(
            balance_for(&state, maker_id, "BTC"),
            BalanceSnapshot { free: 3, locked: 0 }
        );
        assert_eq!(
            balance_for(&state, maker_id, "USD"),
            BalanceSnapshot {
                free: 200,
                locked: 0,
            }
        );
        assert_eq!(
            balance_for(&state, taker_id, "USD"),
            BalanceSnapshot {
                free: 800,
                locked: 0,
            }
        );
        assert_eq!(
            balance_for(&state, taker_id, "BTC"),
            BalanceSnapshot { free: 2, locked: 0 }
        );
        assert_eq!(state.storage.list_open_orders(maker_id, None).len(), 0);
        assert_eq!(state.storage.list_fills(maker_id, None).len(), 1);
        assert_eq!(state.storage.list_fills(taker_id, None).len(), 1);
    }

    #[tokio::test]
    async fn recovered_open_orders_participate_in_matching_after_restart() {
        let storage = crate::storage::StorageRepository::new_in_memory();
        let maker_id = Uuid::new_v4();
        let taker_id = Uuid::new_v4();
        let maker_order = Order {
            id: Uuid::new_v4(),
            trader_id: maker_id,
            market: "BTC-USD".to_string(),
            side: Side::Sell,
            price: 100,
            quantity: 2,
            remaining: 2,
            created_at: Utc::now(),
        };

        storage.upsert_order_ledger(maker_order.clone());
        storage.upsert_open_order(maker_id, maker_order.clone());
        storage.replace_balances(
            maker_id,
            vec![
                Balance {
                    asset: "BTC".to_string(),
                    free: 0,
                    locked: 2,
                },
                Balance {
                    asset: "USD".to_string(),
                    free: 0,
                    locked: 0,
                },
            ],
        );
        let now = Utc::now();
        storage.upsert_market(MarketDefinition {
            market_id: "BTC-USD".to_string(),
            display_name: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USD".to_string(),
            tick_size: 1,
            min_order_quantity: 1,
            reference_price: None,
            settlement_price: None,
            status: MarketStatus::Enabled,
            created_at: now,
            updated_at: now,
        });

        let state = AppState::with_storage(
            Config {
                bind_addr: "127.0.0.1:0".to_string(),
                database_url: "postgres://test".to_string(),
                storage_backend: crate::storage::StorageBackendKind::InMemory,
                ws_broadcast_buffer: 64,
                per_user_requests_per_second: 100,
                admin_api_token: "test-admin-token".to_string(),
                postgres_write_batch_size: 128,
                postgres_write_flush_interval_ms: 25,
                postgres_write_queue_capacity: 4_096,
                postgres_write_retry_backoff_ms: 250,
            },
            storage,
        );
        SettlementEngine::seed_balance(&state, taker_id, "USD", 500);

        let response = TradingService::submit_limit_order(
            &state,
            taker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 2,
            },
        )
        .await
        .expect("submit should succeed");

        assert_eq!(response.fills.len(), 1);
        assert!(!response.resting);
        assert_eq!(state.storage.list_open_orders(maker_id, None).len(), 0);
        assert_eq!(
            balance_for(&state, maker_id, "USD"),
            BalanceSnapshot {
                free: 200,
                locked: 0,
            }
        );
        assert_eq!(
            balance_for(&state, taker_id, "BTC"),
            BalanceSnapshot { free: 2, locked: 0 }
        );
    }

    #[tokio::test]
    async fn cancel_releases_locked_balance() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, trader_id, "USD", 1_000);

        let response = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 3,
            },
        )
        .await
        .expect("submit should succeed");

        TradingService::cancel_order(&state, trader_id, response.order.id)
            .await
            .expect("cancel should succeed");

        assert_eq!(
            balance_for(&state, trader_id, "USD"),
            BalanceSnapshot {
                free: 1_000,
                locked: 0,
            }
        );
        assert_eq!(state.storage.list_open_orders(trader_id, None).len(), 0);
    }

    #[tokio::test]
    async fn amend_down_releases_excess_locked_balance() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, trader_id, "USD", 1_000);

        let response = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 5,
            },
        )
        .await
        .expect("submit should succeed");

        let amended = TradingService::amend_order(
            &state,
            trader_id,
            response.order.id,
            AmendOrderRequest { remaining: 2 },
        )
        .await
        .expect("amend should succeed");

        assert_eq!(amended.order.remaining, 2);
        assert_eq!(
            balance_for(&state, trader_id, "USD"),
            BalanceSnapshot {
                free: 800,
                locked: 200,
            }
        );
    }

    #[tokio::test]
    async fn submit_amend_cancel_publish_snapshot_delta_events_in_sequence() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, trader_id, "USD", 1_000);
        let mut rx = state.events_tx.subscribe();

        let submitted = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 5,
            },
        )
        .await
        .expect("submit should succeed");
        let add_event = rx.recv().await.expect("add event");
        assert_eq!(add_event.market, "BTC-USD");
        assert_eq!(add_event.sequence, 1);
        match add_event.event {
            BookDelta::OrderAdded { order } => {
                assert_eq!(order.order_id, submitted.order.id);
                assert_eq!(order.remaining, 5);
            }
            other => panic!("unexpected add event: {other:?}"),
        }

        TradingService::amend_order(
            &state,
            trader_id,
            submitted.order.id,
            AmendOrderRequest { remaining: 2 },
        )
        .await
        .expect("amend should succeed");
        let update_event = rx.recv().await.expect("update event");
        assert_eq!(update_event.sequence, 2);
        match update_event.event {
            BookDelta::OrderUpdated { order } => {
                assert_eq!(order.order_id, submitted.order.id);
                assert_eq!(order.remaining, 2);
            }
            other => panic!("unexpected update event: {other:?}"),
        }

        TradingService::cancel_order(&state, trader_id, submitted.order.id)
            .await
            .expect("cancel should succeed");
        let remove_event = rx.recv().await.expect("remove event");
        assert_eq!(remove_event.sequence, 3);
        match remove_event.event {
            BookDelta::OrderRemoved { order_id, .. } => {
                assert_eq!(order_id, submitted.order.id);
            }
            other => panic!("unexpected remove event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn full_match_publishes_order_removed_then_trade_delta() {
        let state = test_state();
        let maker_id = Uuid::new_v4();
        let taker_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, maker_id, "BTC", 5);
        SettlementEngine::seed_balance(&state, taker_id, "USD", 1_000);
        let mut rx = state.events_tx.subscribe();

        let maker_submit = TradingService::submit_limit_order(
            &state,
            maker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                price: 100,
                quantity: 2,
            },
        )
        .await
        .expect("maker submit should succeed");
        let _ = rx.recv().await.expect("maker add event");

        let taker_submit = TradingService::submit_limit_order(
            &state,
            taker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 101,
                quantity: 2,
            },
        )
        .await
        .expect("taker submit should succeed");
        assert!(!taker_submit.resting);

        let remove_event = rx.recv().await.expect("remove event");
        assert_eq!(remove_event.sequence, 2);
        match remove_event.event {
            BookDelta::OrderRemoved { order_id, .. } => {
                assert_eq!(order_id, maker_submit.order.id);
            }
            other => panic!("unexpected remove event: {other:?}"),
        }

        let trade_event = rx.recv().await.expect("trade event");
        assert_eq!(trade_event.sequence, 3);
        match trade_event.event {
            BookDelta::Trade {
                maker_order_id,
                taker_order_id,
                price,
                quantity,
            } => {
                assert_eq!(maker_order_id, maker_submit.order.id);
                assert_eq!(taker_order_id, taker_submit.order.id);
                assert_eq!(price, 100);
                assert_eq!(quantity, 2);
            }
            other => panic!("unexpected trade event: {other:?}"),
        }
    }
}
