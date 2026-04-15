use crate::accounts::{UserProfile, UserRecord};
use crate::admin::{CompetitionLeaderboardSnapshot, ExchangeControls, MarketDefinition};
use crate::state::{AppState, Balance, Position};
use crate::storage::{PersistenceMode, PersistenceStatus, StorageBackendKind, StorageRepository};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use uuid::Uuid;

const CHECKPOINT_VERSION: u32 = 1;
const CHECKPOINT_SIGNAL_CAPACITY: usize = 1;

#[derive(Debug, Serialize, Deserialize)]
struct RuntimeCheckpoint {
    version: u32,
    saved_at: DateTime<Utc>,
    users: Vec<UserProfile>,
    exchange_controls: ExchangeControls,
    markets: Vec<MarketDefinition>,
    balances: Vec<TraderBalances>,
    positions: Vec<TraderPositions>,
    competition_snapshots: Vec<CompetitionLeaderboardSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TraderBalances {
    trader_id: Uuid,
    balances: Vec<Balance>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TraderPositions {
    trader_id: Uuid,
    positions: Vec<Position>,
}

#[derive(Clone)]
pub(crate) struct CheckpointHandle {
    tx: SyncSender<CheckpointCommand>,
    telemetry: Arc<CheckpointTelemetry>,
}

enum CheckpointCommand {
    Save,
}

struct CheckpointTelemetry {
    pending_requests: AtomicUsize,
    in_flight_ops: AtomicUsize,
    high_water_mark: AtomicUsize,
    total_enqueued: AtomicU64,
    total_flushes: AtomicU64,
    total_flushed_ops: AtomicU64,
    total_flush_failures: AtomicU64,
    total_retries: AtomicU64,
    last_batch_size: AtomicUsize,
    last_flush_latency_ms: AtomicU64,
    max_flush_latency_ms: AtomicU64,
    worker_alive: AtomicBool,
    retrying: AtomicBool,
    last_error: Mutex<Option<String>>,
}

impl CheckpointHandle {
    pub(crate) fn load_into_storage(
        storage: &StorageRepository,
        path: &Path,
    ) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }

        let file = File::open(path)
            .map_err(|error| format!("failed to open checkpoint {}: {error}", path.display()))?;
        let checkpoint: RuntimeCheckpoint = serde_json::from_reader(file)
            .map_err(|error| format!("failed to decode checkpoint {}: {error}", path.display()))?;
        if checkpoint.version != CHECKPOINT_VERSION {
            return Err(format!(
                "unsupported checkpoint version {} in {}",
                checkpoint.version,
                path.display()
            ));
        }

        for profile in checkpoint.users {
            storage
                .create_user(UserRecord { profile })
                .map_err(|error| format!("failed to load user from checkpoint: {error}"))?;
        }
        storage.set_exchange_controls(checkpoint.exchange_controls);
        for market in checkpoint.markets {
            storage.upsert_market(market);
        }
        for balances in checkpoint.balances {
            storage.replace_balances(balances.trader_id, balances.balances);
        }
        for positions in checkpoint.positions {
            storage.replace_positions(positions.trader_id, positions.positions);
        }
        for snapshot in checkpoint.competition_snapshots {
            storage.append_competition_snapshot(snapshot);
        }

        Ok(true)
    }

    pub(crate) fn spawn(state: AppState, path: PathBuf, interval: Duration) -> Self {
        let telemetry = Arc::new(CheckpointTelemetry::new(path.clone()));
        let worker_telemetry = telemetry.clone();
        let (tx, rx) = mpsc::sync_channel(CHECKPOINT_SIGNAL_CAPACITY);
        thread::Builder::new()
            .name("exchange-checkpoint-writer".to_string())
            .spawn(move || {
                checkpoint_loop(state, path, interval, worker_telemetry, rx);
            })
            .unwrap_or_else(|error| panic!("failed to spawn checkpoint writer thread: {error}"));

        Self { tx, telemetry }
    }

    pub(crate) fn request_save(&self) {
        self.telemetry.prepare_request_enqueue();
        match self.tx.try_send(CheckpointCommand::Save) {
            Ok(()) => self.telemetry.record_request_enqueued(),
            Err(TrySendError::Full(_)) => self.telemetry.cancel_request_enqueue(),
            Err(TrySendError::Disconnected(_)) => {
                self.telemetry.cancel_request_enqueue_and_mark_stopped(
                    "checkpoint writer thread terminated".to_string(),
                )
            }
        }
    }

    pub(crate) fn status(&self) -> PersistenceStatus {
        self.telemetry.snapshot()
    }
}

impl CheckpointTelemetry {
    fn new(_path: PathBuf) -> Self {
        Self {
            pending_requests: AtomicUsize::new(0),
            in_flight_ops: AtomicUsize::new(0),
            high_water_mark: AtomicUsize::new(0),
            total_enqueued: AtomicU64::new(0),
            total_flushes: AtomicU64::new(0),
            total_flushed_ops: AtomicU64::new(0),
            total_flush_failures: AtomicU64::new(0),
            total_retries: AtomicU64::new(0),
            last_batch_size: AtomicUsize::new(0),
            last_flush_latency_ms: AtomicU64::new(0),
            max_flush_latency_ms: AtomicU64::new(0),
            worker_alive: AtomicBool::new(true),
            retrying: AtomicBool::new(false),
            last_error: Mutex::new(None),
        }
    }

    fn prepare_request_enqueue(&self) {
        let depth = self.pending_requests.fetch_add(1, Ordering::Relaxed) + 1;
        update_max_usize(&self.high_water_mark, depth);
    }

    fn record_request_enqueued(&self) {
        self.total_enqueued.fetch_add(1, Ordering::Relaxed);
    }

    fn cancel_request_enqueue(&self) {
        self.pending_requests.fetch_sub(1, Ordering::Relaxed);
    }

    fn cancel_request_enqueue_and_mark_stopped(&self, last_error: String) {
        self.cancel_request_enqueue();
        self.mark_stopped(Some(last_error));
    }

    fn record_request_dequeued(&self) {
        self.pending_requests.fetch_sub(1, Ordering::Relaxed);
    }

    fn start_flush(&self) {
        self.in_flight_ops.store(1, Ordering::Relaxed);
        self.last_batch_size.store(1, Ordering::Relaxed);
    }

    fn record_flush_success(&self, latency: Duration) {
        self.in_flight_ops.store(0, Ordering::Relaxed);
        self.total_flushes.fetch_add(1, Ordering::Relaxed);
        self.total_flushed_ops.fetch_add(1, Ordering::Relaxed);
        let latency_ms = duration_to_millis(latency);
        self.last_flush_latency_ms
            .store(latency_ms, Ordering::Relaxed);
        update_max_u64(&self.max_flush_latency_ms, latency_ms);
        self.retrying.store(false, Ordering::Relaxed);
        *self.last_error.lock().expect("checkpoint telemetry lock") = None;
    }

    fn record_flush_failure(&self, error: String) {
        self.in_flight_ops.store(0, Ordering::Relaxed);
        self.total_flush_failures.fetch_add(1, Ordering::Relaxed);
        self.total_retries.fetch_add(1, Ordering::Relaxed);
        self.retrying.store(true, Ordering::Relaxed);
        *self.last_error.lock().expect("checkpoint telemetry lock") = Some(error);
    }

    fn mark_stopped(&self, last_error: Option<String>) {
        self.worker_alive.store(false, Ordering::Relaxed);
        if let Some(last_error) = last_error {
            *self.last_error.lock().expect("checkpoint telemetry lock") = Some(last_error);
        }
    }

    fn snapshot(&self) -> PersistenceStatus {
        let queue_depth = self.pending_requests.load(Ordering::Relaxed);
        let in_flight_ops = self.in_flight_ops.load(Ordering::Relaxed);
        let backlog_depth = queue_depth.saturating_add(in_flight_ops);
        let mode = if !self.worker_alive.load(Ordering::Relaxed) {
            PersistenceMode::Stopped
        } else if self.retrying.load(Ordering::Relaxed) {
            PersistenceMode::Retrying
        } else {
            PersistenceMode::Ok
        };

        PersistenceStatus {
            backend: StorageBackendKind::InMemory,
            mode,
            queue_capacity: CHECKPOINT_SIGNAL_CAPACITY,
            backpressure_threshold: CHECKPOINT_SIGNAL_CAPACITY,
            queue_depth,
            in_flight_ops,
            backlog_depth,
            high_water_mark: self.high_water_mark.load(Ordering::Relaxed),
            total_enqueued: self.total_enqueued.load(Ordering::Relaxed),
            total_flushes: self.total_flushes.load(Ordering::Relaxed),
            total_flushed_ops: self.total_flushed_ops.load(Ordering::Relaxed),
            total_blocked_enqueues: 0,
            total_enqueue_block_time_ms: 0,
            total_flush_failures: self.total_flush_failures.load(Ordering::Relaxed),
            total_retries: self.total_retries.load(Ordering::Relaxed),
            last_batch_size: self.last_batch_size.load(Ordering::Relaxed),
            last_flush_latency_ms: self.last_flush_latency_ms.load(Ordering::Relaxed),
            max_flush_latency_ms: self.max_flush_latency_ms.load(Ordering::Relaxed),
            last_error: self
                .last_error
                .lock()
                .expect("checkpoint telemetry lock")
                .clone(),
        }
    }
}

fn checkpoint_loop(
    state: AppState,
    path: PathBuf,
    interval: Duration,
    telemetry: Arc<CheckpointTelemetry>,
    rx: mpsc::Receiver<CheckpointCommand>,
) {
    loop {
        match rx.recv_timeout(interval) {
            Ok(CheckpointCommand::Save) => {
                telemetry.record_request_dequeued();
                save_checkpoint(&state, &path, &telemetry);
            }
            Err(RecvTimeoutError::Timeout) => save_checkpoint(&state, &path, &telemetry),
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    telemetry.mark_stopped(None);
}

fn save_checkpoint(state: &AppState, path: &Path, telemetry: &CheckpointTelemetry) {
    telemetry.start_flush();
    let started_at = Instant::now();

    let result = state
        .account_dispatch_barrier()
        .blocking_recv()
        .map_err(|_| "account dispatcher barrier failed".to_string())
        .and_then(|_| write_checkpoint(path, snapshot_runtime_state(&state.storage)));

    match result {
        Ok(()) => {
            telemetry.record_flush_success(started_at.elapsed());
            debug!(path = %path.display(), "checkpoint saved");
        }
        Err(error) => {
            warn!(path = %path.display(), error, "checkpoint save failed");
            telemetry.record_flush_failure(error);
        }
    }
}

fn snapshot_runtime_state(storage: &StorageRepository) -> RuntimeCheckpoint {
    RuntimeCheckpoint {
        version: CHECKPOINT_VERSION,
        saved_at: Utc::now(),
        users: storage
            .list_users()
            .into_iter()
            .map(|record| record.profile)
            .collect(),
        exchange_controls: storage.get_exchange_controls(),
        markets: storage.list_markets(),
        balances: storage
            .list_all_balances()
            .into_iter()
            .map(|(trader_id, balances)| TraderBalances {
                trader_id,
                balances,
            })
            .collect(),
        positions: storage
            .list_all_positions()
            .into_iter()
            .map(|(trader_id, positions)| TraderPositions {
                trader_id,
                positions,
            })
            .collect(),
        competition_snapshots: storage.list_competition_snapshots(),
    }
}

fn write_checkpoint(path: &Path, checkpoint: RuntimeCheckpoint) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create checkpoint directory {}: {error}",
            parent.display()
        )
    })?;

    let temp_path = path.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));
    let file = File::create(&temp_path).map_err(|error| {
        format!(
            "failed to create checkpoint temp file {}: {error}",
            temp_path.display()
        )
    })?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &checkpoint).map_err(|error| {
        format!(
            "failed to serialize checkpoint {}: {error}",
            temp_path.display()
        )
    })?;
    writer.flush().map_err(|error| {
        format!(
            "failed to flush checkpoint {}: {error}",
            temp_path.display()
        )
    })?;
    let file = writer.into_inner().map_err(|error| {
        format!(
            "failed to finalize checkpoint {}: {error}",
            temp_path.display()
        )
    })?;
    file.sync_all()
        .map_err(|error| format!("failed to sync checkpoint {}: {error}", temp_path.display()))?;

    fs::rename(&temp_path, path).map_err(|error| {
        format!(
            "failed to atomically replace checkpoint {}: {error}",
            path.display()
        )
    })?;

    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }

    Ok(())
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
    use crate::accounts::UserRole;
    use crate::admin::LeaderboardRow;
    use crate::config::Config;

    fn test_config(path: Option<String>) -> Config {
        Config {
            bind_addr: "127.0.0.1:0".to_string(),
            checkpoint_path: path,
            checkpoint_interval_seconds: 3_600,
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
        }
    }

    #[tokio::test]
    async fn app_state_recovers_users_positions_and_snapshots_from_checkpoint() {
        let path =
            std::env::temp_dir().join(format!("exchange-checkpoint-{}.json", Uuid::new_v4()));
        let path_string = path.to_string_lossy().to_string();

        let state = AppState::new(test_config(Some(path_string.clone())));
        let provisioned = crate::auth::AuthService::provision_user_as_admin(
            &state,
            &crate::auth::AuthenticatedAdmin {
                username: "admin-token".to_string(),
            },
            crate::auth::ProvisionUserRequest {
                username: "alice".to_string(),
                team_number: None,
                role: Some(UserRole::Trader),
            },
        )
        .expect("provision user");
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
            reference_price: Some(100),
            settlement_price: None,
            status: crate::admin::MarketStatus::Enabled,
            created_at: now,
            updated_at: now,
        });
        state.storage.replace_positions(
            provisioned.profile.trader_id,
            vec![Position {
                market: "BTC-USD".to_string(),
                net_quantity: 5,
                average_entry_price: Some(100),
                realized_pnl: 12,
                updated_at: now,
            }],
        );
        state
            .storage
            .append_competition_snapshot(CompetitionLeaderboardSnapshot {
                snapshot_id: Uuid::new_v4(),
                competition_id: "default".to_string(),
                label: "default final standings".to_string(),
                created_at: now,
                entrants: 1,
                leaderboard: vec![LeaderboardRow {
                    rank: 1,
                    trader_id: provisioned.profile.trader_id,
                    team_number: provisioned.profile.public_team_number().to_string(),
                    net_pnl: 12,
                    realized_pnl: 12,
                    unrealized_pnl: 0,
                    gross_exposure: 500,
                }],
            });

        state.request_checkpoint_save();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if state.persistence_status().total_flushes >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(path.exists(), "checkpoint file should exist");

        let recovered = AppState::new(test_config(Some(path_string)));
        let recovered_user = recovered
            .storage
            .get_user_by_username("alice")
            .expect("recovered user");
        assert_eq!(recovered_user.profile.username, "alice");
        assert_eq!(recovered.storage.list_all_open_orders().len(), 0);
        assert_eq!(
            recovered
                .storage
                .list_positions(recovered_user.profile.trader_id)
                .len(),
            1
        );
        assert_eq!(recovered.storage.list_competition_snapshots().len(), 1);

        let _ = std::fs::remove_file(path);
    }
}
