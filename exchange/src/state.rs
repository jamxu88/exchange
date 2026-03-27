use crate::bots::BotManager;
use crate::config::Config;
use crate::derived_marketdata::{DerivedMarketDataHandle, MarketL3Snapshot};
use crate::marketdata::{
    BookDelta, BroadcastEvent, L2_CHANNEL, L3_CHANNEL, L3BroadcastEvent, MarketEvent,
    MarketEventEnvelope, ServerMessage, UserBroadcastEvent,
};
use crate::marketdata_bridge::MarketDataBridgeHandle;
use crate::marketdata_ipc::MarketBootstrapState;
use crate::orderbook::{Fill, Order, OrderBook};
use crate::rate_limit::PerUserRateLimiter;
use crate::storage::{StorageBackendKind, StorageRepository};
use crate::telemetry::{OperatorTelemetry, OperatorTelemetrySnapshot};
use crate::trading::{MarketBookSnapshot, MarketEngineHandle};
use chrono::{DateTime, Utc};
use dashmap::{DashMap, mapref::entry::Entry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc as tokio_mpsc, oneshot};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct Balance {
    pub asset: String,
    pub free: u64,
    pub locked: u64,
}

pub const NET_POSITION_LIMIT: i64 = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct Position {
    pub market: String,
    pub net_quantity: i64,
    pub average_entry_price: Option<u64>,
    pub realized_pnl: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioSnapshot {
    pub trader_id: Uuid,
    pub position_limit: Option<i64>,
    pub positions: Vec<Position>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DispatchQueueMode {
    Disabled,
    Ok,
    Backpressured,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DispatchQueueStatus {
    pub mode: DispatchQueueMode,
    pub queue_capacity: usize,
    pub backpressure_threshold: usize,
    pub queue_depth: usize,
    pub high_water_mark: usize,
    pub total_enqueued: u64,
    pub total_dequeued: u64,
    pub total_blocked_enqueues: u64,
    pub total_enqueue_block_time_ms: u64,
}

impl DispatchQueueStatus {
    fn disabled() -> Self {
        Self {
            mode: DispatchQueueMode::Disabled,
            queue_capacity: 0,
            backpressure_threshold: 0,
            queue_depth: 0,
            high_water_mark: 0,
            total_enqueued: 0,
            total_dequeued: 0,
            total_blocked_enqueues: 0,
            total_enqueue_block_time_ms: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BarrierWaitStatus {
    pub total_waits: u64,
    pub total_wait_time_ms: u64,
    pub max_wait_time_ms: u64,
    pub last_wait_time_ms: u64,
    pub waits_over_1ms: u64,
    pub waits_over_5ms: u64,
    pub waits_over_25ms: u64,
    pub waits_over_100ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AccountBarrierStatus {
    pub submit: BarrierWaitStatus,
    pub cancel: BarrierWaitStatus,
    pub amend: BarrierWaitStatus,
}

#[derive(Clone)]
pub(crate) struct AccountBarrierTelemetry {
    submit: BarrierWaitTelemetry,
    cancel: BarrierWaitTelemetry,
    amend: BarrierWaitTelemetry,
}

#[derive(Clone)]
struct DispatchQueueTelemetry {
    inner: Arc<DispatchQueueTelemetryInner>,
}

struct DispatchQueueTelemetryInner {
    queue_capacity: usize,
    backpressure_threshold: usize,
    queued_ops: AtomicUsize,
    high_water_mark: AtomicUsize,
    total_enqueued: AtomicU64,
    total_dequeued: AtomicU64,
    total_blocked_enqueues: AtomicU64,
    total_enqueue_block_time_ms: AtomicU64,
    worker_alive: AtomicBool,
}

#[derive(Clone)]
struct BarrierWaitTelemetry {
    inner: Arc<BarrierWaitTelemetryInner>,
}

struct BarrierWaitTelemetryInner {
    total_waits: AtomicU64,
    total_wait_time_ms: AtomicU64,
    max_wait_time_ms: AtomicU64,
    last_wait_time_ms: AtomicU64,
    waits_over_1ms: AtomicU64,
    waits_over_5ms: AtomicU64,
    waits_over_25ms: AtomicU64,
    waits_over_100ms: AtomicU64,
}

#[derive(Clone, Copy)]
pub(crate) enum BarrierKind {
    Submit,
    Cancel,
    Amend,
}

impl DispatchQueueMode {
    pub fn is_degraded(self) -> bool {
        matches!(self, Self::Backpressured | Self::Stopped)
    }
}

impl DispatchQueueTelemetry {
    fn new(queue_capacity: usize) -> Self {
        assert!(
            queue_capacity > 0,
            "dispatch queue capacity must be positive"
        );
        Self {
            inner: Arc::new(DispatchQueueTelemetryInner {
                queue_capacity,
                backpressure_threshold: backpressure_threshold(queue_capacity),
                queued_ops: AtomicUsize::new(0),
                high_water_mark: AtomicUsize::new(0),
                total_enqueued: AtomicU64::new(0),
                total_dequeued: AtomicU64::new(0),
                total_blocked_enqueues: AtomicU64::new(0),
                total_enqueue_block_time_ms: AtomicU64::new(0),
                worker_alive: AtomicBool::new(true),
            }),
        }
    }

    fn record_enqueue_started(&self) {
        self.inner.total_enqueued.fetch_add(1, Ordering::Relaxed);
        let queue_depth = self.inner.queued_ops.fetch_add(1, Ordering::Relaxed) + 1;
        update_max_usize(&self.inner.high_water_mark, queue_depth);
    }

    fn record_enqueue_blocked(&self, blocked_for: Duration) {
        let blocked_ms = duration_to_millis(blocked_for);
        if blocked_ms > 0 {
            self.inner
                .total_blocked_enqueues
                .fetch_add(1, Ordering::Relaxed);
            self.inner
                .total_enqueue_block_time_ms
                .fetch_add(blocked_ms, Ordering::Relaxed);
        }
    }

    fn record_dequeued(&self) {
        self.inner.total_dequeued.fetch_add(1, Ordering::Relaxed);
        self.inner.queued_ops.fetch_sub(1, Ordering::Relaxed);
    }

    fn mark_stopped(&self) {
        self.inner.worker_alive.store(false, Ordering::Relaxed);
    }

    fn snapshot(&self) -> DispatchQueueStatus {
        let queue_depth = self.inner.queued_ops.load(Ordering::Relaxed);
        let mode = if !self.inner.worker_alive.load(Ordering::Relaxed) {
            DispatchQueueMode::Stopped
        } else if queue_depth >= self.inner.backpressure_threshold && queue_depth > 0 {
            DispatchQueueMode::Backpressured
        } else {
            DispatchQueueMode::Ok
        };

        DispatchQueueStatus {
            mode,
            queue_capacity: self.inner.queue_capacity,
            backpressure_threshold: self.inner.backpressure_threshold,
            queue_depth,
            high_water_mark: self.inner.high_water_mark.load(Ordering::Relaxed),
            total_enqueued: self.inner.total_enqueued.load(Ordering::Relaxed),
            total_dequeued: self.inner.total_dequeued.load(Ordering::Relaxed),
            total_blocked_enqueues: self.inner.total_blocked_enqueues.load(Ordering::Relaxed),
            total_enqueue_block_time_ms: self
                .inner
                .total_enqueue_block_time_ms
                .load(Ordering::Relaxed),
        }
    }
}

impl BarrierWaitTelemetry {
    fn new() -> Self {
        Self {
            inner: Arc::new(BarrierWaitTelemetryInner {
                total_waits: AtomicU64::new(0),
                total_wait_time_ms: AtomicU64::new(0),
                max_wait_time_ms: AtomicU64::new(0),
                last_wait_time_ms: AtomicU64::new(0),
                waits_over_1ms: AtomicU64::new(0),
                waits_over_5ms: AtomicU64::new(0),
                waits_over_25ms: AtomicU64::new(0),
                waits_over_100ms: AtomicU64::new(0),
            }),
        }
    }

    fn record(&self, wait: Duration) {
        let wait_ms = duration_to_millis(wait);
        self.inner.total_waits.fetch_add(1, Ordering::Relaxed);
        self.inner
            .total_wait_time_ms
            .fetch_add(wait_ms, Ordering::Relaxed);
        self.inner
            .last_wait_time_ms
            .store(wait_ms, Ordering::Relaxed);
        update_max_u64(&self.inner.max_wait_time_ms, wait_ms);
        if wait_ms >= 1 {
            self.inner.waits_over_1ms.fetch_add(1, Ordering::Relaxed);
        }
        if wait_ms >= 5 {
            self.inner.waits_over_5ms.fetch_add(1, Ordering::Relaxed);
        }
        if wait_ms >= 25 {
            self.inner.waits_over_25ms.fetch_add(1, Ordering::Relaxed);
        }
        if wait_ms >= 100 {
            self.inner.waits_over_100ms.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn snapshot(&self) -> BarrierWaitStatus {
        BarrierWaitStatus {
            total_waits: self.inner.total_waits.load(Ordering::Relaxed),
            total_wait_time_ms: self.inner.total_wait_time_ms.load(Ordering::Relaxed),
            max_wait_time_ms: self.inner.max_wait_time_ms.load(Ordering::Relaxed),
            last_wait_time_ms: self.inner.last_wait_time_ms.load(Ordering::Relaxed),
            waits_over_1ms: self.inner.waits_over_1ms.load(Ordering::Relaxed),
            waits_over_5ms: self.inner.waits_over_5ms.load(Ordering::Relaxed),
            waits_over_25ms: self.inner.waits_over_25ms.load(Ordering::Relaxed),
            waits_over_100ms: self.inner.waits_over_100ms.load(Ordering::Relaxed),
        }
    }
}

impl Default for AccountBarrierTelemetry {
    fn default() -> Self {
        Self {
            submit: BarrierWaitTelemetry::new(),
            cancel: BarrierWaitTelemetry::new(),
            amend: BarrierWaitTelemetry::new(),
        }
    }
}

impl AccountBarrierTelemetry {
    pub(crate) fn record(&self, barrier: BarrierKind, wait: Duration) {
        match barrier {
            BarrierKind::Submit => self.submit.record(wait),
            BarrierKind::Cancel => self.cancel.record(wait),
            BarrierKind::Amend => self.amend.record(wait),
        }
    }

    fn snapshot(&self) -> AccountBarrierStatus {
        AccountBarrierStatus {
            submit: self.submit.snapshot(),
            cancel: self.cancel.snapshot(),
            amend: self.amend.snapshot(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub market_engines: Arc<DashMap<String, MarketEngineHandle>>,
    pub storage: StorageRepository,
    derived_market_data: DerivedMarketDataHandle,
    market_data_bridge: Arc<OnceLock<MarketDataBridgeHandle>>,
    pub bot_manager: BotManager,
    runtime_dispatcher: RuntimeDispatchHandle,
    account_dispatcher: AccountDispatchHandle,
    account_barrier_telemetry: AccountBarrierTelemetry,
    persistence_dispatcher: PersistenceDispatchHandle,
    market_broadcaster: MarketBroadcastHandle,
    market_event_broadcaster: MarketEventBroadcastHandle,
    pub events_tx: broadcast::Sender<BroadcastEvent>,
    pub market_event_tx: broadcast::Sender<MarketEventEnvelope>,
    pub public_events_tx: broadcast::Sender<ServerMessage>,
    pub user_events_tx: broadcast::Sender<UserBroadcastEvent>,
    pub system_events_tx: broadcast::Sender<ServerMessage>,
    pub market_sequences: Arc<DashMap<String, u64>>,
    pub market_event_sequences: Arc<DashMap<String, u64>>,
    pub user_rate_limiter: PerUserRateLimiter,
    operator_telemetry: OperatorTelemetry,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let storage = StorageRepository::from_config(&config);
        Self::with_storage(config, storage)
    }

    pub fn with_storage(config: Config, storage: StorageRepository) -> Self {
        let (events_tx, _) = broadcast::channel(config.ws_broadcast_buffer);
        let (market_event_tx, _) = broadcast::channel(config.ws_broadcast_buffer);
        let (public_events_tx, _) = broadcast::channel(config.ws_broadcast_buffer);
        let (user_events_tx, _) = broadcast::channel(config.ws_broadcast_buffer);
        let (system_events_tx, _) = broadcast::channel(config.ws_broadcast_buffer);
        let operator_telemetry = OperatorTelemetry::default();
        let market_broadcaster = MarketBroadcastHandle::spawn(
            config.ws_market_broadcast_workers,
            config.runtime_dispatch_queue_capacity,
            operator_telemetry.clone(),
        );
        let market_event_broadcaster = MarketEventBroadcastHandle::spawn(
            config.ws_market_broadcast_workers,
            config.runtime_dispatch_queue_capacity,
            operator_telemetry.clone(),
        );
        let runtime_dispatcher = RuntimeDispatchHandle::spawn(
            config.runtime_dispatch_queue_capacity,
            config.ws_market_delta_batch_interval_ms,
            events_tx.clone(),
            market_event_tx.clone(),
            market_broadcaster.clone(),
            market_event_broadcaster.clone(),
            public_events_tx.clone(),
            user_events_tx.clone(),
            system_events_tx.clone(),
        );
        let account_dispatcher =
            AccountDispatchHandle::spawn(storage.clone(), config.account_dispatch_queue_capacity);
        let account_barrier_telemetry = AccountBarrierTelemetry::default();
        let persistence_dispatcher = PersistenceDispatchHandle::spawn(
            storage.clone(),
            config.persistence_dispatch_queue_capacity,
        );
        let state = Self {
            config,
            market_engines: Arc::new(DashMap::new()),
            storage,
            derived_market_data: DerivedMarketDataHandle::default(),
            market_data_bridge: Arc::new(OnceLock::new()),
            bot_manager: BotManager::default(),
            runtime_dispatcher,
            account_dispatcher,
            account_barrier_telemetry,
            persistence_dispatcher,
            market_broadcaster,
            market_event_broadcaster,
            events_tx,
            market_event_tx,
            public_events_tx,
            user_events_tx,
            system_events_tx,
            market_sequences: Arc::new(DashMap::new()),
            market_event_sequences: Arc::new(DashMap::new()),
            user_rate_limiter: PerUserRateLimiter::new(),
            operator_telemetry,
        };
        state.recover_runtime_state();
        state.rebuild_derived_market_data();
        state.start_market_data_bridge();
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

    pub fn current_market_event_sequence(&self, market: &str) -> u64 {
        self.market_event_sequences
            .get(market)
            .map(|entry| *entry.value())
            .unwrap_or(0)
    }

    pub fn next_market_event_sequence(&self, market: &str) -> u64 {
        let mut entry = self
            .market_event_sequences
            .entry(market.to_string())
            .or_insert(0);
        *entry += 1;
        *entry
    }

    pub fn dispatch_market_delta(&self, market: &str, event: BookDelta) {
        let sequence = self.next_market_sequence(market);
        self.runtime_dispatcher.dispatch_market(BroadcastEvent {
            market: market.to_string(),
            start_sequence: sequence,
            sequence,
            events: vec![event],
        });
    }

    pub fn dispatch_market_event(&self, market: &str, event: MarketEvent) {
        let envelope = MarketEventEnvelope {
            market: market.to_string(),
            sequence: self.next_market_event_sequence(market),
            recorded_at: Utc::now(),
            event,
        };
        let market = envelope.market.clone();
        let local_deltas = self
            .derived_market_data
            .apply_market_event(envelope.clone());
        let bridged = self
            .market_data_bridge()
            .is_some_and(|bridge| bridge.publish_event(envelope.clone()));
        if !bridged {
            for delta in local_deltas {
                self.dispatch_market_delta(&market, delta);
            }
        }
        self.runtime_dispatcher.dispatch_market_event(envelope);
    }

    pub fn register_book_stream(
        &self,
        tx: tokio_mpsc::UnboundedSender<Arc<ServerMessage>>,
    ) -> Uuid {
        self.market_broadcaster.register(tx)
    }

    pub fn update_book_stream_subscription(
        &self,
        client_id: Uuid,
        market: Option<String>,
        last_sequence: Option<u64>,
    ) {
        self.market_broadcaster
            .update_subscription(client_id, market, last_sequence);
    }

    pub fn unregister_book_stream(&self, client_id: Uuid) {
        self.market_broadcaster.unregister(client_id);
    }

    pub fn register_l3_stream(&self, tx: tokio_mpsc::UnboundedSender<Arc<ServerMessage>>) -> Uuid {
        self.market_event_broadcaster.register(tx)
    }

    pub fn update_l3_stream_subscription(
        &self,
        client_id: Uuid,
        market: Option<String>,
        last_sequence: Option<u64>,
    ) {
        self.market_event_broadcaster
            .update_subscription(client_id, market, last_sequence);
    }

    pub fn unregister_l3_stream(&self, client_id: Uuid) {
        self.market_event_broadcaster.unregister(client_id);
    }

    pub fn dispatch_user_event(&self, trader_id: Uuid, message: ServerMessage) {
        self.runtime_dispatcher
            .dispatch_user(UserBroadcastEvent { trader_id, message });
    }

    pub fn dispatch_public_message(&self, message: ServerMessage) {
        self.runtime_dispatcher.dispatch_public(message);
    }

    pub fn dispatch_system_message(&self, message: ServerMessage) {
        self.runtime_dispatcher.dispatch_system(message);
    }

    pub fn persist_order_ledger(&self, order: Order) {
        self.persistence_dispatcher.upsert_order_ledger(order);
    }

    pub fn close_order_ledger(&self, trader_id: Uuid, order_id: Uuid, remaining: u64) {
        self.persistence_dispatcher
            .close_order_ledger(trader_id, order_id, remaining);
    }

    pub fn persist_fill(&self, fill: Fill) {
        self.persistence_dispatcher.append_fill(fill);
    }

    pub fn queue_upsert_position(&self, trader_id: Uuid, position: Position) {
        self.account_dispatcher.upsert_position(trader_id, position);
    }

    pub fn queue_delete_position(&self, trader_id: Uuid, market: String) {
        self.account_dispatcher.delete_position(trader_id, market);
    }

    pub fn queue_upsert_open_order(&self, trader_id: Uuid, order: Order) {
        self.account_dispatcher.upsert_open_order(trader_id, order);
    }

    pub fn queue_delete_open_order(&self, trader_id: Uuid, order_id: Uuid) {
        self.account_dispatcher
            .delete_open_order(trader_id, order_id);
    }

    pub fn queue_append_fill(&self, trader_id: Uuid, fill: Fill) {
        self.account_dispatcher.append_fill(trader_id, fill);
    }

    pub fn account_dispatch_barrier(&self) -> oneshot::Receiver<()> {
        self.account_dispatcher.barrier()
    }

    pub fn runtime_dispatch_status(&self) -> DispatchQueueStatus {
        self.runtime_dispatcher.status()
    }

    pub fn account_dispatch_status(&self) -> DispatchQueueStatus {
        self.account_dispatcher.status()
    }

    pub fn persistence_dispatch_status(&self) -> DispatchQueueStatus {
        self.persistence_dispatcher.status()
    }

    pub fn account_barrier_status(&self) -> AccountBarrierStatus {
        self.account_barrier_telemetry.snapshot()
    }

    pub fn operator_telemetry_snapshot(&self) -> OperatorTelemetrySnapshot {
        self.operator_telemetry.snapshot()
    }

    pub(crate) fn operator_telemetry(&self) -> &OperatorTelemetry {
        &self.operator_telemetry
    }

    pub(crate) fn account_barrier_telemetry(&self) -> AccountBarrierTelemetry {
        self.account_barrier_telemetry.clone()
    }

    pub fn ensure_market_engine(&self, market: &str) -> MarketEngineHandle {
        if let Some(engine) = self.market_engines.get(market) {
            return engine.clone();
        }

        let handle = MarketEngineHandle::spawn(
            self.clone(),
            market.to_string(),
            recover_orderbook(self.storage.list_all_open_orders(), market),
        );

        match self.market_engines.entry(market.to_string()) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
                entry.insert(handle.clone());
                handle
            }
        }
    }

    pub fn remove_market_runtime(&self, market: &str) {
        self.market_engines.remove(market);
        self.market_sequences.remove(market);
        self.market_event_sequences.remove(market);
        self.derived_market_data.clear_market(market);
    }

    pub fn clear_market_runtime(&self) {
        self.market_engines.clear();
        self.derived_market_data.clear_all();
    }

    pub async fn market_book_snapshot(&self, market: &str) -> MarketBookSnapshot {
        self.market_book_snapshot_with_sequence(market).await.0
    }

    pub async fn market_book_snapshot_with_sequence(
        &self,
        market: &str,
    ) -> (MarketBookSnapshot, u64) {
        let market_is_settled = self
            .storage
            .get_market(market)
            .is_some_and(|definition| definition.status == crate::admin::MarketStatus::Settled);
        if !market_is_settled {
            if let Some(bridge) = self.market_data_bridge() {
                if let Some(snapshot) = bridge.request_snapshot(market).await {
                    return (
                        MarketBookSnapshot {
                            bids: snapshot.bids,
                            asks: snapshot.asks,
                        },
                        snapshot.sequence,
                    );
                }
            }
        }
        (
            self.derived_market_data.book_snapshot(market),
            self.current_market_sequence(market),
        )
    }

    pub async fn market_best_prices(&self, market: &str) -> (Option<u64>, Option<u64>) {
        self.derived_market_data.best_prices(market)
    }

    #[allow(dead_code)]
    pub(crate) fn market_l3_snapshot(&self, market: &str) -> MarketL3Snapshot {
        self.derived_market_data.l3_snapshot(market)
    }

    pub(crate) fn rebuild_derived_market_data(&self) {
        let market_sequences = self
            .storage
            .list_markets()
            .into_iter()
            .map(|market| {
                let market_id = market.market_id;
                let sequence = self.current_market_event_sequence(&market_id);
                (market_id, sequence)
            })
            .collect();
        self.derived_market_data
            .replace_from_open_orders(market_sequences, self.storage.list_all_open_orders());
    }

    pub(crate) fn market_bootstrap_state(&self) -> Vec<MarketBootstrapState> {
        self.storage
            .list_markets()
            .into_iter()
            .map(|market| {
                let market_id = market.market_id;
                MarketBootstrapState {
                    book_sequence: self.current_market_sequence(&market_id),
                    event_sequence: self.current_market_event_sequence(&market_id),
                    market: market_id,
                }
            })
            .collect()
    }

    pub(crate) fn apply_bridge_market_batch(&self, batch: BroadcastEvent) {
        self.market_sequences
            .insert(batch.market.clone(), batch.sequence);
        self.runtime_dispatcher.dispatch_market(batch);
    }

    fn start_market_data_bridge(&self) {
        if self.config.market_data_service_socket.is_none() {
            return;
        }

        let _ = self
            .market_data_bridge
            .set(MarketDataBridgeHandle::spawn(self.clone()));
    }

    fn market_data_bridge(&self) -> Option<MarketDataBridgeHandle> {
        self.market_data_bridge.get().cloned()
    }

    fn recover_runtime_state(&self) {
        for market in self.storage.list_markets() {
            let market_id = market.market_id;
            self.market_sequences.entry(market_id.clone()).or_insert(0);
            self.market_event_sequences.entry(market_id).or_insert(0);
        }
        let open_orders = self.storage.list_all_open_orders();
        let recovered = recover_orderbooks(open_orders.clone());
        for (market, orderbook) in recovered {
            self.market_engines.insert(
                market.clone(),
                MarketEngineHandle::spawn(self.clone(), market.clone(), orderbook),
            );
            self.market_sequences.entry(market.clone()).or_insert(0);
            self.market_event_sequences.entry(market).or_insert(0);
        }
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

fn recover_orderbook(orders: Vec<Order>, market: &str) -> OrderBook {
    let mut orderbook = OrderBook::default();
    for order in orders {
        if order.market == market {
            orderbook.add_order(order);
        }
    }
    orderbook
}

#[derive(Clone)]
struct MarketBroadcastHandle {
    workers: Arc<Vec<MarketBroadcastWorker>>,
    worker_index_by_client: Arc<DashMap<Uuid, usize>>,
    next_worker: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct MarketBroadcastWorker {
    tx: mpsc::SyncSender<MarketBroadcastCommand>,
}

enum MarketBroadcastCommand {
    Register {
        client_id: Uuid,
        tx: tokio_mpsc::UnboundedSender<Arc<ServerMessage>>,
    },
    UpdateSubscription {
        client_id: Uuid,
        market: Option<String>,
        last_sequence: Option<u64>,
    },
    Publish(BroadcastEvent),
    Remove {
        client_id: Uuid,
    },
}

struct MarketBroadcastClient {
    tx: tokio_mpsc::UnboundedSender<Arc<ServerMessage>>,
    subscription: Option<String>,
    last_market_sequence: Option<u64>,
}

impl MarketBroadcastHandle {
    fn spawn(worker_count: usize, queue_capacity: usize, telemetry: OperatorTelemetry) -> Self {
        let worker_count = worker_count.max(1);
        let workers = (0..worker_count)
            .map(|index| MarketBroadcastWorker::spawn(index, queue_capacity, telemetry.clone()))
            .collect();

        Self {
            workers: Arc::new(workers),
            worker_index_by_client: Arc::new(DashMap::new()),
            next_worker: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn register(&self, tx: tokio_mpsc::UnboundedSender<Arc<ServerMessage>>) -> Uuid {
        let client_id = Uuid::new_v4();
        let worker_index = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        self.worker_index_by_client.insert(client_id, worker_index);
        self.workers[worker_index].send(MarketBroadcastCommand::Register { client_id, tx });
        client_id
    }

    fn update_subscription(
        &self,
        client_id: Uuid,
        market: Option<String>,
        last_sequence: Option<u64>,
    ) {
        let Some(worker_index) = self
            .worker_index_by_client
            .get(&client_id)
            .map(|entry| *entry.value())
        else {
            return;
        };

        self.workers[worker_index].send(MarketBroadcastCommand::UpdateSubscription {
            client_id,
            market,
            last_sequence,
        });
    }

    fn publish(&self, batch: BroadcastEvent) {
        for worker in self.workers.iter() {
            worker.send(MarketBroadcastCommand::Publish(batch.clone()));
        }
    }

    fn unregister(&self, client_id: Uuid) {
        let Some((_, worker_index)) = self.worker_index_by_client.remove(&client_id) else {
            return;
        };

        self.workers[worker_index].send(MarketBroadcastCommand::Remove { client_id });
    }
}

impl MarketBroadcastWorker {
    fn spawn(index: usize, queue_capacity: usize, telemetry: OperatorTelemetry) -> Self {
        let (tx, rx) = mpsc::sync_channel(queue_capacity);
        thread::Builder::new()
            .name(format!("exchange-market-broadcast-{index}"))
            .spawn(move || {
                let mut clients = HashMap::<Uuid, MarketBroadcastClient>::new();
                while let Ok(command) = rx.recv() {
                    match command {
                        MarketBroadcastCommand::Register { client_id, tx } => {
                            clients.insert(
                                client_id,
                                MarketBroadcastClient {
                                    tx,
                                    subscription: None,
                                    last_market_sequence: None,
                                },
                            );
                        }
                        MarketBroadcastCommand::UpdateSubscription {
                            client_id,
                            market,
                            last_sequence,
                        } => {
                            if let Some(client) = clients.get_mut(&client_id) {
                                client.subscription = market;
                                client.last_market_sequence = last_sequence;
                            }
                        }
                        MarketBroadcastCommand::Publish(batch) => {
                            publish_market_batch(&mut clients, batch, &telemetry);
                        }
                        MarketBroadcastCommand::Remove { client_id } => {
                            clients.remove(&client_id);
                        }
                    }
                }
            })
            .unwrap_or_else(|error| panic!("failed to spawn market broadcast thread: {error}"));
        Self { tx }
    }

    fn send(&self, command: MarketBroadcastCommand) {
        self.tx
            .send(command)
            .unwrap_or_else(|_| panic!("market broadcast thread terminated"));
    }
}

#[derive(Clone)]
struct MarketEventBroadcastHandle {
    workers: Arc<Vec<MarketEventBroadcastWorker>>,
    worker_index_by_client: Arc<DashMap<Uuid, usize>>,
    next_worker: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct MarketEventBroadcastWorker {
    tx: mpsc::SyncSender<MarketEventBroadcastCommand>,
}

enum MarketEventBroadcastCommand {
    Register {
        client_id: Uuid,
        tx: tokio_mpsc::UnboundedSender<Arc<ServerMessage>>,
    },
    UpdateSubscription {
        client_id: Uuid,
        market: Option<String>,
        last_sequence: Option<u64>,
    },
    Publish(L3BroadcastEvent),
    Remove {
        client_id: Uuid,
    },
}

struct MarketEventBroadcastClient {
    tx: tokio_mpsc::UnboundedSender<Arc<ServerMessage>>,
    subscription: Option<String>,
    last_market_sequence: Option<u64>,
}

impl MarketEventBroadcastHandle {
    fn spawn(worker_count: usize, queue_capacity: usize, telemetry: OperatorTelemetry) -> Self {
        let worker_count = worker_count.max(1);
        let workers = (0..worker_count)
            .map(|index| {
                MarketEventBroadcastWorker::spawn(index, queue_capacity, telemetry.clone())
            })
            .collect();

        Self {
            workers: Arc::new(workers),
            worker_index_by_client: Arc::new(DashMap::new()),
            next_worker: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn register(&self, tx: tokio_mpsc::UnboundedSender<Arc<ServerMessage>>) -> Uuid {
        let client_id = Uuid::new_v4();
        let worker_index = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        self.worker_index_by_client.insert(client_id, worker_index);
        self.workers[worker_index].send(MarketEventBroadcastCommand::Register { client_id, tx });
        client_id
    }

    fn update_subscription(
        &self,
        client_id: Uuid,
        market: Option<String>,
        last_sequence: Option<u64>,
    ) {
        let Some(worker_index) = self
            .worker_index_by_client
            .get(&client_id)
            .map(|entry| *entry.value())
        else {
            return;
        };

        self.workers[worker_index].send(MarketEventBroadcastCommand::UpdateSubscription {
            client_id,
            market,
            last_sequence,
        });
    }

    fn publish(&self, batch: L3BroadcastEvent) {
        for worker in self.workers.iter() {
            worker.send(MarketEventBroadcastCommand::Publish(batch.clone()));
        }
    }

    fn unregister(&self, client_id: Uuid) {
        let Some((_, worker_index)) = self.worker_index_by_client.remove(&client_id) else {
            return;
        };

        self.workers[worker_index].send(MarketEventBroadcastCommand::Remove { client_id });
    }
}

impl MarketEventBroadcastWorker {
    fn spawn(index: usize, queue_capacity: usize, telemetry: OperatorTelemetry) -> Self {
        let (tx, rx) = mpsc::sync_channel(queue_capacity);
        thread::Builder::new()
            .name(format!("exchange-l3-broadcast-{index}"))
            .spawn(move || {
                let mut clients = HashMap::<Uuid, MarketEventBroadcastClient>::new();
                while let Ok(command) = rx.recv() {
                    match command {
                        MarketEventBroadcastCommand::Register { client_id, tx } => {
                            clients.insert(
                                client_id,
                                MarketEventBroadcastClient {
                                    tx,
                                    subscription: None,
                                    last_market_sequence: None,
                                },
                            );
                        }
                        MarketEventBroadcastCommand::UpdateSubscription {
                            client_id,
                            market,
                            last_sequence,
                        } => {
                            if let Some(client) = clients.get_mut(&client_id) {
                                client.subscription = market;
                                client.last_market_sequence = last_sequence;
                            }
                        }
                        MarketEventBroadcastCommand::Publish(batch) => {
                            publish_market_event_batch(&mut clients, batch, &telemetry);
                        }
                        MarketEventBroadcastCommand::Remove { client_id } => {
                            clients.remove(&client_id);
                        }
                    }
                }
            })
            .unwrap_or_else(|error| panic!("failed to spawn l3 broadcast thread: {error}"));
        Self { tx }
    }

    fn send(&self, command: MarketEventBroadcastCommand) {
        self.tx
            .send(command)
            .unwrap_or_else(|_| panic!("l3 broadcast thread terminated"));
    }
}

fn publish_market_batch(
    clients: &mut HashMap<Uuid, MarketBroadcastClient>,
    batch: BroadcastEvent,
    telemetry: &OperatorTelemetry,
) {
    let delta_message = Arc::new(ServerMessage::Delta {
        channel: L2_CHANNEL.to_string(),
        market: batch.market.clone(),
        start_sequence: batch.start_sequence,
        sequence: batch.sequence,
        events: batch.events.clone(),
    });
    let mut disconnected = Vec::new();

    for (client_id, client) in clients.iter_mut() {
        if client.subscription.as_deref() != Some(batch.market.as_str()) {
            continue;
        }

        if let Some(last_sequence) = client.last_market_sequence {
            let expected_sequence = last_sequence.saturating_add(1);
            if batch.start_sequence != expected_sequence {
                telemetry.record_l2_resync();
                let resync_message = Arc::new(ServerMessage::ResyncRequired {
                    channel: L2_CHANNEL.to_string(),
                    market: Some(batch.market.clone()),
                    expected_sequence: Some(expected_sequence),
                    current_sequence: Some(batch.sequence),
                    reason: "market sequence gap detected; resubscribe for a fresh snapshot"
                        .to_string(),
                });
                client.subscription = None;
                client.last_market_sequence = None;
                if client.tx.send(resync_message).is_err() {
                    disconnected.push(*client_id);
                }
                continue;
            }
        }

        client.last_market_sequence = Some(batch.sequence);
        if client.tx.send(delta_message.clone()).is_err() {
            disconnected.push(*client_id);
        }
    }

    for client_id in disconnected {
        clients.remove(&client_id);
    }
}

fn publish_market_event_batch(
    clients: &mut HashMap<Uuid, MarketEventBroadcastClient>,
    batch: L3BroadcastEvent,
    telemetry: &OperatorTelemetry,
) {
    let delta_message = Arc::new(ServerMessage::L3Delta {
        channel: L3_CHANNEL.to_string(),
        market: batch.market.clone(),
        start_sequence: batch.start_sequence,
        sequence: batch.sequence,
        events: batch.events.clone(),
    });
    let mut disconnected = Vec::new();

    for (client_id, client) in clients.iter_mut() {
        if client.subscription.as_deref() != Some(batch.market.as_str()) {
            continue;
        }

        if let Some(last_sequence) = client.last_market_sequence {
            let expected_sequence = last_sequence.saturating_add(1);
            if batch.start_sequence != expected_sequence {
                telemetry.record_l3_resync();
                let resync_message = Arc::new(ServerMessage::ResyncRequired {
                    channel: L3_CHANNEL.to_string(),
                    market: Some(batch.market.clone()),
                    expected_sequence: Some(expected_sequence),
                    current_sequence: Some(batch.sequence),
                    reason: "market sequence gap detected; resubscribe for a fresh snapshot"
                        .to_string(),
                });
                client.subscription = None;
                client.last_market_sequence = None;
                if client.tx.send(resync_message).is_err() {
                    disconnected.push(*client_id);
                }
                continue;
            }
        }

        client.last_market_sequence = Some(batch.sequence);
        if client.tx.send(delta_message.clone()).is_err() {
            disconnected.push(*client_id);
        }
    }

    for client_id in disconnected {
        clients.remove(&client_id);
    }
}

fn merge_market_batch(
    pending_by_market: &mut BTreeMap<String, BroadcastEvent>,
    next: BroadcastEvent,
) -> Option<BroadcastEvent> {
    if let Some(pending) = pending_by_market.get_mut(&next.market) {
        if pending.sequence.saturating_add(1) == next.start_sequence {
            pending.sequence = next.sequence;
            pending.events.extend(next.events);
            return None;
        }
    }

    pending_by_market.insert(next.market.clone(), next)
}

fn publish_market_batch_to_subscribers(
    batch: BroadcastEvent,
    events_tx: &broadcast::Sender<BroadcastEvent>,
    market_broadcaster: &MarketBroadcastHandle,
) {
    let _ = events_tx.send(batch.clone());
    market_broadcaster.publish(batch);
}

fn flush_market_batches(
    pending_by_market: &mut BTreeMap<String, BroadcastEvent>,
    events_tx: &broadcast::Sender<BroadcastEvent>,
    market_broadcaster: &MarketBroadcastHandle,
) {
    for (_, batch) in std::mem::take(pending_by_market) {
        publish_market_batch_to_subscribers(batch, events_tx, market_broadcaster);
    }
}

#[derive(Clone)]
struct RuntimeDispatchHandle {
    tx: mpsc::SyncSender<RuntimeDispatch>,
    telemetry: DispatchQueueTelemetry,
}

enum RuntimeDispatch {
    Market(BroadcastEvent),
    MarketEvent(MarketEventEnvelope),
    Public(ServerMessage),
    User(UserBroadcastEvent),
    System(ServerMessage),
}

impl RuntimeDispatchHandle {
    fn spawn(
        queue_capacity: usize,
        market_batch_interval_ms: u64,
        events_tx: broadcast::Sender<BroadcastEvent>,
        market_event_tx: broadcast::Sender<MarketEventEnvelope>,
        market_broadcaster: MarketBroadcastHandle,
        market_event_broadcaster: MarketEventBroadcastHandle,
        public_events_tx: broadcast::Sender<ServerMessage>,
        user_events_tx: broadcast::Sender<UserBroadcastEvent>,
        system_events_tx: broadcast::Sender<ServerMessage>,
    ) -> Self {
        let telemetry = DispatchQueueTelemetry::new(queue_capacity);
        let worker_telemetry = telemetry.clone();
        let (tx, rx) = mpsc::sync_channel(queue_capacity);
        let market_batch_interval = Duration::from_millis(market_batch_interval_ms.max(1));
        thread::Builder::new()
            .name("exchange-runtime-dispatcher".to_string())
            .spawn(move || {
                let mut pending_market_batches = BTreeMap::<String, BroadcastEvent>::new();
                let mut next_market_flush_at: Option<Instant> = None;

                loop {
                    if next_market_flush_at.is_some_and(|deadline| Instant::now() >= deadline) {
                        flush_market_batches(
                            &mut pending_market_batches,
                            &events_tx,
                            &market_broadcaster,
                        );
                        next_market_flush_at = None;
                    }

                    let next_event = match next_market_flush_at {
                        Some(deadline) => {
                            let wait_for = deadline.saturating_duration_since(Instant::now());
                            rx.recv_timeout(wait_for)
                        }
                        None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
                    };

                    match next_event {
                        Ok(event) => {
                            worker_telemetry.record_dequeued();
                            match event {
                                RuntimeDispatch::Market(message) => {
                                    if let Some(batch) =
                                        merge_market_batch(&mut pending_market_batches, message)
                                    {
                                        publish_market_batch_to_subscribers(
                                            batch,
                                            &events_tx,
                                            &market_broadcaster,
                                        );
                                    }
                                    next_market_flush_at.get_or_insert_with(|| {
                                        Instant::now() + market_batch_interval
                                    });
                                }
                                RuntimeDispatch::MarketEvent(message) => {
                                    let l3_batch = L3BroadcastEvent {
                                        market: message.market.clone(),
                                        start_sequence: message.sequence,
                                        sequence: message.sequence,
                                        events: vec![message.event.clone()],
                                    };
                                    let _ = market_event_tx.send(message);
                                    market_event_broadcaster.publish(l3_batch);
                                }
                                RuntimeDispatch::Public(message) => {
                                    let _ = public_events_tx.send(message);
                                }
                                RuntimeDispatch::User(message) => {
                                    let _ = user_events_tx.send(message);
                                }
                                RuntimeDispatch::System(message) => {
                                    let _ = system_events_tx.send(message);
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            flush_market_batches(
                                &mut pending_market_batches,
                                &events_tx,
                                &market_broadcaster,
                            );
                            next_market_flush_at = None;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            flush_market_batches(
                                &mut pending_market_batches,
                                &events_tx,
                                &market_broadcaster,
                            );
                            break;
                        }
                    }
                }
                worker_telemetry.mark_stopped();
            })
            .unwrap_or_else(|error| panic!("failed to spawn runtime dispatcher thread: {error}"));
        Self { tx, telemetry }
    }

    fn dispatch_market(&self, event: BroadcastEvent) {
        self.send(RuntimeDispatch::Market(event));
    }

    fn dispatch_market_event(&self, event: MarketEventEnvelope) {
        self.send(RuntimeDispatch::MarketEvent(event));
    }

    fn dispatch_user(&self, event: UserBroadcastEvent) {
        self.send(RuntimeDispatch::User(event));
    }

    fn dispatch_public(&self, message: ServerMessage) {
        self.send(RuntimeDispatch::Public(message));
    }

    fn dispatch_system(&self, message: ServerMessage) {
        self.send(RuntimeDispatch::System(message));
    }

    fn send(&self, event: RuntimeDispatch) {
        self.telemetry.record_enqueue_started();
        let blocked_at = Instant::now();
        self.tx
            .send(event)
            .unwrap_or_else(|_| panic!("runtime dispatcher thread terminated"));
        self.telemetry.record_enqueue_blocked(blocked_at.elapsed());
    }

    fn status(&self) -> DispatchQueueStatus {
        self.telemetry.snapshot()
    }
}

#[derive(Clone)]
struct AccountDispatchHandle {
    tx: mpsc::SyncSender<AccountDispatch>,
    telemetry: DispatchQueueTelemetry,
}

enum AccountDispatch {
    UpsertPosition { trader_id: Uuid, position: Position },
    DeletePosition { trader_id: Uuid, market: String },
    UpsertOpenOrder { trader_id: Uuid, order: Order },
    DeleteOpenOrder { trader_id: Uuid, order_id: Uuid },
    AppendFill { trader_id: Uuid, fill: Fill },
    Barrier { respond_to: oneshot::Sender<()> },
}

impl AccountDispatchHandle {
    fn spawn(storage: StorageRepository, queue_capacity: usize) -> Self {
        let telemetry = DispatchQueueTelemetry::new(queue_capacity);
        let worker_telemetry = telemetry.clone();
        let (tx, rx) = mpsc::sync_channel(queue_capacity);
        thread::Builder::new()
            .name("exchange-account-dispatcher".to_string())
            .spawn(move || {
                while let Ok(task) = rx.recv() {
                    worker_telemetry.record_dequeued();
                    match task {
                        AccountDispatch::UpsertPosition {
                            trader_id,
                            position,
                        } => {
                            storage.upsert_position(trader_id, position);
                        }
                        AccountDispatch::DeletePosition { trader_id, market } => {
                            let _ = storage.delete_position(trader_id, &market);
                        }
                        AccountDispatch::UpsertOpenOrder { trader_id, order } => {
                            storage.upsert_open_order(trader_id, order);
                        }
                        AccountDispatch::DeleteOpenOrder {
                            trader_id,
                            order_id,
                        } => {
                            let _ = storage.delete_open_order(trader_id, order_id);
                        }
                        AccountDispatch::AppendFill { trader_id, fill } => {
                            storage.append_fill(trader_id, fill);
                        }
                        AccountDispatch::Barrier { respond_to } => {
                            let _ = respond_to.send(());
                        }
                    }
                }
                worker_telemetry.mark_stopped();
            })
            .unwrap_or_else(|error| panic!("failed to spawn account dispatcher thread: {error}"));
        Self { tx, telemetry }
    }

    fn upsert_position(&self, trader_id: Uuid, position: Position) {
        self.send(AccountDispatch::UpsertPosition {
            trader_id,
            position,
        });
    }

    fn delete_position(&self, trader_id: Uuid, market: String) {
        self.send(AccountDispatch::DeletePosition { trader_id, market });
    }

    fn upsert_open_order(&self, trader_id: Uuid, order: Order) {
        self.send(AccountDispatch::UpsertOpenOrder { trader_id, order });
    }

    fn delete_open_order(&self, trader_id: Uuid, order_id: Uuid) {
        self.send(AccountDispatch::DeleteOpenOrder {
            trader_id,
            order_id,
        });
    }

    fn append_fill(&self, trader_id: Uuid, fill: Fill) {
        self.send(AccountDispatch::AppendFill { trader_id, fill });
    }

    fn barrier(&self) -> oneshot::Receiver<()> {
        let (respond_to, response) = oneshot::channel();
        self.send(AccountDispatch::Barrier { respond_to });
        response
    }

    fn send(&self, task: AccountDispatch) {
        self.telemetry.record_enqueue_started();
        let blocked_at = Instant::now();
        self.tx
            .send(task)
            .unwrap_or_else(|_| panic!("account dispatcher thread terminated"));
        self.telemetry.record_enqueue_blocked(blocked_at.elapsed());
    }

    fn status(&self) -> DispatchQueueStatus {
        self.telemetry.snapshot()
    }
}

#[derive(Clone)]
struct PersistenceDispatchHandle {
    storage: StorageRepository,
    tx: Option<mpsc::SyncSender<PersistenceDispatch>>,
    telemetry: Option<DispatchQueueTelemetry>,
}

enum PersistenceDispatch {
    UpsertOrderLedger(Order),
    CloseOrderLedger {
        trader_id: Uuid,
        order_id: Uuid,
        remaining: u64,
    },
    AppendFill(Fill),
}

impl PersistenceDispatchHandle {
    fn spawn(storage: StorageRepository, queue_capacity: usize) -> Self {
        if storage.kind() == StorageBackendKind::InMemory {
            return Self {
                storage,
                tx: None,
                telemetry: None,
            };
        }

        let telemetry = DispatchQueueTelemetry::new(queue_capacity);
        let worker_telemetry = telemetry.clone();
        let (tx, rx) = mpsc::sync_channel(queue_capacity);
        let worker_storage = storage.clone();
        thread::Builder::new()
            .name("exchange-runtime-persistence".to_string())
            .spawn(move || {
                while let Ok(task) = rx.recv() {
                    worker_telemetry.record_dequeued();
                    match task {
                        PersistenceDispatch::UpsertOrderLedger(order) => {
                            worker_storage.upsert_order_ledger(order);
                        }
                        PersistenceDispatch::CloseOrderLedger {
                            trader_id,
                            order_id,
                            remaining,
                        } => {
                            worker_storage.close_order_ledger(trader_id, order_id, remaining);
                        }
                        PersistenceDispatch::AppendFill(fill) => {
                            worker_storage.persist_fill(fill);
                        }
                    }
                }
                worker_telemetry.mark_stopped();
            })
            .unwrap_or_else(|error| {
                panic!("failed to spawn persistence dispatcher thread: {error}")
            });

        Self {
            storage,
            tx: Some(tx),
            telemetry: Some(telemetry),
        }
    }

    fn upsert_order_ledger(&self, order: Order) {
        if self
            .send(PersistenceDispatch::UpsertOrderLedger(order.clone()))
            .is_some()
        {
            return;
        }
        self.storage.upsert_order_ledger(order);
    }

    fn close_order_ledger(&self, trader_id: Uuid, order_id: Uuid, remaining: u64) {
        if self
            .send(PersistenceDispatch::CloseOrderLedger {
                trader_id,
                order_id,
                remaining,
            })
            .is_some()
        {
            return;
        }
        self.storage
            .close_order_ledger(trader_id, order_id, remaining);
    }

    fn append_fill(&self, fill: Fill) {
        if self
            .send(PersistenceDispatch::AppendFill(fill.clone()))
            .is_some()
        {
            return;
        }
        self.storage.persist_fill(fill);
    }

    fn send(&self, task: PersistenceDispatch) -> Option<()> {
        let tx = self.tx.as_ref()?;
        self.telemetry
            .as_ref()
            .expect("persistence telemetry")
            .record_enqueue_started();
        let blocked_at = Instant::now();
        tx.send(task)
            .unwrap_or_else(|_| panic!("persistence dispatcher thread terminated"));
        self.telemetry
            .as_ref()
            .expect("persistence telemetry")
            .record_enqueue_blocked(blocked_at.elapsed());
        Some(())
    }

    fn status(&self) -> DispatchQueueStatus {
        self.telemetry
            .as_ref()
            .map(DispatchQueueTelemetry::snapshot)
            .unwrap_or_else(DispatchQueueStatus::disabled)
    }
}

fn backpressure_threshold(queue_capacity: usize) -> usize {
    std::cmp::max(1, (queue_capacity.saturating_mul(80) + 99) / 100)
}

fn duration_to_millis(duration: Duration) -> u64 {
    duration
        .as_millis()
        .min(u128::from(u64::MAX))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn update_max_usize(target: &AtomicUsize, candidate: usize) {
    let mut current = target.load(Ordering::Relaxed);
    while candidate > current {
        match target.compare_exchange_weak(current, candidate, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn update_max_u64(target: &AtomicU64, candidate: u64) {
    let mut current = target.load(Ordering::Relaxed);
    while candidate > current {
        match target.compare_exchange_weak(current, candidate, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::orderbook::Side;
    use chrono::{TimeZone, Utc};
    use tokio::time::timeout;

    fn test_config() -> Config {
        Config {
            bind_addr: "127.0.0.1:0".to_string(),
            database_url: "postgres://test".to_string(),
            storage_backend: crate::storage::StorageBackendKind::InMemory,
            ws_broadcast_buffer: 64,
            ws_market_delta_batch_interval_ms: 10,
            ws_market_broadcast_workers: 1,
            market_data_service_socket: None,
            market_data_service_retry_backoff_ms: 250,
            runtime_dispatch_queue_capacity: 4_096,
            account_dispatch_queue_capacity: 4_096,
            persistence_dispatch_queue_capacity: 4_096,
            per_user_rate_limit_burst_capacity: 500,
            per_user_rate_limit_burst_window_seconds: 10,
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

        let btc_book = state.market_book_snapshot("BTC-USD").await;
        let bids = btc_book.bids;
        assert_eq!(bids.len(), 1);
        assert_eq!(bids[0].price, 100);
        assert_eq!(bids[0].quantity, 5);

        let eth_book = state.market_book_snapshot("ETH-USD").await;
        let asks = eth_book.asks;
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].price, 200);
        assert_eq!(asks[0].quantity, 1);

        let btc_l3 = state.market_l3_snapshot("BTC-USD");
        assert_eq!(btc_l3.sequence, 0);
        assert_eq!(btc_l3.bids.len(), 2);
        assert!(btc_l3.asks.is_empty());

        let eth_l3 = state.market_l3_snapshot("ETH-USD");
        assert_eq!(eth_l3.sequence, 0);
        assert_eq!(eth_l3.asks.len(), 1);
        assert!(eth_l3.bids.is_empty());

        assert_eq!(state.current_market_sequence("BTC-USD"), 0);
        assert_eq!(state.current_market_sequence("ETH-USD"), 0);
        assert_eq!(state.current_market_event_sequence("BTC-USD"), 0);
        assert_eq!(state.current_market_event_sequence("ETH-USD"), 0);
    }

    #[test]
    fn dispatch_queue_telemetry_reports_backpressure_and_blocked_enqueue_time() {
        let telemetry = DispatchQueueTelemetry::new(10);
        for _ in 0..8 {
            telemetry.record_enqueue_started();
        }
        telemetry.record_enqueue_started();
        telemetry.record_enqueue_blocked(Duration::from_millis(3));

        let status = telemetry.snapshot();
        assert_eq!(status.mode, DispatchQueueMode::Backpressured);
        assert_eq!(status.queue_capacity, 10);
        assert_eq!(status.backpressure_threshold, 8);
        assert_eq!(status.queue_depth, 9);
        assert_eq!(status.high_water_mark, 9);
        assert_eq!(status.total_enqueued, 9);
        assert_eq!(status.total_blocked_enqueues, 1);
        assert_eq!(status.total_enqueue_block_time_ms, 3);

        telemetry.record_dequeued();
        telemetry.record_dequeued();
        let recovered = telemetry.snapshot();
        assert_eq!(recovered.mode, DispatchQueueMode::Ok);
        assert_eq!(recovered.queue_depth, 7);
        assert_eq!(recovered.total_dequeued, 2);
    }

    #[test]
    fn account_barrier_telemetry_tracks_wait_buckets_per_operation() {
        let telemetry = AccountBarrierTelemetry::default();
        telemetry.record(BarrierKind::Submit, Duration::from_millis(7));
        telemetry.record(BarrierKind::Submit, Duration::from_millis(0));
        telemetry.record(BarrierKind::Amend, Duration::from_millis(120));

        let status = telemetry.snapshot();
        assert_eq!(status.submit.total_waits, 2);
        assert_eq!(status.submit.total_wait_time_ms, 7);
        assert_eq!(status.submit.max_wait_time_ms, 7);
        assert_eq!(status.submit.waits_over_5ms, 1);
        assert_eq!(status.submit.waits_over_25ms, 0);
        assert_eq!(status.cancel.total_waits, 0);
        assert_eq!(status.amend.total_waits, 1);
        assert_eq!(status.amend.waits_over_100ms, 1);
        assert_eq!(status.amend.last_wait_time_ms, 120);
    }

    #[tokio::test]
    async fn market_broadcast_batches_consecutive_deltas_for_fanout() {
        let state = AppState::new(test_config());
        let (tx, mut rx) = tokio_mpsc::unbounded_channel();
        let client_id = state.register_book_stream(tx);
        state.update_book_stream_subscription(client_id, Some("BTC-USD".to_string()), Some(0));

        state.dispatch_market_delta(
            "BTC-USD",
            BookDelta::LevelUpdated {
                side: Side::Buy,
                price: 100,
                quantity: 2,
            },
        );
        state.dispatch_market_delta(
            "BTC-USD",
            BookDelta::Trade {
                price: 100,
                quantity: 1,
            },
        );

        let message = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for batched market delta")
            .expect("market message");
        match message.as_ref() {
            ServerMessage::Delta {
                channel,
                market,
                start_sequence,
                sequence,
                events,
            } => {
                assert_eq!(channel, "l2");
                assert_eq!(market, "BTC-USD");
                assert_eq!(*start_sequence, 1);
                assert_eq!(*sequence, 2);
                assert_eq!(events.len(), 2);
                assert!(matches!(
                    events[0],
                    BookDelta::LevelUpdated {
                        side: Side::Buy,
                        price: 100,
                        quantity: 2,
                    }
                ));
                assert!(matches!(
                    events[1],
                    BookDelta::Trade {
                        price: 100,
                        quantity: 1,
                    }
                ));
            }
            other => panic!("unexpected market message: {other:?}"),
        }

        assert!(
            timeout(Duration::from_millis(25), rx.recv()).await.is_err(),
            "expected a single batched delta message",
        );
        state.unregister_book_stream(client_id);
    }

    #[tokio::test]
    async fn market_broadcast_requests_resync_after_sequence_gap() {
        let state = AppState::new(test_config());
        let (tx, mut rx) = tokio_mpsc::unbounded_channel();
        let client_id = state.register_book_stream(tx);
        state.update_book_stream_subscription(client_id, Some("BTC-USD".to_string()), Some(4));

        state.runtime_dispatcher.dispatch_market(BroadcastEvent {
            market: "BTC-USD".to_string(),
            start_sequence: 7,
            sequence: 7,
            events: vec![BookDelta::LevelUpdated {
                side: Side::Sell,
                price: 101,
                quantity: 0,
            }],
        });

        let message = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for resync message")
            .expect("market message");
        assert_eq!(
            message.as_ref(),
            &ServerMessage::ResyncRequired {
                channel: "l2".to_string(),
                market: Some("BTC-USD".to_string()),
                expected_sequence: Some(5),
                current_sequence: Some(7),
                reason: "market sequence gap detected; resubscribe for a fresh snapshot"
                    .to_string(),
            }
        );
        state.unregister_book_stream(client_id);
    }

    #[tokio::test]
    async fn l3_broadcast_streams_raw_market_events_and_detects_gaps() {
        let state = AppState::new(test_config());
        let (tx, mut rx) = tokio_mpsc::unbounded_channel();
        let client_id = state.register_l3_stream(tx);
        state.update_l3_stream_subscription(client_id, Some("BTC-USD".to_string()), Some(0));

        state.dispatch_market_event(
            "BTC-USD",
            MarketEvent::OrderAdded {
                order_id: Uuid::from_u128(1),
                side: Side::Buy,
                price: 100,
                remaining: 2,
                created_at: Utc::now(),
            },
        );

        let message = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for l3 delta")
            .expect("l3 message");
        match message.as_ref() {
            ServerMessage::L3Delta {
                channel,
                market,
                start_sequence,
                sequence,
                events,
            } => {
                assert_eq!(channel, "l3");
                assert_eq!(market, "BTC-USD");
                assert_eq!(*start_sequence, 1);
                assert_eq!(*sequence, 1);
                assert!(matches!(
                    events.as_slice(),
                    [MarketEvent::OrderAdded {
                        side: Side::Buy,
                        price: 100,
                        remaining: 2,
                        ..
                    }]
                ));
            }
            other => panic!("unexpected l3 message: {other:?}"),
        }

        state.update_l3_stream_subscription(client_id, Some("BTC-USD".to_string()), Some(4));
        state
            .runtime_dispatcher
            .dispatch_market_event(MarketEventEnvelope {
                market: "BTC-USD".to_string(),
                sequence: 7,
                recorded_at: Utc::now(),
                event: MarketEvent::OrderRemoved {
                    order_id: Uuid::from_u128(1),
                    side: Side::Buy,
                    price: 100,
                    reason: crate::marketdata::MarketEventRemoveReason::Canceled,
                },
            });

        let message = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for l3 resync message")
            .expect("l3 resync message");
        assert_eq!(
            message.as_ref(),
            &ServerMessage::ResyncRequired {
                channel: "l3".to_string(),
                market: Some("BTC-USD".to_string()),
                expected_sequence: Some(5),
                current_sequence: Some(7),
                reason: "market sequence gap detected; resubscribe for a fresh snapshot"
                    .to_string(),
            }
        );

        state.unregister_l3_stream(client_id);
    }
}
