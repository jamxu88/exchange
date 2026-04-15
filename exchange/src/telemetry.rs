use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use utoipa::ToSchema;

const RATE_WINDOW_SECONDS: u64 = 10;
const SNAPSHOT_CACHE_TTL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActionTelemetrySnapshot {
    pub total: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub total_per_second_10s: f64,
    pub accepted_per_second_10s: f64,
    pub rejected_per_second_10s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FillTelemetrySnapshot {
    pub total: u64,
    pub shares: u64,
    pub fills_per_second_10s: f64,
    pub shares_per_second_10s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CounterTelemetrySnapshot {
    pub total: u64,
    pub per_second_10s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebSocketTelemetrySnapshot {
    pub connections_current: u64,
    pub connections_total: u64,
    pub authenticated_current: u64,
    pub authenticated_total: u64,
    pub data_stream_subscribers_current: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResyncTelemetrySnapshot {
    pub user: CounterTelemetrySnapshot,
    pub system: CounterTelemetrySnapshot,
    pub data_stream: CounterTelemetrySnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OperatorTelemetrySnapshot {
    pub submits: ActionTelemetrySnapshot,
    pub cancels: ActionTelemetrySnapshot,
    pub amends: ActionTelemetrySnapshot,
    pub fills: FillTelemetrySnapshot,
    pub rate_limit_rejections: CounterTelemetrySnapshot,
    pub websocket: WebSocketTelemetrySnapshot,
    pub resyncs: ResyncTelemetrySnapshot,
}

#[derive(Clone, Default)]
pub struct OperatorTelemetry {
    submits: ActionTelemetry,
    cancels: ActionTelemetry,
    amends: ActionTelemetry,
    fills: CounterMetric,
    fill_shares: CounterMetric,
    rate_limit_rejections: CounterMetric,
    user_resyncs: CounterMetric,
    system_resyncs: CounterMetric,
    data_stream_resyncs: CounterMetric,
    ws_connections_current: Arc<AtomicU64>,
    ws_connections_total: Arc<AtomicU64>,
    ws_authenticated_current: Arc<AtomicU64>,
    ws_authenticated_total: Arc<AtomicU64>,
    data_stream_subscribers_current: Arc<AtomicU64>,
    snapshot_cache: Arc<Mutex<Option<CachedOperatorTelemetrySnapshot>>>,
}

impl OperatorTelemetry {
    pub fn record_submit_attempt(&self) {
        self.submits.record_attempt();
    }

    pub fn record_submit_accept(&self) {
        self.submits.record_accept();
    }

    pub fn record_submit_reject(&self) {
        self.submits.record_reject();
    }

    pub fn record_cancel_attempt(&self) {
        self.cancels.record_attempt();
    }

    pub fn record_cancel_accept(&self) {
        self.cancels.record_accept();
    }

    pub fn record_cancel_reject(&self) {
        self.cancels.record_reject();
    }

    pub fn record_amend_attempt(&self) {
        self.amends.record_attempt();
    }

    pub fn record_amend_accept(&self) {
        self.amends.record_accept();
    }

    pub fn record_amend_reject(&self) {
        self.amends.record_reject();
    }

    pub fn record_fill(&self, quantity: u64) {
        self.fills.record(1);
        self.fill_shares.record(quantity);
    }

    pub fn record_rate_limit_reject(&self) {
        self.rate_limit_rejections.record(1);
    }

    pub fn record_ws_connection_open(&self) {
        self.ws_connections_current.fetch_add(1, Ordering::Relaxed);
        self.ws_connections_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_ws_connection_close(&self) {
        decrement_atomic(&self.ws_connections_current);
    }

    pub fn record_ws_authenticated_open(&self) {
        self.ws_authenticated_current
            .fetch_add(1, Ordering::Relaxed);
        self.ws_authenticated_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_ws_authenticated_close(&self) {
        decrement_atomic(&self.ws_authenticated_current);
    }

    pub fn record_data_stream_subscriber_open(&self) {
        self.data_stream_subscribers_current
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_data_stream_subscriber_close(&self) {
        decrement_atomic(&self.data_stream_subscribers_current);
    }

    pub fn record_user_resync(&self) {
        self.user_resyncs.record(1);
    }

    pub fn record_system_resync(&self) {
        self.system_resyncs.record(1);
    }

    pub fn record_data_stream_resync(&self) {
        self.data_stream_resyncs.record(1);
    }

    pub fn snapshot(&self) -> OperatorTelemetrySnapshot {
        {
            let cache = self
                .snapshot_cache
                .lock()
                .expect("telemetry snapshot cache lock");
            if let Some(cached) = cache.as_ref() {
                if cached.captured_at.elapsed() < SNAPSHOT_CACHE_TTL {
                    return cached.snapshot.clone();
                }
            }
        }

        let snapshot = OperatorTelemetrySnapshot {
            submits: self.submits.snapshot(),
            cancels: self.cancels.snapshot(),
            amends: self.amends.snapshot(),
            fills: FillTelemetrySnapshot {
                total: self.fills.total(),
                shares: self.fill_shares.total(),
                fills_per_second_10s: self.fills.rate_per_second_10s(),
                shares_per_second_10s: self.fill_shares.rate_per_second_10s(),
            },
            rate_limit_rejections: self.rate_limit_rejections.snapshot(),
            websocket: WebSocketTelemetrySnapshot {
                connections_current: self.ws_connections_current.load(Ordering::Relaxed),
                connections_total: self.ws_connections_total.load(Ordering::Relaxed),
                authenticated_current: self.ws_authenticated_current.load(Ordering::Relaxed),
                authenticated_total: self.ws_authenticated_total.load(Ordering::Relaxed),
                data_stream_subscribers_current: self
                    .data_stream_subscribers_current
                    .load(Ordering::Relaxed),
            },
            resyncs: ResyncTelemetrySnapshot {
                user: self.user_resyncs.snapshot(),
                system: self.system_resyncs.snapshot(),
                data_stream: self.data_stream_resyncs.snapshot(),
            },
        };

        let mut cache = self
            .snapshot_cache
            .lock()
            .expect("telemetry snapshot cache lock");
        *cache = Some(CachedOperatorTelemetrySnapshot {
            captured_at: Instant::now(),
            snapshot: snapshot.clone(),
        });

        snapshot
    }
}

#[derive(Clone)]
struct CachedOperatorTelemetrySnapshot {
    captured_at: Instant,
    snapshot: OperatorTelemetrySnapshot,
}

#[derive(Clone, Default)]
struct ActionTelemetry {
    attempts: CounterMetric,
    accepted: CounterMetric,
    rejected: CounterMetric,
}

impl ActionTelemetry {
    fn record_attempt(&self) {
        self.attempts.record(1);
    }

    fn record_accept(&self) {
        self.accepted.record(1);
    }

    fn record_reject(&self) {
        self.rejected.record(1);
    }

    fn snapshot(&self) -> ActionTelemetrySnapshot {
        ActionTelemetrySnapshot {
            total: self.attempts.total(),
            accepted: self.accepted.total(),
            rejected: self.rejected.total(),
            total_per_second_10s: self.attempts.rate_per_second_10s(),
            accepted_per_second_10s: self.accepted.rate_per_second_10s(),
            rejected_per_second_10s: self.rejected.rate_per_second_10s(),
        }
    }
}

#[derive(Clone, Default)]
struct CounterMetric {
    total: Arc<AtomicU64>,
    window: RollingWindowCounter,
}

impl CounterMetric {
    fn record(&self, value: u64) {
        self.total.fetch_add(value, Ordering::Relaxed);
        self.window.record(value);
    }

    fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    fn rate_per_second_10s(&self) -> f64 {
        self.window.rate_per_second_10s()
    }

    fn snapshot(&self) -> CounterTelemetrySnapshot {
        CounterTelemetrySnapshot {
            total: self.total(),
            per_second_10s: self.rate_per_second_10s(),
        }
    }
}

#[derive(Clone, Default)]
struct RollingWindowCounter {
    inner: Arc<Mutex<RollingWindowCounterInner>>,
}

#[derive(Default)]
struct RollingWindowCounterInner {
    entries: VecDeque<(u64, u64)>,
}

impl RollingWindowCounter {
    fn record(&self, value: u64) {
        self.record_at(current_unix_second(), value);
    }

    fn record_at(&self, second: u64, value: u64) {
        let mut inner = self.inner.lock().expect("rolling counter lock");
        prune_entries(&mut inner.entries, second);
        if let Some((last_second, count)) = inner.entries.back_mut() {
            if *last_second == second {
                *count += value;
                return;
            }
        }
        inner.entries.push_back((second, value));
    }

    fn rate_per_second_10s(&self) -> f64 {
        self.rate_per_second_10s_at(current_unix_second())
    }

    fn rate_per_second_10s_at(&self, second: u64) -> f64 {
        let mut inner = self.inner.lock().expect("rolling counter lock");
        prune_entries(&mut inner.entries, second);
        let total = inner.entries.iter().map(|(_, value)| *value).sum::<u64>();
        total as f64 / RATE_WINDOW_SECONDS as f64
    }
}

fn current_unix_second() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn prune_entries(entries: &mut VecDeque<(u64, u64)>, current_second: u64) {
    while let Some((entry_second, _)) = entries.front() {
        if entry_second.saturating_add(RATE_WINDOW_SECONDS) <= current_second {
            entries.pop_front();
        } else {
            break;
        }
    }
}

fn decrement_atomic(counter: &AtomicU64) {
    let mut current = counter.load(Ordering::Relaxed);
    while current > 0 {
        match counter.compare_exchange_weak(
            current,
            current - 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(updated) => current = updated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_window_counter_prunes_old_entries() {
        let counter = RollingWindowCounter::default();

        counter.record_at(100, 20);
        counter.record_at(105, 30);
        assert_eq!(counter.rate_per_second_10s_at(105), 5.0);

        assert_eq!(counter.rate_per_second_10s_at(110), 3.0);
        assert_eq!(counter.rate_per_second_10s_at(116), 0.0);
    }

    #[test]
    fn websocket_current_counts_do_not_underflow() {
        let telemetry = OperatorTelemetry::default();

        telemetry.record_ws_connection_close();
        telemetry.record_data_stream_subscriber_close();

        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.websocket.connections_current, 0);
        assert_eq!(snapshot.websocket.data_stream_subscribers_current, 0);
    }
}
