use crate::accounts::{UserProfile, UserRecord, UserRole};
use crate::admin::{
    AdminAuditEntry, AdminMessageEntry, AdminMessageLevel, CompetitionLeaderboardSnapshot,
    ExchangeControls, MarketDefinition, MarketStatus,
};
use crate::config::Config;
use crate::orderbook::{Fill, Order, Side};
use crate::settlement::{SettlementJournalEntry, SettlementJournalReason};
use crate::state::{Balance, Position};
use chrono::Utc;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use postgres::{Client, NoTls, Row, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

const DEFAULT_POSTGRES_BACKPRESSURE_PERCENT: usize = 80;

pub const USERS_TABLE: &str = "users";
pub const API_KEYS_TABLE: &str = "api_keys";
pub const EXCHANGE_CONTROLS_TABLE: &str = "exchange_controls";
pub const MARKETS_TABLE: &str = "markets";
pub const ADMIN_AUDIT_LOGS_TABLE: &str = "admin_audit_logs";
pub const ADMIN_MESSAGES_TABLE: &str = "admin_messages";
pub const BALANCES_TABLE: &str = "balances";
pub const SETTLEMENT_JOURNAL_TABLE: &str = "settlement_journal";
pub const POSITIONS_TABLE: &str = "positions";
pub const PENDING_POSITIONS_TABLE: &str = "pending_positions";
pub const ORDERS_TABLE: &str = "orders";
pub const FILLS_TABLE: &str = "fills";
pub const PNL_SNAPSHOTS_TABLE: &str = "pnl_snapshots";
pub const COMPETITION_LEADERBOARD_SNAPSHOTS_TABLE: &str = "competition_leaderboard_snapshots";
pub const COMPETITION_LEADERBOARD_SNAPSHOT_ROWS_TABLE: &str =
    "competition_leaderboard_snapshot_rows";
pub const POSTGRES_INITIAL_SCHEMA: &str = include_str!("../sql/migrations/001_initial.sql");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackendKind {
    InMemory,
    Postgres,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceMode {
    Disabled,
    Ok,
    Backpressured,
    Retrying,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PersistenceStatus {
    pub backend: StorageBackendKind,
    pub mode: PersistenceMode,
    pub queue_capacity: usize,
    pub backpressure_threshold: usize,
    pub queue_depth: usize,
    pub in_flight_ops: usize,
    pub backlog_depth: usize,
    pub high_water_mark: usize,
    pub total_enqueued: u64,
    pub total_flushes: u64,
    pub total_flushed_ops: u64,
    pub total_blocked_enqueues: u64,
    pub total_enqueue_block_time_ms: u64,
    pub total_flush_failures: u64,
    pub total_retries: u64,
    pub last_batch_size: usize,
    pub last_flush_latency_ms: u64,
    pub max_flush_latency_ms: u64,
    pub last_error: Option<String>,
}

impl PersistenceStatus {
    fn disabled(backend: StorageBackendKind) -> Self {
        Self {
            backend,
            mode: PersistenceMode::Disabled,
            queue_capacity: 0,
            backpressure_threshold: 0,
            queue_depth: 0,
            in_flight_ops: 0,
            backlog_depth: 0,
            high_water_mark: 0,
            total_enqueued: 0,
            total_flushes: 0,
            total_flushed_ops: 0,
            total_blocked_enqueues: 0,
            total_enqueue_block_time_ms: 0,
            total_flush_failures: 0,
            total_retries: 0,
            last_batch_size: 0,
            last_flush_latency_ms: 0,
            max_flush_latency_ms: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StorageError {
    #[error("username already exists")]
    UsernameTaken,
    #[error("api key already exists")]
    ApiKeyTaken,
}

pub trait StorageBackend: Send + Sync {
    fn kind(&self) -> StorageBackendKind;
    fn persistence_status(&self) -> PersistenceStatus;
    fn create_user(&self, record: UserRecord) -> Result<(), StorageError>;
    fn list_users(&self) -> Vec<UserRecord>;
    fn get_user(&self, trader_id: Uuid) -> Option<UserRecord>;
    fn get_user_by_username(&self, username: &str) -> Option<UserRecord>;
    fn get_user_by_api_key(&self, api_key: &str) -> Option<UserRecord>;
    fn get_exchange_controls(&self) -> ExchangeControls;
    fn set_exchange_controls(&self, controls: ExchangeControls);
    fn list_markets(&self) -> Vec<MarketDefinition>;
    fn get_market(&self, market_id: &str) -> Option<MarketDefinition>;
    fn upsert_market(&self, market: MarketDefinition);
    fn delete_market(&self, market_id: &str) -> Option<MarketDefinition>;
    fn append_admin_audit_log(&self, entry: AdminAuditEntry);
    fn list_admin_audit_logs(&self) -> Vec<AdminAuditEntry>;
    fn append_admin_message(&self, entry: AdminMessageEntry);
    fn list_admin_messages(&self, limit: Option<usize>) -> Vec<AdminMessageEntry>;
    fn append_competition_snapshot(&self, snapshot: CompetitionLeaderboardSnapshot);
    fn get_competition_snapshot(&self, snapshot_id: Uuid)
    -> Option<CompetitionLeaderboardSnapshot>;
    fn latest_competition_snapshot(
        &self,
        competition_id: &str,
    ) -> Option<CompetitionLeaderboardSnapshot>;
    fn list_balances(&self, trader_id: Uuid) -> Vec<Balance>;
    fn list_all_balances(&self) -> Vec<(Uuid, Vec<Balance>)>;
    fn put_balance(&self, trader_id: Uuid, balance: Balance);
    fn replace_balances(&self, trader_id: Uuid, balances: Vec<Balance>);
    fn apply_settlement_update(
        &self,
        trader_id: Uuid,
        balances: Vec<Balance>,
        journal_entries: Vec<SettlementJournalEntry>,
    );
    fn list_settlement_journal(&self) -> Vec<SettlementJournalEntry>;
    fn list_positions(&self, trader_id: Uuid) -> Vec<Position>;
    fn list_all_positions(&self) -> Vec<(Uuid, Vec<Position>)>;
    fn get_position(&self, trader_id: Uuid, market: &str) -> Option<Position>;
    fn upsert_position(&self, trader_id: Uuid, position: Position);
    fn delete_position(&self, trader_id: Uuid, market: &str) -> Option<Position>;
    fn replace_positions(&self, trader_id: Uuid, positions: Vec<Position>);
    fn upsert_order_ledger(&self, order: Order);
    fn close_order_ledger(&self, trader_id: Uuid, order_id: Uuid, remaining: u64);
    fn list_all_open_orders(&self) -> Vec<Order>;
    fn list_open_orders(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Order>;
    fn get_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order>;
    fn upsert_open_order(&self, trader_id: Uuid, order: Order);
    fn delete_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order>;
    fn append_fill(&self, trader_id: Uuid, fill: Fill);
    fn persist_fill(&self, fill: Fill);
    fn list_fills(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Fill>;
    fn reset_all_trading_state(&self);
}

#[derive(Clone)]
pub struct StorageRepository {
    backend: Arc<dyn StorageBackend>,
}

impl StorageRepository {
    pub fn from_config(config: &Config) -> Self {
        match config.storage_backend {
            StorageBackendKind::InMemory => Self::new_in_memory(),
            StorageBackendKind::Postgres => Self::new_postgres(config),
        }
    }

    pub fn new_in_memory() -> Self {
        Self {
            backend: Arc::new(InMemoryRepository::default()),
        }
    }

    pub fn new_postgres(config: &Config) -> Self {
        Self {
            backend: Arc::new(PostgresRepository::new(config)),
        }
    }

    pub fn kind(&self) -> StorageBackendKind {
        self.backend.kind()
    }

    pub fn persistence_status(&self) -> PersistenceStatus {
        self.backend.persistence_status()
    }

    pub fn create_user(&self, record: UserRecord) -> Result<(), StorageError> {
        self.backend.create_user(record)
    }

    pub fn list_users(&self) -> Vec<UserRecord> {
        self.backend.list_users()
    }

    pub fn get_user(&self, trader_id: Uuid) -> Option<UserRecord> {
        self.backend.get_user(trader_id)
    }

    pub fn get_user_by_username(&self, username: &str) -> Option<UserRecord> {
        self.backend.get_user_by_username(username)
    }

    pub fn get_user_by_api_key(&self, api_key: &str) -> Option<UserRecord> {
        self.backend.get_user_by_api_key(api_key)
    }

    pub fn get_exchange_controls(&self) -> ExchangeControls {
        self.backend.get_exchange_controls()
    }

    pub fn set_exchange_controls(&self, controls: ExchangeControls) {
        self.backend.set_exchange_controls(controls)
    }

    pub fn list_markets(&self) -> Vec<MarketDefinition> {
        self.backend.list_markets()
    }

    pub fn get_market(&self, market_id: &str) -> Option<MarketDefinition> {
        self.backend.get_market(market_id)
    }

    pub fn upsert_market(&self, market: MarketDefinition) {
        self.backend.upsert_market(market)
    }

    pub fn delete_market(&self, market_id: &str) -> Option<MarketDefinition> {
        self.backend.delete_market(market_id)
    }

    pub fn append_admin_audit_log(&self, entry: AdminAuditEntry) {
        self.backend.append_admin_audit_log(entry)
    }

    pub fn list_admin_audit_logs(&self) -> Vec<AdminAuditEntry> {
        self.backend.list_admin_audit_logs()
    }

    pub fn append_admin_message(&self, entry: AdminMessageEntry) {
        self.backend.append_admin_message(entry)
    }

    pub fn list_admin_messages(&self, limit: Option<usize>) -> Vec<AdminMessageEntry> {
        self.backend.list_admin_messages(limit)
    }

    pub fn append_competition_snapshot(&self, snapshot: CompetitionLeaderboardSnapshot) {
        self.backend.append_competition_snapshot(snapshot)
    }

    pub fn get_competition_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> Option<CompetitionLeaderboardSnapshot> {
        self.backend.get_competition_snapshot(snapshot_id)
    }

    pub fn latest_competition_snapshot(
        &self,
        competition_id: &str,
    ) -> Option<CompetitionLeaderboardSnapshot> {
        self.backend.latest_competition_snapshot(competition_id)
    }

    pub fn list_balances(&self, trader_id: Uuid) -> Vec<Balance> {
        self.backend.list_balances(trader_id)
    }

    pub fn list_all_balances(&self) -> Vec<(Uuid, Vec<Balance>)> {
        self.backend.list_all_balances()
    }

    pub fn put_balance(&self, trader_id: Uuid, balance: Balance) {
        self.backend.put_balance(trader_id, balance)
    }

    pub fn replace_balances(&self, trader_id: Uuid, balances: Vec<Balance>) {
        self.backend.replace_balances(trader_id, balances)
    }

    pub fn apply_settlement_update(
        &self,
        trader_id: Uuid,
        balances: Vec<Balance>,
        journal_entries: Vec<SettlementJournalEntry>,
    ) {
        self.backend
            .apply_settlement_update(trader_id, balances, journal_entries)
    }

    pub fn list_settlement_journal(&self) -> Vec<SettlementJournalEntry> {
        self.backend.list_settlement_journal()
    }

    pub fn list_positions(&self, trader_id: Uuid) -> Vec<Position> {
        self.backend.list_positions(trader_id)
    }

    pub fn list_all_positions(&self) -> Vec<(Uuid, Vec<Position>)> {
        self.backend.list_all_positions()
    }

    pub fn get_position(&self, trader_id: Uuid, market: &str) -> Option<Position> {
        self.backend.get_position(trader_id, market)
    }

    pub fn upsert_position(&self, trader_id: Uuid, position: Position) {
        self.backend.upsert_position(trader_id, position)
    }

    pub fn delete_position(&self, trader_id: Uuid, market: &str) -> Option<Position> {
        self.backend.delete_position(trader_id, market)
    }

    pub fn replace_positions(&self, trader_id: Uuid, positions: Vec<Position>) {
        self.backend.replace_positions(trader_id, positions)
    }

    pub fn upsert_order_ledger(&self, order: Order) {
        self.backend.upsert_order_ledger(order)
    }

    pub fn close_order_ledger(&self, trader_id: Uuid, order_id: Uuid, remaining: u64) {
        self.backend
            .close_order_ledger(trader_id, order_id, remaining)
    }

    pub fn list_all_open_orders(&self) -> Vec<Order> {
        self.backend.list_all_open_orders()
    }

    pub fn list_open_orders(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Order> {
        self.backend.list_open_orders(trader_id, market)
    }

    pub fn get_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order> {
        self.backend.get_open_order(trader_id, order_id)
    }

    pub fn upsert_open_order(&self, trader_id: Uuid, order: Order) {
        self.backend.upsert_open_order(trader_id, order)
    }

    pub fn delete_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order> {
        self.backend.delete_open_order(trader_id, order_id)
    }

    pub fn append_fill(&self, trader_id: Uuid, fill: Fill) {
        self.backend.append_fill(trader_id, fill)
    }

    pub fn persist_fill(&self, fill: Fill) {
        self.backend.persist_fill(fill)
    }

    pub fn list_fills(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Fill> {
        self.backend.list_fills(trader_id, market)
    }

    pub fn reset_all_trading_state(&self) {
        self.backend.reset_all_trading_state()
    }
}

#[derive(Default)]
struct InMemoryRepository {
    users: DashMap<Uuid, UserRecord>,
    usernames: DashMap<String, Uuid>,
    api_keys: DashMap<String, Uuid>,
    exchange_controls: Arc<Mutex<ExchangeControls>>,
    markets: DashMap<String, MarketDefinition>,
    admin_audit_logs: Arc<Mutex<Vec<AdminAuditEntry>>>,
    admin_messages: Arc<Mutex<Vec<AdminMessageEntry>>>,
    competition_snapshots: Arc<Mutex<Vec<CompetitionLeaderboardSnapshot>>>,
    settlement_journal: Arc<Mutex<Vec<SettlementJournalEntry>>>,
    accounts: DashMap<Uuid, Arc<Mutex<AccountPartition>>>,
}

#[derive(Default)]
struct AccountPartition {
    balances: BTreeMap<String, Balance>,
    positions: BTreeMap<String, Position>,
    open_orders: BTreeMap<Uuid, Order>,
    fills: BTreeMap<Uuid, Fill>,
}

impl StorageBackend for InMemoryRepository {
    fn kind(&self) -> StorageBackendKind {
        StorageBackendKind::InMemory
    }

    fn persistence_status(&self) -> PersistenceStatus {
        PersistenceStatus::disabled(StorageBackendKind::InMemory)
    }

    fn create_user(&self, record: UserRecord) -> Result<(), StorageError> {
        let trader_id = record.profile.trader_id;
        let username = record.profile.username.clone();
        let api_key = record.profile.api_key.clone();

        match self.usernames.entry(username.clone()) {
            Entry::Occupied(_) => return Err(StorageError::UsernameTaken),
            Entry::Vacant(entry) => {
                entry.insert(trader_id);
            }
        }
        match self.api_keys.entry(api_key.clone()) {
            Entry::Occupied(_) => {
                self.usernames.remove(&username);
                return Err(StorageError::ApiKeyTaken);
            }
            Entry::Vacant(entry) => {
                entry.insert(trader_id);
            }
        }

        self.users.insert(trader_id, record);
        Ok(())
    }

    fn list_users(&self) -> Vec<UserRecord> {
        let mut users = self
            .users
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        users.sort_by(|left, right| {
            left.profile
                .username
                .cmp(&right.profile.username)
                .then_with(|| left.profile.trader_id.cmp(&right.profile.trader_id))
        });
        users
    }

    fn get_user(&self, trader_id: Uuid) -> Option<UserRecord> {
        self.users.get(&trader_id).map(|entry| entry.clone())
    }

    fn get_user_by_username(&self, username: &str) -> Option<UserRecord> {
        let trader_id = self.usernames.get(username).map(|entry| *entry.value())?;
        self.get_user(trader_id)
    }

    fn get_user_by_api_key(&self, api_key: &str) -> Option<UserRecord> {
        let trader_id = self.api_keys.get(api_key).map(|entry| *entry.value())?;
        self.get_user(trader_id)
    }

    fn get_exchange_controls(&self) -> ExchangeControls {
        self.exchange_controls
            .lock()
            .expect("exchange controls lock")
            .clone()
    }

    fn set_exchange_controls(&self, controls: ExchangeControls) {
        *self
            .exchange_controls
            .lock()
            .expect("exchange controls lock") = controls;
    }

    fn list_markets(&self) -> Vec<MarketDefinition> {
        let mut markets = self
            .markets
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        markets.sort_by(|left, right| left.market_id.cmp(&right.market_id));
        markets
    }

    fn get_market(&self, market_id: &str) -> Option<MarketDefinition> {
        self.markets.get(market_id).map(|entry| entry.clone())
    }

    fn upsert_market(&self, market: MarketDefinition) {
        self.markets.insert(market.market_id.clone(), market);
    }

    fn delete_market(&self, market_id: &str) -> Option<MarketDefinition> {
        self.markets.remove(market_id).map(|(_, market)| market)
    }

    fn append_admin_audit_log(&self, entry: AdminAuditEntry) {
        let mut guard = self.admin_audit_logs.lock().expect("admin audit log lock");
        guard.push(entry);
    }

    fn list_admin_audit_logs(&self) -> Vec<AdminAuditEntry> {
        let mut entries = self
            .admin_audit_logs
            .lock()
            .expect("admin audit log lock")
            .clone();
        entries.sort_by_key(|entry| (entry.occurred_at, entry.audit_id));
        entries
    }

    fn append_admin_message(&self, entry: AdminMessageEntry) {
        let mut guard = self.admin_messages.lock().expect("admin message lock");
        guard.push(entry);
    }

    fn list_admin_messages(&self, limit: Option<usize>) -> Vec<AdminMessageEntry> {
        let mut entries = self
            .admin_messages
            .lock()
            .expect("admin message lock")
            .clone();
        entries.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.message_id.cmp(&left.message_id))
        });
        if let Some(limit) = limit {
            entries.truncate(limit);
        }
        entries
    }

    fn append_competition_snapshot(&self, snapshot: CompetitionLeaderboardSnapshot) {
        let mut guard = self
            .competition_snapshots
            .lock()
            .expect("competition snapshots lock");
        guard.retain(|existing| existing.snapshot_id != snapshot.snapshot_id);
        guard.push(snapshot);
    }

    fn get_competition_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> Option<CompetitionLeaderboardSnapshot> {
        self.competition_snapshots
            .lock()
            .expect("competition snapshots lock")
            .iter()
            .find(|snapshot| snapshot.snapshot_id == snapshot_id)
            .cloned()
    }

    fn latest_competition_snapshot(
        &self,
        competition_id: &str,
    ) -> Option<CompetitionLeaderboardSnapshot> {
        self.competition_snapshots
            .lock()
            .expect("competition snapshots lock")
            .iter()
            .filter(|snapshot| snapshot.competition_id == competition_id)
            .max_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.snapshot_id.cmp(&right.snapshot_id))
            })
            .cloned()
    }

    fn list_balances(&self, trader_id: Uuid) -> Vec<Balance> {
        self.with_account(trader_id, |account| {
            account.balances.values().cloned().collect()
        })
    }

    fn list_all_balances(&self) -> Vec<(Uuid, Vec<Balance>)> {
        let mut balances = Vec::new();
        for entry in &self.accounts {
            let trader_id = *entry.key();
            let mut trader_balances = entry
                .value()
                .lock()
                .expect("account partition lock")
                .balances
                .values()
                .cloned()
                .collect::<Vec<_>>();
            trader_balances.sort_by(|left, right| left.asset.cmp(&right.asset));
            balances.push((trader_id, trader_balances));
        }
        balances.sort_by_key(|(trader_id, _)| *trader_id);
        balances
    }

    fn put_balance(&self, trader_id: Uuid, balance: Balance) {
        self.with_account_mut(trader_id, |account| {
            account.balances.insert(balance.asset.clone(), balance);
        });
    }

    fn replace_balances(&self, trader_id: Uuid, balances: Vec<Balance>) {
        self.with_account_mut(trader_id, |account| {
            account.balances = balances
                .into_iter()
                .map(|balance| (balance.asset.clone(), balance))
                .collect();
        });
    }

    fn apply_settlement_update(
        &self,
        trader_id: Uuid,
        balances: Vec<Balance>,
        journal_entries: Vec<SettlementJournalEntry>,
    ) {
        self.replace_balances(trader_id, balances);
        if journal_entries.is_empty() {
            return;
        }
        let mut guard = self
            .settlement_journal
            .lock()
            .expect("settlement journal lock");
        guard.extend(journal_entries);
    }

    fn list_settlement_journal(&self) -> Vec<SettlementJournalEntry> {
        let mut entries = self
            .settlement_journal
            .lock()
            .expect("settlement journal lock")
            .clone();
        entries.sort_by_key(|entry| (entry.occurred_at, entry.journal_id));
        entries
    }

    fn list_positions(&self, trader_id: Uuid) -> Vec<Position> {
        let mut positions = self.with_account(trader_id, |account| {
            account.positions.values().cloned().collect::<Vec<_>>()
        });
        positions.sort_by(|left, right| left.market.cmp(&right.market));
        positions
    }

    fn list_all_positions(&self) -> Vec<(Uuid, Vec<Position>)> {
        let mut positions = Vec::new();
        for entry in &self.accounts {
            let trader_id = *entry.key();
            let mut trader_positions = entry
                .value()
                .lock()
                .expect("account partition lock")
                .positions
                .values()
                .cloned()
                .collect::<Vec<_>>();
            trader_positions.sort_by(|left, right| left.market.cmp(&right.market));
            positions.push((trader_id, trader_positions));
        }
        positions.sort_by_key(|(trader_id, _)| *trader_id);
        positions
    }

    fn get_position(&self, trader_id: Uuid, market: &str) -> Option<Position> {
        self.with_account(trader_id, |account| account.positions.get(market).cloned())
    }

    fn upsert_position(&self, trader_id: Uuid, position: Position) {
        self.with_account_mut(trader_id, |account| {
            account.positions.insert(position.market.clone(), position);
        });
    }

    fn delete_position(&self, trader_id: Uuid, market: &str) -> Option<Position> {
        self.with_account_mut(trader_id, |account| account.positions.remove(market))
    }

    fn replace_positions(&self, trader_id: Uuid, positions: Vec<Position>) {
        self.with_account_mut(trader_id, |account| {
            account.positions = positions
                .into_iter()
                .map(|position| (position.market.clone(), position))
                .collect();
        });
    }

    fn upsert_order_ledger(&self, _order: Order) {}

    fn close_order_ledger(&self, _trader_id: Uuid, _order_id: Uuid, _remaining: u64) {}

    fn list_all_open_orders(&self) -> Vec<Order> {
        let mut orders = Vec::new();
        for entry in &self.accounts {
            let account = entry.value().lock().expect("account partition lock");
            orders.extend(account.open_orders.values().cloned());
        }
        orders.sort_by_key(|order| (order.market.clone(), order.created_at, order.id));
        orders
    }

    fn list_open_orders(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Order> {
        let mut orders = self.with_account(trader_id, |account| {
            account.open_orders.values().cloned().collect::<Vec<_>>()
        });
        if let Some(market) = market {
            orders.retain(|order| order.market == market);
        }
        orders.sort_by_key(|order| (order.created_at, order.id));
        orders
    }

    fn get_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order> {
        self.with_account(trader_id, |account| {
            account.open_orders.get(&order_id).cloned()
        })
    }

    fn upsert_open_order(&self, trader_id: Uuid, order: Order) {
        self.with_account_mut(trader_id, |account| {
            account.open_orders.insert(order.id, order);
        });
    }

    fn delete_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order> {
        self.with_account_mut(trader_id, |account| account.open_orders.remove(&order_id))
    }

    fn append_fill(&self, trader_id: Uuid, fill: Fill) {
        self.with_account_mut(trader_id, |account| {
            account.fills.insert(fill.fill_id, fill);
        });
    }

    fn persist_fill(&self, _fill: Fill) {}

    fn list_fills(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Fill> {
        let mut fills = self.with_account(trader_id, |account| {
            account.fills.values().cloned().collect::<Vec<_>>()
        });
        if let Some(market) = market {
            fills.retain(|fill| fill.market == market);
        }
        fills.sort_by_key(|fill| (fill.occurred_at, fill.fill_id));
        fills
    }

    fn reset_all_trading_state(&self) {
        for entry in &self.accounts {
            let mut account = entry.value().lock().expect("account partition lock");
            account.balances.clear();
            account.positions.clear();
            account.open_orders.clear();
            account.fills.clear();
        }
        self.settlement_journal
            .lock()
            .expect("settlement journal lock")
            .clear();
    }
}

impl InMemoryRepository {
    fn with_account<T, F>(&self, trader_id: Uuid, read: F) -> T
    where
        F: FnOnce(&AccountPartition) -> T,
    {
        let account = self.account_partition(trader_id);
        let guard = account.lock().expect("account partition lock");
        read(&guard)
    }

    fn with_account_mut<T, F>(&self, trader_id: Uuid, update: F) -> T
    where
        F: FnOnce(&mut AccountPartition) -> T,
    {
        let account = self.account_partition(trader_id);
        let mut guard = account.lock().expect("account partition lock");
        update(&mut guard)
    }

    fn account_partition(&self, trader_id: Uuid) -> Arc<Mutex<AccountPartition>> {
        self.accounts
            .entry(trader_id)
            .or_insert_with(|| Arc::new(Mutex::new(AccountPartition::default())))
            .clone()
    }
}

pub struct PostgresRepository {
    cache: InMemoryRepository,
    writer: PostgresWritePipeline,
}

impl PostgresRepository {
    pub fn new(config: &Config) -> Self {
        let mut client = Client::connect(&config.database_url, NoTls)
            .unwrap_or_else(|error| panic!("failed to connect to postgres: {error}"));
        client
            .batch_execute(POSTGRES_INITIAL_SCHEMA)
            .unwrap_or_else(|error| panic!("failed to ensure postgres schema: {error}"));

        let cache = InMemoryRepository::default();
        hydrate_cache(&mut client, &cache);

        let writer = PostgresWritePipeline::spawn(
            config.database_url.clone(),
            config.postgres_write_batch_size,
            Duration::from_millis(config.postgres_write_flush_interval_ms),
            config.postgres_write_queue_capacity,
            Duration::from_millis(config.postgres_write_retry_backoff_ms),
        );

        Self { cache, writer }
    }
}

impl StorageBackend for PostgresRepository {
    fn kind(&self) -> StorageBackendKind {
        StorageBackendKind::Postgres
    }

    fn persistence_status(&self) -> PersistenceStatus {
        self.writer.status()
    }

    fn create_user(&self, record: UserRecord) -> Result<(), StorageError> {
        self.cache.create_user(record.clone())?;
        self.writer.enqueue(PersistOp::CreateUser(record));
        Ok(())
    }

    fn list_users(&self) -> Vec<UserRecord> {
        self.cache.list_users()
    }

    fn get_user(&self, trader_id: Uuid) -> Option<UserRecord> {
        self.cache.get_user(trader_id)
    }

    fn get_user_by_username(&self, username: &str) -> Option<UserRecord> {
        self.cache.get_user_by_username(username)
    }

    fn get_user_by_api_key(&self, api_key: &str) -> Option<UserRecord> {
        self.cache.get_user_by_api_key(api_key)
    }

    fn get_exchange_controls(&self) -> ExchangeControls {
        self.cache.get_exchange_controls()
    }

    fn set_exchange_controls(&self, controls: ExchangeControls) {
        self.cache.set_exchange_controls(controls.clone());
        self.writer
            .enqueue(PersistOp::SetExchangeControls(controls));
    }

    fn list_markets(&self) -> Vec<MarketDefinition> {
        self.cache.list_markets()
    }

    fn get_market(&self, market_id: &str) -> Option<MarketDefinition> {
        self.cache.get_market(market_id)
    }

    fn upsert_market(&self, market: MarketDefinition) {
        self.cache.upsert_market(market.clone());
        self.writer.enqueue(PersistOp::UpsertMarket(market));
    }

    fn delete_market(&self, market_id: &str) -> Option<MarketDefinition> {
        let removed = self.cache.delete_market(market_id);
        if removed.is_some() {
            self.writer
                .enqueue(PersistOp::DeleteMarket(market_id.to_string()));
        }
        removed
    }

    fn append_admin_audit_log(&self, entry: AdminAuditEntry) {
        self.cache.append_admin_audit_log(entry.clone());
        self.writer.enqueue(PersistOp::AppendAdminAuditLog(entry));
    }

    fn list_admin_audit_logs(&self) -> Vec<AdminAuditEntry> {
        self.cache.list_admin_audit_logs()
    }

    fn append_admin_message(&self, entry: AdminMessageEntry) {
        self.cache.append_admin_message(entry.clone());
        self.writer.enqueue(PersistOp::AppendAdminMessage(entry));
    }

    fn list_admin_messages(&self, limit: Option<usize>) -> Vec<AdminMessageEntry> {
        self.cache.list_admin_messages(limit)
    }

    fn append_competition_snapshot(&self, snapshot: CompetitionLeaderboardSnapshot) {
        self.cache.append_competition_snapshot(snapshot.clone());
        self.writer
            .enqueue(PersistOp::AppendCompetitionSnapshot(snapshot));
    }

    fn get_competition_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> Option<CompetitionLeaderboardSnapshot> {
        self.cache.get_competition_snapshot(snapshot_id)
    }

    fn latest_competition_snapshot(
        &self,
        competition_id: &str,
    ) -> Option<CompetitionLeaderboardSnapshot> {
        self.cache.latest_competition_snapshot(competition_id)
    }

    fn list_balances(&self, trader_id: Uuid) -> Vec<Balance> {
        self.cache.list_balances(trader_id)
    }

    fn list_all_balances(&self) -> Vec<(Uuid, Vec<Balance>)> {
        self.cache.list_all_balances()
    }

    fn put_balance(&self, trader_id: Uuid, balance: Balance) {
        self.cache.put_balance(trader_id, balance.clone());
        self.writer
            .enqueue(PersistOp::PutBalance { trader_id, balance });
    }

    fn replace_balances(&self, trader_id: Uuid, balances: Vec<Balance>) {
        self.cache.replace_balances(trader_id, balances.clone());
        self.writer.enqueue(PersistOp::ReplaceBalances {
            trader_id,
            balances,
        });
    }

    fn apply_settlement_update(
        &self,
        trader_id: Uuid,
        balances: Vec<Balance>,
        journal_entries: Vec<SettlementJournalEntry>,
    ) {
        self.cache
            .apply_settlement_update(trader_id, balances.clone(), journal_entries.clone());
        self.writer.enqueue(PersistOp::ApplySettlementUpdate {
            trader_id,
            balances,
            journal_entries,
        });
    }

    fn list_settlement_journal(&self) -> Vec<SettlementJournalEntry> {
        self.cache.list_settlement_journal()
    }

    fn list_positions(&self, trader_id: Uuid) -> Vec<Position> {
        self.cache.list_positions(trader_id)
    }

    fn list_all_positions(&self) -> Vec<(Uuid, Vec<Position>)> {
        self.cache.list_all_positions()
    }

    fn get_position(&self, trader_id: Uuid, market: &str) -> Option<Position> {
        self.cache.get_position(trader_id, market)
    }

    fn upsert_position(&self, trader_id: Uuid, position: Position) {
        self.cache.upsert_position(trader_id, position.clone());
        self.writer.enqueue(PersistOp::UpsertPosition {
            trader_id,
            position,
        });
    }

    fn delete_position(&self, trader_id: Uuid, market: &str) -> Option<Position> {
        let removed = self.cache.delete_position(trader_id, market);
        if removed.is_some() {
            self.writer.enqueue(PersistOp::DeletePosition {
                trader_id,
                market: market.to_string(),
            });
        }
        removed
    }

    fn replace_positions(&self, trader_id: Uuid, positions: Vec<Position>) {
        self.cache.replace_positions(trader_id, positions.clone());
        self.writer.enqueue(PersistOp::ReplacePositions {
            trader_id,
            positions,
        });
    }

    fn upsert_order_ledger(&self, order: Order) {
        self.writer.enqueue(PersistOp::UpsertOrderLedger(order));
    }

    fn close_order_ledger(&self, trader_id: Uuid, order_id: Uuid, remaining: u64) {
        self.writer.enqueue(PersistOp::CloseOrderLedger {
            trader_id,
            order_id,
            remaining,
        });
    }

    fn list_all_open_orders(&self) -> Vec<Order> {
        self.cache.list_all_open_orders()
    }

    fn list_open_orders(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Order> {
        self.cache.list_open_orders(trader_id, market)
    }

    fn get_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order> {
        self.cache.get_open_order(trader_id, order_id)
    }

    fn upsert_open_order(&self, trader_id: Uuid, order: Order) {
        self.cache.upsert_open_order(trader_id, order.clone());
        if order.remaining != order.quantity {
            self.writer.enqueue(PersistOp::UpsertOrderLedger(order));
        }
    }

    fn delete_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order> {
        self.cache.delete_open_order(trader_id, order_id)
    }

    fn append_fill(&self, trader_id: Uuid, fill: Fill) {
        self.cache.append_fill(trader_id, fill);
    }

    fn persist_fill(&self, fill: Fill) {
        self.writer.enqueue(PersistOp::AppendFill(fill));
    }

    fn list_fills(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Fill> {
        self.cache.list_fills(trader_id, market)
    }

    fn reset_all_trading_state(&self) {
        self.cache.reset_all_trading_state();
        self.writer.enqueue(PersistOp::ResetAllTradingState);
    }
}

#[derive(Clone)]
struct PostgresWritePipeline {
    tx: Sender<PersistOp>,
    telemetry: Arc<PostgresWriteTelemetry>,
}

impl PostgresWritePipeline {
    fn spawn(
        connection_string: String,
        batch_size: usize,
        flush_interval: Duration,
        queue_capacity: usize,
        retry_backoff: Duration,
    ) -> Self {
        assert!(batch_size > 0, "postgres write batch size must be positive");
        assert!(
            queue_capacity > 0,
            "postgres write queue capacity must be positive"
        );

        let telemetry = Arc::new(PostgresWriteTelemetry::new(queue_capacity));
        let (tx, rx) = mpsc::channel();
        let writer_telemetry = telemetry.clone();
        thread::Builder::new()
            .name("exchange-postgres-writer".to_string())
            .spawn(move || {
                writer_loop(
                    connection_string,
                    batch_size,
                    flush_interval,
                    retry_backoff,
                    writer_telemetry,
                    rx,
                )
            })
            .unwrap_or_else(|error| panic!("failed to spawn postgres writer thread: {error}"));

        Self { tx, telemetry }
    }

    fn enqueue(&self, op: PersistOp) {
        self.telemetry.record_enqueue_started();
        self.tx.send(op).unwrap_or_else(|_| {
            self.telemetry
                .mark_stopped(Some("postgres writer thread terminated".to_string()));
            panic!("postgres writer thread terminated");
        });
        self.telemetry.record_enqueue_blocked(Duration::ZERO);
    }

    fn status(&self) -> PersistenceStatus {
        self.telemetry.snapshot(StorageBackendKind::Postgres)
    }
}

struct PostgresWriteTelemetry {
    queue_capacity: usize,
    backpressure_threshold: usize,
    queued_ops: AtomicUsize,
    in_flight_ops: AtomicUsize,
    high_water_mark: AtomicUsize,
    total_enqueued: AtomicU64,
    total_flushes: AtomicU64,
    total_flushed_ops: AtomicU64,
    total_blocked_enqueues: AtomicU64,
    total_enqueue_block_time_ms: AtomicU64,
    total_flush_failures: AtomicU64,
    total_retries: AtomicU64,
    last_batch_size: AtomicUsize,
    last_flush_latency_ms: AtomicU64,
    max_flush_latency_ms: AtomicU64,
    retrying: AtomicBool,
    writer_alive: AtomicBool,
    last_error: Mutex<Option<String>>,
}

impl PostgresWriteTelemetry {
    fn new(queue_capacity: usize) -> Self {
        Self {
            queue_capacity,
            backpressure_threshold: backpressure_threshold(queue_capacity),
            queued_ops: AtomicUsize::new(0),
            in_flight_ops: AtomicUsize::new(0),
            high_water_mark: AtomicUsize::new(0),
            total_enqueued: AtomicU64::new(0),
            total_flushes: AtomicU64::new(0),
            total_flushed_ops: AtomicU64::new(0),
            total_blocked_enqueues: AtomicU64::new(0),
            total_enqueue_block_time_ms: AtomicU64::new(0),
            total_flush_failures: AtomicU64::new(0),
            total_retries: AtomicU64::new(0),
            last_batch_size: AtomicUsize::new(0),
            last_flush_latency_ms: AtomicU64::new(0),
            max_flush_latency_ms: AtomicU64::new(0),
            retrying: AtomicBool::new(false),
            writer_alive: AtomicBool::new(true),
            last_error: Mutex::new(None),
        }
    }

    fn record_enqueue_started(&self) {
        self.total_enqueued.fetch_add(1, Ordering::Relaxed);
        let queue_depth = self.queued_ops.fetch_add(1, Ordering::Relaxed) + 1;
        update_max_usize(&self.high_water_mark, queue_depth);
    }

    fn record_enqueue_blocked(&self, blocked_for: Duration) {
        let blocked_ms = duration_to_millis(blocked_for);
        if blocked_ms > 0 {
            self.total_blocked_enqueues.fetch_add(1, Ordering::Relaxed);
            self.total_enqueue_block_time_ms
                .fetch_add(blocked_ms, Ordering::Relaxed);
        }
    }

    fn record_dequeued(&self) {
        self.queued_ops.fetch_sub(1, Ordering::Relaxed);
    }

    fn start_flush(&self, batch_len: usize) {
        self.last_batch_size.store(batch_len, Ordering::Relaxed);
        self.in_flight_ops.fetch_add(batch_len, Ordering::Relaxed);
    }

    fn record_flush_success(&self, batch_len: usize, latency: Duration) {
        self.in_flight_ops.fetch_sub(batch_len, Ordering::Relaxed);
        self.total_flushes.fetch_add(1, Ordering::Relaxed);
        self.total_flushed_ops
            .fetch_add(batch_len as u64, Ordering::Relaxed);
        let latency_ms = duration_to_millis(latency);
        self.last_flush_latency_ms
            .store(latency_ms, Ordering::Relaxed);
        update_max_u64(&self.max_flush_latency_ms, latency_ms);
        self.retrying.store(false, Ordering::Relaxed);
        *self.last_error.lock().expect("writer telemetry lock") = None;
    }

    fn record_flush_failure(&self, error: String) {
        self.total_flush_failures.fetch_add(1, Ordering::Relaxed);
        self.retrying.store(true, Ordering::Relaxed);
        *self.last_error.lock().expect("writer telemetry lock") = Some(error);
    }

    fn record_retry(&self) {
        self.total_retries.fetch_add(1, Ordering::Relaxed);
    }

    fn mark_stopped(&self, last_error: Option<String>) {
        self.writer_alive.store(false, Ordering::Relaxed);
        if let Some(last_error) = last_error {
            *self.last_error.lock().expect("writer telemetry lock") = Some(last_error);
        }
    }

    fn snapshot(&self, backend: StorageBackendKind) -> PersistenceStatus {
        let queue_depth = self.queued_ops.load(Ordering::Relaxed);
        let in_flight_ops = self.in_flight_ops.load(Ordering::Relaxed);
        let backlog_depth = queue_depth.saturating_add(in_flight_ops);
        let mode = if !self.writer_alive.load(Ordering::Relaxed) {
            PersistenceMode::Stopped
        } else if self.retrying.load(Ordering::Relaxed) {
            PersistenceMode::Retrying
        } else if queue_depth >= self.backpressure_threshold && queue_depth > 0 {
            PersistenceMode::Backpressured
        } else {
            PersistenceMode::Ok
        };

        PersistenceStatus {
            backend,
            mode,
            queue_capacity: self.queue_capacity,
            backpressure_threshold: self.backpressure_threshold,
            queue_depth,
            in_flight_ops,
            backlog_depth,
            high_water_mark: self.high_water_mark.load(Ordering::Relaxed),
            total_enqueued: self.total_enqueued.load(Ordering::Relaxed),
            total_flushes: self.total_flushes.load(Ordering::Relaxed),
            total_flushed_ops: self.total_flushed_ops.load(Ordering::Relaxed),
            total_blocked_enqueues: self.total_blocked_enqueues.load(Ordering::Relaxed),
            total_enqueue_block_time_ms: self.total_enqueue_block_time_ms.load(Ordering::Relaxed),
            total_flush_failures: self.total_flush_failures.load(Ordering::Relaxed),
            total_retries: self.total_retries.load(Ordering::Relaxed),
            last_batch_size: self.last_batch_size.load(Ordering::Relaxed),
            last_flush_latency_ms: self.last_flush_latency_ms.load(Ordering::Relaxed),
            max_flush_latency_ms: self.max_flush_latency_ms.load(Ordering::Relaxed),
            last_error: self
                .last_error
                .lock()
                .expect("writer telemetry lock")
                .clone(),
        }
    }
}

#[derive(Debug, Clone)]
enum PersistOp {
    CreateUser(UserRecord),
    SetExchangeControls(ExchangeControls),
    UpsertMarket(MarketDefinition),
    DeleteMarket(String),
    AppendAdminAuditLog(AdminAuditEntry),
    AppendAdminMessage(AdminMessageEntry),
    AppendCompetitionSnapshot(CompetitionLeaderboardSnapshot),
    PutBalance {
        trader_id: Uuid,
        balance: Balance,
    },
    ReplaceBalances {
        trader_id: Uuid,
        balances: Vec<Balance>,
    },
    ApplySettlementUpdate {
        trader_id: Uuid,
        balances: Vec<Balance>,
        journal_entries: Vec<SettlementJournalEntry>,
    },
    UpsertPosition {
        trader_id: Uuid,
        position: Position,
    },
    DeletePosition {
        trader_id: Uuid,
        market: String,
    },
    ReplacePositions {
        trader_id: Uuid,
        positions: Vec<Position>,
    },
    UpsertOrderLedger(Order),
    CloseOrderLedger {
        trader_id: Uuid,
        order_id: Uuid,
        remaining: u64,
    },
    AppendFill(Fill),
    ResetAllTradingState,
}

fn writer_loop(
    connection_string: String,
    batch_size: usize,
    flush_interval: Duration,
    retry_backoff: Duration,
    telemetry: Arc<PostgresWriteTelemetry>,
    rx: mpsc::Receiver<PersistOp>,
) {
    let mut client: Option<Client> = None;

    loop {
        let first = match rx.recv() {
            Ok(op) => op,
            Err(_) => break,
        };
        telemetry.record_dequeued();

        let mut batch = Vec::with_capacity(batch_size);
        batch.push(first);
        let deadline = Instant::now() + flush_interval;
        let mut disconnected = false;

        while batch.len() < batch_size {
            let timeout = deadline.saturating_duration_since(Instant::now());
            if timeout.is_zero() {
                break;
            }

            match rx.recv_timeout(timeout) {
                Ok(op) => {
                    telemetry.record_dequeued();
                    batch.push(op);
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        telemetry.start_flush(batch.len());
        let flush_started_at = Instant::now();
        loop {
            if client.is_none() {
                client = Some(connect_postgres_writer(
                    &connection_string,
                    retry_backoff,
                    &telemetry,
                ));
            }

            match flush_batch(client.as_mut().expect("postgres writer client"), &batch) {
                Ok(()) => {
                    telemetry.record_flush_success(batch.len(), flush_started_at.elapsed());
                    break;
                }
                Err(error) => {
                    warn!(error, "postgres writer flush failed; retrying batch");
                    telemetry.record_flush_failure(error);
                    telemetry.record_retry();
                    client = None;
                    thread::sleep(retry_backoff);
                }
            }
        }
        if disconnected {
            break;
        }
    }

    telemetry.mark_stopped(None);
}

fn connect_postgres_writer(
    connection_string: &str,
    retry_backoff: Duration,
    telemetry: &PostgresWriteTelemetry,
) -> Client {
    loop {
        match try_connect_postgres_writer(connection_string) {
            Ok(client) => return client,
            Err(error) => {
                warn!(error, "postgres writer could not connect; retrying");
                telemetry.record_flush_failure(error);
                telemetry.record_retry();
                thread::sleep(retry_backoff);
            }
        }
    }
}

fn try_connect_postgres_writer(connection_string: &str) -> Result<Client, String> {
    let mut client = Client::connect(connection_string, NoTls)
        .map_err(|error| format!("failed to connect postgres writer: {error}"))?;
    client
        .batch_execute(POSTGRES_INITIAL_SCHEMA)
        .map_err(|error| format!("failed to ensure postgres writer schema: {error}"))?;
    Ok(client)
}

fn flush_batch(client: &mut Client, batch: &[PersistOp]) -> Result<(), String> {
    if batch.is_empty() {
        return Ok(());
    }

    let mut tx = client
        .transaction()
        .map_err(|error| format!("postgres batch transaction failed: {error}"))?;

    for op in batch {
        apply_persist_op(&mut tx, op)?;
    }

    tx.commit()
        .map_err(|error| format!("postgres batch commit failed: {error}"))
}

fn apply_persist_op(tx: &mut Transaction<'_>, op: &PersistOp) -> Result<(), String> {
    match op {
        PersistOp::CreateUser(record) => {
            tx.execute(
                "INSERT INTO users (trader_id, username, role, created_at) VALUES ($1, $2, $3, $4)",
                &[
                    &record.profile.trader_id,
                    &record.profile.username,
                    &user_role_to_db(record.profile.role),
                    &record.profile.created_at,
                ],
            )
            .map_err(|error| format!("postgres user insert failed: {error}"))?;

            tx.execute(
                "INSERT INTO api_keys (api_key, trader_id, created_at, revoked_at) VALUES ($1, $2, $3, NULL)",
                &[
                    &record.profile.api_key,
                    &record.profile.trader_id,
                    &record.profile.created_at,
                ],
            )
            .map_err(|error| format!("postgres api key insert failed: {error}"))?;
        }
        PersistOp::SetExchangeControls(controls) => {
            tx.execute(
                "INSERT INTO exchange_controls (control_key, trading_enabled, updated_at) \
                 VALUES ('exchange', $1, $2) \
                 ON CONFLICT (control_key) DO UPDATE SET \
                   trading_enabled = EXCLUDED.trading_enabled, \
                   updated_at = EXCLUDED.updated_at",
                &[&controls.trading_enabled, &controls.updated_at],
            )
            .map_err(|error| format!("postgres exchange control upsert failed: {error}"))?;
        }
        PersistOp::UpsertMarket(market) => {
            tx.execute(
                "INSERT INTO markets \
                 (market_id, display_name, base_asset, quote_asset, tick_size, min_order_quantity, \
                  reference_price, settlement_price, status, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
                 ON CONFLICT (market_id) DO UPDATE SET \
                   display_name = EXCLUDED.display_name, \
                   base_asset = EXCLUDED.base_asset, \
                   quote_asset = EXCLUDED.quote_asset, \
                   tick_size = EXCLUDED.tick_size, \
                   min_order_quantity = EXCLUDED.min_order_quantity, \
                   reference_price = EXCLUDED.reference_price, \
                   settlement_price = EXCLUDED.settlement_price, \
                   status = EXCLUDED.status, \
                   updated_at = EXCLUDED.updated_at",
                &[
                    &market.market_id,
                    &market.display_name,
                    &market.base_asset,
                    &market.quote_asset,
                    &u64_to_i64(market.tick_size),
                    &u64_to_i64(market.min_order_quantity),
                    &market.reference_price.map(u64_to_i64),
                    &market.settlement_price.map(u64_to_i64),
                    &market_status_to_db(market.status),
                    &market.created_at,
                    &market.updated_at,
                ],
            )
            .map_err(|error| format!("postgres market upsert failed: {error}"))?;
        }
        PersistOp::DeleteMarket(market_id) => {
            tx.execute("DELETE FROM markets WHERE market_id = $1", &[market_id])
                .map_err(|error| format!("postgres market delete failed: {error}"))?;
        }
        PersistOp::AppendAdminAuditLog(entry) => {
            tx.execute(
                "INSERT INTO admin_audit_logs \
                 (audit_id, actor_username, action, target_username, target_trader_id, details, occurred_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &entry.audit_id,
                    &entry.actor_username,
                    &entry.action,
                    &entry.target_username,
                    &entry.target_trader_id,
                    &entry.details,
                    &entry.occurred_at,
                ],
            )
            .map_err(|error| format!("postgres admin audit insert failed: {error}"))?;
        }
        PersistOp::AppendAdminMessage(entry) => {
            tx.execute(
                "INSERT INTO admin_messages \
                 (message_id, target_username, target_trader_id, market_id, level, title, body, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                &[
                    &entry.message_id,
                    &entry.target_username,
                    &entry.target_trader_id,
                    &entry.market,
                    &admin_message_level_to_db(entry.level),
                    &entry.title,
                    &entry.body,
                    &entry.created_at,
                ],
            )
            .map_err(|error| format!("postgres admin message insert failed: {error}"))?;
        }
        PersistOp::AppendCompetitionSnapshot(snapshot) => {
            tx.execute(
                "INSERT INTO competition_leaderboard_snapshots \
                 (snapshot_id, competition_id, label, created_at) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (snapshot_id) DO UPDATE SET \
                   competition_id = EXCLUDED.competition_id, \
                   label = EXCLUDED.label, \
                   created_at = EXCLUDED.created_at",
                &[
                    &snapshot.snapshot_id,
                    &snapshot.competition_id,
                    &snapshot.label,
                    &snapshot.created_at,
                ],
            )
            .map_err(|error| format!("postgres competition snapshot insert failed: {error}"))?;

            tx.execute(
                "DELETE FROM competition_leaderboard_snapshot_rows WHERE snapshot_id = $1",
                &[&snapshot.snapshot_id],
            )
            .map_err(|error| format!("postgres competition snapshot row delete failed: {error}"))?;

            for row in &snapshot.leaderboard {
                tx.execute(
                    "INSERT INTO competition_leaderboard_snapshot_rows \
                     (snapshot_id, rank, trader_id, username, net_pnl, realized_pnl, unrealized_pnl, gross_exposure) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    &[
                        &snapshot.snapshot_id,
                        &i64::try_from(row.rank).map_err(|_| "competition rank overflow".to_string())?,
                        &row.trader_id,
                        &row.username,
                        &row.net_pnl,
                        &row.realized_pnl,
                        &row.unrealized_pnl,
                        &u64_to_i64(row.gross_exposure),
                    ],
                )
                .map_err(|error| format!("postgres competition snapshot row insert failed: {error}"))?;
            }
        }
        PersistOp::PutBalance { trader_id, balance } => {
            let updated_at = Utc::now();
            tx.execute(
                "INSERT INTO balances (trader_id, asset, free, locked, updated_at) \
                 VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (trader_id, asset) DO UPDATE SET \
                   free = EXCLUDED.free, \
                   locked = EXCLUDED.locked, \
                   updated_at = EXCLUDED.updated_at",
                &[
                    trader_id,
                    &balance.asset,
                    &u64_to_i64(balance.free),
                    &u64_to_i64(balance.locked),
                    &updated_at,
                ],
            )
            .map_err(|error| format!("postgres balance upsert failed: {error}"))?;
        }
        PersistOp::ReplaceBalances {
            trader_id,
            balances,
        } => {
            persist_balance_snapshot(tx, *trader_id, balances)?;
        }
        PersistOp::ApplySettlementUpdate {
            trader_id,
            balances,
            journal_entries,
        } => {
            persist_balance_snapshot(tx, *trader_id, balances)?;
            for entry in journal_entries {
                tx.execute(
                    "INSERT INTO settlement_journal \
                     (journal_id, trader_id, asset, free_delta, locked_delta, reason, order_id, fill_id, occurred_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                     ON CONFLICT (journal_id) DO NOTHING",
                    &[
                        &entry.journal_id,
                        &entry.trader_id,
                        &entry.asset,
                        &entry.free_delta,
                        &entry.locked_delta,
                        &settlement_reason_to_db(entry.reason),
                        &entry.order_id,
                        &entry.fill_id,
                        &entry.occurred_at,
                    ],
                )
                .map_err(|error| format!("postgres settlement journal insert failed: {error}"))?;
            }
        }
        PersistOp::UpsertPosition {
            trader_id,
            position,
        } => {
            let updated_at = position.updated_at;
            tx.execute(
                "INSERT INTO positions (trader_id, market, net_quantity, average_entry_price, realized_pnl, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (trader_id, market) DO UPDATE SET \
                   net_quantity = EXCLUDED.net_quantity, \
                   average_entry_price = EXCLUDED.average_entry_price, \
                   realized_pnl = EXCLUDED.realized_pnl, \
                   updated_at = EXCLUDED.updated_at",
                &[
                    trader_id,
                    &position.market,
                    &position.net_quantity,
                    &position.average_entry_price.map(u64_to_i64),
                    &position.realized_pnl,
                    &updated_at,
                ],
            )
            .map_err(|error| format!("postgres position upsert failed: {error}"))?;
        }
        PersistOp::DeletePosition { trader_id, market } => {
            tx.execute(
                "DELETE FROM positions WHERE trader_id = $1 AND market = $2",
                &[trader_id, market],
            )
            .map_err(|error| format!("postgres position delete failed: {error}"))?;
        }
        PersistOp::ReplacePositions {
            trader_id,
            positions,
        } => {
            persist_position_snapshot(tx, *trader_id, positions)?;
        }
        PersistOp::UpsertOrderLedger(order) => {
            let updated_at = Utc::now();
            tx.execute(
                "INSERT INTO orders \
                 (order_id, trader_id, market, side, price, quantity, remaining, status, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 'OPEN', $8, $9) \
                 ON CONFLICT (order_id) DO UPDATE SET \
                   trader_id = EXCLUDED.trader_id, \
                   market = EXCLUDED.market, \
                   side = EXCLUDED.side, \
                   price = EXCLUDED.price, \
                   quantity = EXCLUDED.quantity, \
                   remaining = EXCLUDED.remaining, \
                   status = 'OPEN', \
                   updated_at = EXCLUDED.updated_at",
                &[
                    &order.id,
                    &order.trader_id,
                    &order.market,
                    &side_to_db(order.side),
                    &u64_to_i64(order.price),
                    &u64_to_i64(order.quantity),
                    &u64_to_i64(order.remaining),
                    &order.created_at,
                    &updated_at,
                ],
            )
            .map_err(|error| format!("postgres order ledger upsert failed: {error}"))?;
        }
        PersistOp::CloseOrderLedger {
            trader_id,
            order_id,
            remaining,
        } => {
            let updated_at = Utc::now();
            tx.execute(
                "UPDATE orders SET status = 'CLOSED', remaining = $3, updated_at = $4 \
                 WHERE trader_id = $1 AND order_id = $2",
                &[trader_id, order_id, &u64_to_i64(*remaining), &updated_at],
            )
            .map_err(|error| format!("postgres order close failed: {error}"))?;
        }
        PersistOp::AppendFill(fill) => {
            tx.execute(
                "INSERT INTO fills \
                 (fill_id, market, maker_order_id, taker_order_id, price, quantity, occurred_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 ON CONFLICT (fill_id) DO NOTHING",
                &[
                    &fill.fill_id,
                    &fill.market,
                    &fill.maker_order_id,
                    &fill.taker_order_id,
                    &u64_to_i64(fill.price),
                    &u64_to_i64(fill.quantity),
                    &fill.occurred_at,
                ],
            )
            .map_err(|error| format!("postgres fill insert failed: {error}"))?;
        }
        PersistOp::ResetAllTradingState => {
            tx.batch_execute(
                "TRUNCATE TABLE fills, orders, positions, pending_positions, balances, settlement_journal, pnl_snapshots RESTART IDENTITY",
            )
            .map_err(|error| format!("postgres trading reset failed: {error}"))?;
        }
    }

    Ok(())
}

fn hydrate_cache(client: &mut Client, cache: &InMemoryRepository) {
    hydrate_users(client, cache);
    hydrate_exchange_controls(client, cache);
    hydrate_markets(client, cache);
    hydrate_admin_audit_logs(client, cache);
    hydrate_admin_messages(client, cache);
    hydrate_competition_snapshots(client, cache);
    hydrate_balances(client, cache);
    hydrate_settlement_journal(client, cache);
    hydrate_positions(client, cache);
    hydrate_open_orders(client, cache);
    hydrate_fills(client, cache);
}

fn hydrate_users(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT DISTINCT ON (u.trader_id) u.trader_id, u.username, u.role, u.created_at, ak.api_key \
             FROM users u \
             JOIN api_keys ak ON ak.trader_id = u.trader_id AND ak.revoked_at IS NULL \
             ORDER BY u.trader_id, ak.created_at ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres user hydrate failed: {error}"));

    for row in rows {
        cache
            .create_user(user_from_row(row))
            .unwrap_or_else(|error| panic!("postgres user hydrate conflict: {error}"));
    }
}

fn hydrate_exchange_controls(client: &mut Client, cache: &InMemoryRepository) {
    let row = client
        .query_opt(
            "SELECT trading_enabled, updated_at FROM exchange_controls WHERE control_key = 'exchange'",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres exchange controls hydrate failed: {error}"));

    if let Some(row) = row {
        cache.set_exchange_controls(exchange_controls_from_row(row));
    }
}

fn hydrate_markets(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT market_id, display_name, base_asset, quote_asset, tick_size, min_order_quantity, \
                    reference_price, settlement_price, status, created_at, updated_at \
             FROM markets \
             ORDER BY market_id ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres market hydrate failed: {error}"));

    for row in rows {
        cache.upsert_market(market_from_row(row));
    }
}

fn hydrate_admin_audit_logs(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT audit_id, actor_username, action, target_username, target_trader_id, details, occurred_at \
             FROM admin_audit_logs \
             ORDER BY occurred_at ASC, audit_id ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres admin audit hydrate failed: {error}"));

    for row in rows {
        cache.append_admin_audit_log(admin_audit_from_row(row));
    }
}

fn hydrate_admin_messages(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT message_id, target_username, target_trader_id, market_id, level, title, body, created_at \
             FROM admin_messages \
             ORDER BY created_at ASC, message_id ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres admin message hydrate failed: {error}"));

    for row in rows {
        cache.append_admin_message(admin_message_from_row(row));
    }
}

fn hydrate_competition_snapshots(client: &mut Client, cache: &InMemoryRepository) {
    let snapshot_rows = client
        .query(
            "SELECT snapshot_id, competition_id, label, created_at \
             FROM competition_leaderboard_snapshots \
             ORDER BY created_at ASC, snapshot_id ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres competition snapshot hydrate failed: {error}"));

    let row_rows = client
        .query(
            "SELECT snapshot_id, rank, trader_id, username, net_pnl, realized_pnl, unrealized_pnl, gross_exposure \
             FROM competition_leaderboard_snapshot_rows \
             ORDER BY snapshot_id ASC, rank ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres competition snapshot rows hydrate failed: {error}"));

    let mut rows_by_snapshot = BTreeMap::<Uuid, Vec<crate::admin::LeaderboardRow>>::new();
    for row in row_rows {
        let snapshot_id: Uuid = row.get("snapshot_id");
        rows_by_snapshot
            .entry(snapshot_id)
            .or_default()
            .push(crate::admin::LeaderboardRow {
                rank: usize::try_from(row.get::<_, i64>("rank"))
                    .unwrap_or_else(|_| panic!("invalid competition snapshot rank")),
                trader_id: row.get("trader_id"),
                username: row.get("username"),
                net_pnl: row.get("net_pnl"),
                realized_pnl: row.get("realized_pnl"),
                unrealized_pnl: row.get("unrealized_pnl"),
                gross_exposure: i64_to_u64(row.get("gross_exposure")),
            });
    }

    for row in snapshot_rows {
        let snapshot_id: Uuid = row.get("snapshot_id");
        let leaderboard = rows_by_snapshot.remove(&snapshot_id).unwrap_or_default();
        cache.append_competition_snapshot(CompetitionLeaderboardSnapshot {
            snapshot_id,
            competition_id: row.get("competition_id"),
            label: row.get("label"),
            created_at: row.get("created_at"),
            entrants: leaderboard.len(),
            leaderboard,
        });
    }
}

fn hydrate_balances(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT trader_id, asset, free, locked FROM balances ORDER BY trader_id ASC, asset ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres balance hydrate failed: {error}"));

    for row in rows {
        let trader_id: Uuid = row.get("trader_id");
        cache.put_balance(trader_id, balance_from_row(row));
    }
}

fn hydrate_settlement_journal(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT journal_id, trader_id, asset, free_delta, locked_delta, reason, order_id, fill_id, occurred_at \
             FROM settlement_journal \
             ORDER BY occurred_at ASC, journal_id ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres settlement journal hydrate failed: {error}"));

    let entries: Vec<_> = rows.into_iter().map(settlement_journal_from_row).collect();
    cache
        .settlement_journal
        .lock()
        .expect("settlement journal lock")
        .extend(entries);
}

fn hydrate_positions(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT trader_id, market, net_quantity, average_entry_price, realized_pnl, updated_at \
             FROM positions ORDER BY trader_id ASC, market ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres position hydrate failed: {error}"));

    for row in rows {
        let trader_id: Uuid = row.get("trader_id");
        cache.upsert_position(trader_id, position_from_row(row));
    }
}

fn hydrate_open_orders(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT order_id, trader_id, market, side, price, quantity, remaining, created_at \
             FROM orders WHERE status = 'OPEN' \
             ORDER BY created_at ASC, order_id ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres open order hydrate failed: {error}"));

    for row in rows {
        let order = order_from_row(row);
        cache.upsert_open_order(order.trader_id, order);
    }
}

fn hydrate_fills(client: &mut Client, cache: &InMemoryRepository) {
    let rows = client
        .query(
            "SELECT f.fill_id, f.market, f.maker_order_id, f.taker_order_id, f.price, f.quantity, f.occurred_at, \
                    mo.trader_id AS maker_trader_id, to2.trader_id AS taker_trader_id \
             FROM fills f \
             JOIN orders mo ON mo.order_id = f.maker_order_id \
             JOIN orders to2 ON to2.order_id = f.taker_order_id \
             ORDER BY f.occurred_at ASC, f.fill_id ASC",
            &[],
        )
        .unwrap_or_else(|error| panic!("postgres fill hydrate failed: {error}"));

    for row in rows {
        let maker_trader_id: Uuid = row.get("maker_trader_id");
        let taker_trader_id: Uuid = row.get("taker_trader_id");
        let fill = fill_from_row(row);
        cache.append_fill(maker_trader_id, fill.clone());
        if taker_trader_id != maker_trader_id {
            cache.append_fill(taker_trader_id, fill);
        }
    }
}

fn user_from_row(row: Row) -> UserRecord {
    UserRecord {
        profile: UserProfile {
            trader_id: row.get("trader_id"),
            username: row.get("username"),
            api_key: row.get("api_key"),
            role: user_role_from_db(&row.get::<_, String>("role")),
            created_at: row.get("created_at"),
        },
    }
}

fn exchange_controls_from_row(row: Row) -> ExchangeControls {
    ExchangeControls {
        trading_enabled: row.get("trading_enabled"),
        updated_at: row.get("updated_at"),
    }
}

fn market_from_row(row: Row) -> MarketDefinition {
    MarketDefinition {
        market_id: row.get("market_id"),
        display_name: row.get("display_name"),
        base_asset: row.get("base_asset"),
        quote_asset: row.get("quote_asset"),
        tick_size: i64_to_u64(row.get("tick_size")),
        min_order_quantity: i64_to_u64(row.get("min_order_quantity")),
        reference_price: row.get::<_, Option<i64>>("reference_price").map(i64_to_u64),
        settlement_price: row
            .get::<_, Option<i64>>("settlement_price")
            .map(i64_to_u64),
        status: market_status_from_db(&row.get::<_, String>("status")),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn admin_audit_from_row(row: Row) -> AdminAuditEntry {
    AdminAuditEntry {
        audit_id: row.get("audit_id"),
        actor_username: row.get("actor_username"),
        action: row.get("action"),
        target_username: row.get("target_username"),
        target_trader_id: row.get("target_trader_id"),
        details: row.get("details"),
        occurred_at: row.get("occurred_at"),
    }
}

fn admin_message_from_row(row: Row) -> AdminMessageEntry {
    AdminMessageEntry {
        message_id: row.get("message_id"),
        target_username: row.get("target_username"),
        target_trader_id: row.get("target_trader_id"),
        market: row.get("market_id"),
        level: admin_message_level_from_db(&row.get::<_, String>("level")),
        title: row.get("title"),
        body: row.get("body"),
        created_at: row.get("created_at"),
    }
}

fn balance_from_row(row: Row) -> Balance {
    Balance {
        asset: row.get("asset"),
        free: i64_to_u64(row.get("free")),
        locked: i64_to_u64(row.get("locked")),
    }
}

fn position_from_row(row: Row) -> Position {
    Position {
        market: row.get("market"),
        net_quantity: row.get("net_quantity"),
        average_entry_price: row
            .get::<_, Option<i64>>("average_entry_price")
            .map(i64_to_u64),
        realized_pnl: row.get("realized_pnl"),
        updated_at: row.get("updated_at"),
    }
}

fn order_from_row(row: Row) -> Order {
    Order {
        id: row.get("order_id"),
        trader_id: row.get("trader_id"),
        market: row.get("market"),
        side: side_from_db(&row.get::<_, String>("side")),
        price: i64_to_u64(row.get("price")),
        quantity: i64_to_u64(row.get("quantity")),
        remaining: i64_to_u64(row.get("remaining")),
        created_at: row.get("created_at"),
    }
}

fn fill_from_row(row: Row) -> Fill {
    Fill {
        fill_id: row.get("fill_id"),
        market: row.get("market"),
        maker_order_id: row.get("maker_order_id"),
        taker_order_id: row.get("taker_order_id"),
        price: i64_to_u64(row.get("price")),
        quantity: i64_to_u64(row.get("quantity")),
        occurred_at: row.get("occurred_at"),
    }
}

fn settlement_journal_from_row(row: Row) -> SettlementJournalEntry {
    SettlementJournalEntry {
        journal_id: row.get("journal_id"),
        trader_id: row.get("trader_id"),
        asset: row.get("asset"),
        free_delta: row.get("free_delta"),
        locked_delta: row.get("locked_delta"),
        reason: settlement_reason_from_db(&row.get::<_, String>("reason")),
        order_id: row.get("order_id"),
        fill_id: row.get("fill_id"),
        occurred_at: row.get("occurred_at"),
    }
}

fn side_to_db(side: Side) -> &'static str {
    match side {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

fn side_from_db(side: &str) -> Side {
    match side {
        "BUY" => Side::Buy,
        "SELL" => Side::Sell,
        other => panic!("unsupported order side in storage: {other}"),
    }
}

fn settlement_reason_to_db(reason: SettlementJournalReason) -> &'static str {
    match reason {
        SettlementJournalReason::BalanceSeeded => "BALANCE_SEEDED",
        SettlementJournalReason::OrderHoldLocked => "ORDER_HOLD_LOCKED",
        SettlementJournalReason::OrderHoldReleased => "ORDER_HOLD_RELEASED",
        SettlementJournalReason::FillSettled => "FILL_SETTLED",
        SettlementJournalReason::MarketSettled => "MARKET_SETTLED",
    }
}

fn settlement_reason_from_db(reason: &str) -> SettlementJournalReason {
    match reason {
        "BALANCE_SEEDED" => SettlementJournalReason::BalanceSeeded,
        "ORDER_HOLD_LOCKED" => SettlementJournalReason::OrderHoldLocked,
        "ORDER_HOLD_RELEASED" => SettlementJournalReason::OrderHoldReleased,
        "FILL_SETTLED" => SettlementJournalReason::FillSettled,
        "MARKET_SETTLED" => SettlementJournalReason::MarketSettled,
        other => panic!("unsupported settlement reason in storage: {other}"),
    }
}

fn market_status_to_db(status: MarketStatus) -> &'static str {
    match status {
        MarketStatus::Enabled => "ENABLED",
        MarketStatus::Disabled => "DISABLED",
        MarketStatus::Settled => "SETTLED",
    }
}

fn market_status_from_db(status: &str) -> MarketStatus {
    match status {
        "ENABLED" => MarketStatus::Enabled,
        "DISABLED" => MarketStatus::Disabled,
        "SETTLED" => MarketStatus::Settled,
        other => panic!("unsupported market status in storage: {other}"),
    }
}

fn admin_message_level_to_db(level: AdminMessageLevel) -> &'static str {
    match level {
        AdminMessageLevel::Info => "INFO",
        AdminMessageLevel::Warning => "WARNING",
        AdminMessageLevel::Critical => "CRITICAL",
    }
}

fn admin_message_level_from_db(level: &str) -> AdminMessageLevel {
    match level {
        "INFO" => AdminMessageLevel::Info,
        "WARNING" => AdminMessageLevel::Warning,
        "CRITICAL" => AdminMessageLevel::Critical,
        other => panic!("unsupported admin message level in storage: {other}"),
    }
}

fn user_role_to_db(role: UserRole) -> &'static str {
    match role {
        UserRole::Trader => "TRADER",
        UserRole::Admin => "ADMIN",
    }
}

fn user_role_from_db(role: &str) -> UserRole {
    match role {
        "TRADER" => UserRole::Trader,
        "ADMIN" => UserRole::Admin,
        other => panic!("unsupported user role in storage: {other}"),
    }
}

fn persist_balance_snapshot(
    tx: &mut Transaction<'_>,
    trader_id: Uuid,
    balances: &[Balance],
) -> Result<(), String> {
    tx.execute("DELETE FROM balances WHERE trader_id = $1", &[&trader_id])
        .map_err(|error| format!("postgres balance delete failed: {error}"))?;

    let updated_at = Utc::now();
    for balance in balances {
        tx.execute(
            "INSERT INTO balances (trader_id, asset, free, locked, updated_at) \
             VALUES ($1, $2, $3, $4, $5)",
            &[
                &trader_id,
                &balance.asset,
                &u64_to_i64(balance.free),
                &u64_to_i64(balance.locked),
                &updated_at,
            ],
        )
        .map_err(|error| format!("postgres balance insert failed: {error}"))?;
    }

    Ok(())
}

fn persist_position_snapshot(
    tx: &mut Transaction<'_>,
    trader_id: Uuid,
    positions: &[Position],
) -> Result<(), String> {
    tx.execute("DELETE FROM positions WHERE trader_id = $1", &[&trader_id])
        .map_err(|error| format!("postgres position delete failed: {error}"))?;

    for position in positions {
        tx.execute(
            "INSERT INTO positions (trader_id, market, net_quantity, average_entry_price, realized_pnl, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &trader_id,
                &position.market,
                &position.net_quantity,
                &position.average_entry_price.map(u64_to_i64),
                &position.realized_pnl,
                &position.updated_at,
            ],
        )
        .map_err(|error| format!("postgres position insert failed: {error}"))?;
    }

    Ok(())
}

fn i64_to_u64(value: i64) -> u64 {
    u64::try_from(value).expect("expected non-negative bigint value")
}

fn u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).expect("expected bigint-compatible value")
}

fn backpressure_threshold(queue_capacity: usize) -> usize {
    usize::max(
        1,
        (queue_capacity.saturating_mul(DEFAULT_POSTGRES_BACKPRESSURE_PERCENT) + 99) / 100,
    )
}

fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
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
    use chrono::Utc;
    use std::time::Duration;

    fn user_record(username: &str, api_key: &str) -> UserRecord {
        UserRecord {
            profile: UserProfile {
                trader_id: Uuid::new_v4(),
                username: username.to_string(),
                api_key: api_key.to_string(),
                role: UserRole::Trader,
                created_at: Utc::now(),
            },
        }
    }

    #[test]
    fn in_memory_backend_is_default_repository_mode() {
        let repository = StorageRepository::new_in_memory();
        assert_eq!(repository.kind(), StorageBackendKind::InMemory);
    }

    #[test]
    fn repository_can_be_selected_from_config() {
        let repository = StorageRepository::from_config(&Config {
            bind_addr: "127.0.0.1:0".to_string(),
            database_url: "postgres://unused".to_string(),
            storage_backend: StorageBackendKind::InMemory,
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
            admin_api_token: "admin-token".to_string(),
            postgres_write_batch_size: 128,
            postgres_write_flush_interval_ms: 25,
            postgres_write_queue_capacity: 4_096,
            postgres_write_retry_backoff_ms: 250,
        });

        assert_eq!(repository.kind(), StorageBackendKind::InMemory);
    }

    #[test]
    fn user_identity_lookups_round_trip() {
        let repository = StorageRepository::new_in_memory();
        let record = user_record("alice", "exch_alice");
        let trader_id = record.profile.trader_id;
        repository.create_user(record).expect("user");

        assert_eq!(
            repository
                .get_user_by_username("alice")
                .expect("user")
                .profile
                .trader_id,
            trader_id
        );
        assert_eq!(
            repository
                .get_user_by_api_key("exch_alice")
                .expect("api key")
                .profile
                .trader_id,
            trader_id
        );
    }

    #[test]
    fn admin_audit_logs_round_trip() {
        let repository = StorageRepository::new_in_memory();
        repository.append_admin_audit_log(AdminAuditEntry {
            audit_id: Uuid::new_v4(),
            actor_username: "ops-admin".to_string(),
            action: "provision_competition_user_succeeded".to_string(),
            target_username: Some("alice".to_string()),
            target_trader_id: Some(Uuid::new_v4()),
            details: "competition account provisioned".to_string(),
            occurred_at: Utc::now(),
        });

        assert_eq!(repository.list_admin_audit_logs().len(), 1);
    }

    #[test]
    fn in_memory_persistence_status_is_disabled() {
        let repository = StorageRepository::new_in_memory();
        let status = repository.persistence_status();

        assert_eq!(status.backend, StorageBackendKind::InMemory);
        assert_eq!(status.mode, PersistenceMode::Disabled);
        assert_eq!(status.queue_depth, 0);
        assert_eq!(status.total_flushes, 0);
    }

    #[test]
    fn account_queries_are_scoped_by_trader_and_market() {
        let repository = StorageRepository::new_in_memory();
        let trader_id = Uuid::new_v4();
        let first = Order {
            id: Uuid::new_v4(),
            trader_id,
            market: "BTC-USD".to_string(),
            side: Side::Buy,
            price: 100,
            quantity: 1,
            remaining: 1,
            created_at: Utc::now(),
        };
        let second = Order {
            id: Uuid::new_v4(),
            trader_id,
            market: "ETH-USD".to_string(),
            side: Side::Sell,
            price: 200,
            quantity: 2,
            remaining: 2,
            created_at: Utc::now(),
        };

        repository.upsert_open_order(trader_id, first);
        repository.upsert_open_order(trader_id, second);

        assert_eq!(repository.list_open_orders(trader_id, None).len(), 2);
        assert_eq!(
            repository
                .list_open_orders(trader_id, Some("BTC-USD"))
                .len(),
            1
        );
    }

    #[test]
    fn duplicate_fill_is_deduplicated_per_trader() {
        let repository = StorageRepository::new_in_memory();
        let trader_id = Uuid::new_v4();
        let fill = Fill {
            fill_id: Uuid::new_v4(),
            market: "BTC-USD".to_string(),
            maker_order_id: Uuid::new_v4(),
            taker_order_id: Uuid::new_v4(),
            price: 100,
            quantity: 1,
            occurred_at: Utc::now(),
        };

        repository.append_fill(trader_id, fill.clone());
        repository.append_fill(trader_id, fill);

        assert_eq!(repository.list_fills(trader_id, None).len(), 1);
    }

    #[test]
    fn postgres_schema_mentions_all_core_tables() {
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS users"));
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS api_keys"));
        assert!(!POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS sessions"));
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS admin_audit_logs"));
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS balances"));
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS positions"));
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS pending_positions"));
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS orders"));
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS fills"));
        assert!(POSTGRES_INITIAL_SCHEMA.contains("CREATE TABLE IF NOT EXISTS pnl_snapshots"));
        assert!(
            POSTGRES_INITIAL_SCHEMA
                .contains("CREATE TABLE IF NOT EXISTS competition_leaderboard_snapshots")
        );
        assert!(
            POSTGRES_INITIAL_SCHEMA
                .contains("CREATE TABLE IF NOT EXISTS competition_leaderboard_snapshot_rows")
        );
    }

    #[test]
    fn postgres_backend_kind_is_exposed() {
        assert_eq!(StorageBackendKind::Postgres, StorageBackendKind::Postgres);
    }

    #[test]
    fn writer_telemetry_tracks_backpressure_and_retry_state() {
        let telemetry = PostgresWriteTelemetry::new(10);

        for _ in 0..8 {
            telemetry.record_enqueue_started();
        }
        let backpressured = telemetry.snapshot(StorageBackendKind::Postgres);
        assert_eq!(backpressured.mode, PersistenceMode::Backpressured);
        assert_eq!(backpressured.queue_depth, 8);
        assert_eq!(backpressured.backpressure_threshold, 8);

        for _ in 0..8 {
            telemetry.record_dequeued();
        }
        telemetry.start_flush(3);
        telemetry.record_flush_failure("db unavailable".to_string());
        let retrying = telemetry.snapshot(StorageBackendKind::Postgres);
        assert_eq!(retrying.mode, PersistenceMode::Retrying);
        assert_eq!(retrying.in_flight_ops, 3);
        assert_eq!(retrying.total_flush_failures, 1);
        assert_eq!(retrying.last_error.as_deref(), Some("db unavailable"));

        telemetry.record_retry();
        telemetry.record_flush_success(3, Duration::from_millis(12));
        let recovered = telemetry.snapshot(StorageBackendKind::Postgres);
        assert_eq!(recovered.mode, PersistenceMode::Ok);
        assert_eq!(recovered.in_flight_ops, 0);
        assert_eq!(recovered.total_retries, 1);
        assert_eq!(recovered.total_flushes, 1);
        assert_eq!(recovered.total_flushed_ops, 3);
        assert_eq!(recovered.last_flush_latency_ms, 12);
        assert_eq!(recovered.last_error, None);
    }
}
