use crate::config::Config;
use crate::marketdata::{BroadcastEvent, ServerMessage, UserBroadcastEvent};
use crate::orderbook::{Order, OrderBook};
use crate::rate_limit::PerUserRateLimiter;
use crate::settlement::SettlementEngine;
use crate::storage::StorageRepository;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct Balance {
    pub asset: String,
    pub free: u64,
    pub locked: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioSnapshot {
    pub trader_id: Uuid,
    pub balances: Vec<Balance>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub orderbooks: Arc<DashMap<String, Arc<Mutex<OrderBook>>>>,
    pub storage: StorageRepository,
    pub events_tx: broadcast::Sender<BroadcastEvent>,
    pub user_events_tx: broadcast::Sender<UserBroadcastEvent>,
    pub system_events_tx: broadcast::Sender<ServerMessage>,
    pub market_sequences: Arc<DashMap<String, u64>>,
    pub user_rate_limiter: PerUserRateLimiter,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let storage = StorageRepository::from_config(&config);
        Self::with_storage(config, storage)
    }

    pub fn with_storage(config: Config, storage: StorageRepository) -> Self {
        let (events_tx, _) = broadcast::channel(config.ws_broadcast_buffer);
        let (user_events_tx, _) = broadcast::channel(config.ws_broadcast_buffer);
        let (system_events_tx, _) = broadcast::channel(config.ws_broadcast_buffer);
        let state = Self {
            config,
            orderbooks: Arc::new(DashMap::new()),
            storage,
            events_tx,
            user_events_tx,
            system_events_tx,
            market_sequences: Arc::new(DashMap::new()),
            user_rate_limiter: PerUserRateLimiter::new(),
        };
        state.recover_runtime_state();
        state
    }

    pub fn current_market_sequence(&self, market: &str) -> u64 {
        self.market_sequences
            .get(market)
            .map(|entry| *entry.value())
            .unwrap_or(0)
    }

    pub fn next_market_sequence(&self, market: &str) -> u64 {
        let mut entry = self.market_sequences.entry(market.to_string()).or_insert(0);
        *entry += 1;
        *entry
    }

    fn recover_runtime_state(&self) {
        for market in self.storage.list_markets() {
            self.market_sequences.entry(market.market_id).or_insert(0);
        }
        let open_orders = self.storage.list_all_open_orders();
        let recovered = recover_orderbooks(open_orders.clone());
        for (market, orderbook) in recovered {
            self.orderbooks
                .insert(market.clone(), Arc::new(Mutex::new(orderbook)));
            self.market_sequences.entry(market).or_insert(0);
        }
        SettlementEngine::reconcile_balances_after_restart(self, &open_orders)
            .unwrap_or_else(|error| panic!("failed to reconcile balances after restart: {error}"));
    }
}

fn recover_orderbooks(orders: Vec<Order>) -> BTreeMap<String, OrderBook> {
    let mut orderbooks = BTreeMap::new();
    for order in orders {
        orderbooks
            .entry(order.market.clone())
            .or_insert_with(OrderBook::default)
            .add_order(order);
    }
    orderbooks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::orderbook::Side;
    use chrono::{TimeZone, Utc};

    fn test_config() -> Config {
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
        }
    }

    fn stable_order(
        id: u128,
        trader_id: u128,
        market: &str,
        side: Side,
        price: u64,
        quantity: u64,
        second: u32,
    ) -> Order {
        Order {
            id: Uuid::from_u128(id),
            trader_id: Uuid::from_u128(trader_id),
            market: market.to_string(),
            side,
            price,
            quantity,
            remaining: quantity,
            created_at: Utc
                .timestamp_opt(second as i64, 0)
                .single()
                .expect("timestamp"),
        }
    }

    #[tokio::test]
    async fn app_state_recovers_orderbooks_from_storage() {
        let storage = StorageRepository::new_in_memory();
        storage.put_balance(
            Uuid::from_u128(10),
            Balance {
                asset: "USD".to_string(),
                free: 0,
                locked: 200,
            },
        );
        storage.put_balance(
            Uuid::from_u128(20),
            Balance {
                asset: "USD".to_string(),
                free: 0,
                locked: 300,
            },
        );
        storage.put_balance(
            Uuid::from_u128(30),
            Balance {
                asset: "ETH".to_string(),
                free: 0,
                locked: 1,
            },
        );
        storage.upsert_open_order(
            Uuid::from_u128(10),
            stable_order(1, 10, "BTC-USD", Side::Buy, 100, 2, 1),
        );
        storage.upsert_open_order(
            Uuid::from_u128(20),
            stable_order(2, 20, "BTC-USD", Side::Buy, 100, 3, 2),
        );
        storage.upsert_open_order(
            Uuid::from_u128(30),
            stable_order(3, 30, "ETH-USD", Side::Sell, 200, 1, 3),
        );

        let state = AppState::with_storage(test_config(), storage);

        let btc_book = state.orderbooks.get("BTC-USD").expect("btc book").clone();
        let btc_book = btc_book.lock().await;
        let bids = btc_book.orders_for_side(Side::Buy);
        assert_eq!(bids.len(), 2);
        assert_eq!(bids[0].id, Uuid::from_u128(1));
        assert_eq!(bids[1].id, Uuid::from_u128(2));

        let eth_book = state.orderbooks.get("ETH-USD").expect("eth book").clone();
        let eth_book = eth_book.lock().await;
        let asks = eth_book.orders_for_side(Side::Sell);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].id, Uuid::from_u128(3));

        assert_eq!(state.current_market_sequence("BTC-USD"), 0);
        assert_eq!(state.current_market_sequence("ETH-USD"), 0);
    }
}
