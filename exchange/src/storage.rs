use crate::accounts::UserRecord;
use crate::admin::{
    AdminAuditEntry, AdminMessageEntry, CompetitionLeaderboardSnapshot, ExchangeControls,
    MarketDefinition,
};
use crate::orderbook::{Fill, Order};
use crate::settlement::SettlementJournalEntry;
use crate::state::{Balance, Position};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackendKind {
    InMemory,
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
    fn list_competition_snapshots(&self) -> Vec<CompetitionLeaderboardSnapshot>;
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
    /// Secondary durable ledger row (e.g. SQL `orders` table). In-memory backends already store
    /// open orders and fills on the account partition; this hook may be a no-op there.
    fn upsert_order_ledger(&self, order: Order);
    /// Companion to `upsert_order_ledger` when an order closes or shrinks.
    fn close_order_ledger(&self, trader_id: Uuid, order_id: Uuid, remaining: u64);
    fn list_all_open_orders(&self) -> Vec<Order>;
    fn list_open_orders(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Order>;
    fn get_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order>;
    fn upsert_open_order(&self, trader_id: Uuid, order: Order);
    fn delete_open_order(&self, trader_id: Uuid, order_id: Uuid) -> Option<Order>;
    fn close_open_orders_for_market(&self, market: &str) -> usize;
    fn append_fill(&self, trader_id: Uuid, fill: Fill);
    /// Extra durable fill record beyond per-account `append_fill`; may be a no-op in memory.
    fn persist_fill(&self, fill: Fill);
    fn list_fills(&self, trader_id: Uuid, market: Option<&str>) -> Vec<Fill>;
    fn reset_all_trading_state(&self);
}

#[derive(Clone)]
pub struct StorageRepository {
    backend: Arc<dyn StorageBackend>,
}

impl StorageRepository {
    pub fn new_in_memory() -> Self {
        Self {
            backend: Arc::new(InMemoryRepository::default()),
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

    pub fn list_competition_snapshots(&self) -> Vec<CompetitionLeaderboardSnapshot> {
        self.backend.list_competition_snapshots()
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

    pub fn close_open_orders_for_market(&self, market: &str) -> usize {
        self.backend.close_open_orders_for_market(market)
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
        markets.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.market_id.cmp(&right.market_id))
        });
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

    fn list_competition_snapshots(&self) -> Vec<CompetitionLeaderboardSnapshot> {
        let mut snapshots = self
            .competition_snapshots
            .lock()
            .expect("competition snapshots lock")
            .clone();
        snapshots.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.snapshot_id.cmp(&right.snapshot_id))
        });
        snapshots
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

    fn upsert_order_ledger(&self, _order: Order) {
        // Intentionally empty: open orders and fills live on `AccountData` only (see `upsert_open_order` / `append_fill`).
    }

    fn close_order_ledger(&self, _trader_id: Uuid, _order_id: Uuid, _remaining: u64) {
        // Intentionally empty: same as `upsert_order_ledger`.
    }

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

    fn close_open_orders_for_market(&self, market: &str) -> usize {
        let mut closed = 0;
        for entry in &self.accounts {
            let mut account = entry.value().lock().expect("account partition lock");
            let before = account.open_orders.len();
            account
                .open_orders
                .retain(|_, order| order.market != market);
            closed += before.saturating_sub(account.open_orders.len());
        }
        closed
    }

    fn append_fill(&self, trader_id: Uuid, fill: Fill) {
        self.with_account_mut(trader_id, |account| {
            account.fills.insert(fill.fill_id, fill);
        });
    }

    fn persist_fill(&self, _fill: Fill) {
        // Intentionally empty: `append_fill` is authoritative for in-memory fills.
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{UserProfile, UserRole};
    use crate::orderbook::Side;
    use chrono::Utc;

    fn user_record(username: &str, api_key: &str) -> UserRecord {
        UserRecord {
            profile: UserProfile {
                trader_id: Uuid::new_v4(),
                username: username.to_string(),
                team_number: username.to_string(),
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
    fn close_open_orders_for_market_only_removes_target_market_orders() {
        let repository = StorageRepository::new_in_memory();
        let first_trader = Uuid::new_v4();
        let second_trader = Uuid::new_v4();
        repository.upsert_open_order(
            first_trader,
            Order {
                id: Uuid::new_v4(),
                trader_id: first_trader,
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 1,
                remaining: 1,
                created_at: Utc::now(),
            },
        );
        repository.upsert_open_order(
            first_trader,
            Order {
                id: Uuid::new_v4(),
                trader_id: first_trader,
                market: "ETH-USD".to_string(),
                side: Side::Sell,
                price: 200,
                quantity: 2,
                remaining: 2,
                created_at: Utc::now(),
            },
        );
        repository.upsert_open_order(
            second_trader,
            Order {
                id: Uuid::new_v4(),
                trader_id: second_trader,
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                price: 101,
                quantity: 3,
                remaining: 3,
                created_at: Utc::now(),
            },
        );

        assert_eq!(repository.close_open_orders_for_market("BTC-USD"), 2);
        assert_eq!(repository.list_all_open_orders().len(), 1);
        assert_eq!(
            repository
                .list_open_orders(first_trader, Some("ETH-USD"))
                .len(),
            1
        );
        assert!(
            repository
                .list_open_orders(first_trader, Some("BTC-USD"))
                .is_empty()
        );
        assert!(
            repository
                .list_open_orders(second_trader, Some("BTC-USD"))
                .is_empty()
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
}
