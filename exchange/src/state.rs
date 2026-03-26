use crate::bots::BotManager;
use crate::config::Config;
use crate::marketdata::{BookDelta, BroadcastEvent, ServerMessage, UserBroadcastEvent};
use crate::orderbook::{Fill, Order, OrderBook};
use crate::rate_limit::PerUserRateLimiter;
use crate::storage::{StorageBackendKind, StorageRepository};
use crate::trading::{MarketBookSnapshot, MarketEngineHandle};
use chrono::{DateTime, Utc};
use dashmap::{DashMap, mapref::entry::Entry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, oneshot};
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
    pub bot_manager: BotManager,
    runtime_dispatcher: RuntimeDispatchHandle,
    account_dispatcher: AccountDispatchHandle,
    account_barrier_telemetry: AccountBarrierTelemetry,
    persistence_dispatcher: PersistenceDispatchHandle,
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
        let runtime_dispatcher = RuntimeDispatchHandle::spawn(
            config.runtime_dispatch_queue_capacity,
            events_tx.clone(),
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
            bot_manager: BotManager::default(),
            runtime_dispatcher,
            account_dispatcher,
            account_barrier_telemetry,
            persistence_dispatcher,
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

    pub fn dispatch_market_delta(&self, market: &str, event: BookDelta) {
        self.runtime_dispatcher.dispatch_market(BroadcastEvent {
            market: market.to_string(),
            sequence: self.next_market_sequence(market),
            event,
        });
    }

    pub fn dispatch_user_event(&self, trader_id: Uuid, message: ServerMessage) {
        self.runtime_dispatcher
            .dispatch_user(UserBroadcastEvent { trader_id, message });
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
    }

    pub fn clear_market_runtime(&self) {
        self.market_engines.clear();
    }

    pub async fn market_book_snapshot(&self, market: &str) -> MarketBookSnapshot {
        if let Some(engine) = self.market_engines.get(market) {
            let handle = engine.clone();
            return handle.snapshot().await.unwrap_or_default();
        }
        let recovered = recover_orderbook(self.storage.list_all_open_orders(), market);
        MarketBookSnapshot {
            bids: recovered.levels_for_side(crate::orderbook::Side::Buy),
            asks: recovered.levels_for_side(crate::orderbook::Side::Sell),
        }
    }

    pub async fn market_best_prices(&self, market: &str) -> (Option<u64>, Option<u64>) {
        if let Some(engine) = self.market_engines.get(market) {
            let handle = engine.clone();
            return handle.best_prices().await.unwrap_or((None, None));
        }
        let recovered = recover_orderbook(self.storage.list_all_open_orders(), market);
        (recovered.best_bid_price(), recovered.best_ask_price())
    }

    fn recover_runtime_state(&self) {
        for market in self.storage.list_markets() {
            self.market_sequences.entry(market.market_id).or_insert(0);
        }
        let open_orders = self.storage.list_all_open_orders();
        let recovered = recover_orderbooks(open_orders.clone());
        for (market, orderbook) in recovered {
            self.market_engines.insert(
                market.clone(),
                MarketEngineHandle::spawn(self.clone(), market.clone(), orderbook),
            );
            self.market_sequences.entry(market).or_insert(0);
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
struct RuntimeDispatchHandle {
    tx: mpsc::SyncSender<RuntimeDispatch>,
    telemetry: DispatchQueueTelemetry,
}

enum RuntimeDispatch {
    Market(BroadcastEvent),
    User(UserBroadcastEvent),
    System(ServerMessage),
}

impl RuntimeDispatchHandle {
    fn spawn(
        queue_capacity: usize,
        events_tx: broadcast::Sender<BroadcastEvent>,
        user_events_tx: broadcast::Sender<UserBroadcastEvent>,
        system_events_tx: broadcast::Sender<ServerMessage>,
    ) -> Self {
        let telemetry = DispatchQueueTelemetry::new(queue_capacity);
        let worker_telemetry = telemetry.clone();
        let (tx, rx) = mpsc::sync_channel(queue_capacity);
        thread::Builder::new()
            .name("exchange-runtime-dispatcher".to_string())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    worker_telemetry.record_dequeued();
                    match event {
                        RuntimeDispatch::Market(message) => {
                            let _ = events_tx.send(message);
                        }
                        RuntimeDispatch::User(message) => {
                            let _ = user_events_tx.send(message);
                        }
                        RuntimeDispatch::System(message) => {
                            let _ = system_events_tx.send(message);
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

    fn dispatch_user(&self, event: UserBroadcastEvent) {
        self.send(RuntimeDispatch::User(event));
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

    fn test_config() -> Config {
        Config {
            bind_addr: "127.0.0.1:0".to_string(),
            database_url: "postgres://test".to_string(),
            storage_backend: crate::storage::StorageBackendKind::InMemory,
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

        assert_eq!(state.current_market_sequence("BTC-USD"), 0);
        assert_eq!(state.current_market_sequence("ETH-USD"), 0);
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
}
