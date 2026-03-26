use crate::storage::StorageBackendKind;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub storage_backend: StorageBackendKind,
    pub ws_broadcast_buffer: usize,
    pub ws_market_delta_batch_interval_ms: u64,
    pub ws_market_broadcast_workers: usize,
    pub market_data_service_socket: Option<String>,
    pub market_data_service_retry_backoff_ms: u64,
    pub runtime_dispatch_queue_capacity: usize,
    pub account_dispatch_queue_capacity: usize,
    pub persistence_dispatch_queue_capacity: usize,
    pub per_user_rate_limit_burst_capacity: u64,
    pub per_user_rate_limit_burst_window_seconds: u64,
    pub admin_api_token: String,
    pub postgres_write_batch_size: usize,
    pub postgres_write_flush_interval_ms: u64,
    pub postgres_write_queue_capacity: usize,
    pub postgres_write_retry_backoff_ms: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://exchange:exchange@localhost:5432/exchange".to_string()
            }),
            storage_backend: parse_storage_backend(),
            ws_broadcast_buffer: env::var("WS_BROADCAST_BUFFER")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1_024),
            ws_market_delta_batch_interval_ms: env::var("WS_MARKET_DELTA_BATCH_INTERVAL_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(100),
            ws_market_broadcast_workers: env::var("WS_MARKET_BROADCAST_WORKERS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .unwrap_or_else(default_market_broadcast_workers),
            market_data_service_socket: env::var("MARKET_DATA_SERVICE_SOCKET").ok().and_then(
                |value| {
                    let trimmed = value.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                },
            ),
            market_data_service_retry_backoff_ms: env::var("MARKET_DATA_SERVICE_RETRY_BACKOFF_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(250),
            runtime_dispatch_queue_capacity: env::var("RUNTIME_DISPATCH_QUEUE_CAPACITY")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(16_384),
            account_dispatch_queue_capacity: env::var("ACCOUNT_DISPATCH_QUEUE_CAPACITY")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(65_536),
            persistence_dispatch_queue_capacity: env::var("PERSISTENCE_DISPATCH_QUEUE_CAPACITY")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(16_384),
            per_user_rate_limit_burst_capacity: env::var("PER_USER_RATE_LIMIT_BURST_CAPACITY")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(500),
            per_user_rate_limit_burst_window_seconds: env::var(
                "PER_USER_RATE_LIMIT_BURST_WINDOW_SECONDS",
            )
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(10),
            admin_api_token: env::var("ADMIN_API_TOKEN")
                .unwrap_or_else(|_| "local-admin-token".to_string()),
            postgres_write_batch_size: env::var("POSTGRES_WRITE_BATCH_SIZE")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(512),
            postgres_write_flush_interval_ms: env::var("POSTGRES_WRITE_FLUSH_INTERVAL_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(10),
            postgres_write_queue_capacity: env::var("POSTGRES_WRITE_QUEUE_CAPACITY")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(65_536),
            postgres_write_retry_backoff_ms: env::var("POSTGRES_WRITE_RETRY_BACKOFF_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(250),
        }
    }
}

fn default_market_broadcast_workers() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, 8))
        .unwrap_or(4)
}

fn parse_storage_backend() -> StorageBackendKind {
    match env::var("STORAGE_BACKEND")
        .unwrap_or_else(|_| "in_memory".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "in_memory" | "memory" => StorageBackendKind::InMemory,
        "postgres" => StorageBackendKind::Postgres,
        other => panic!("unsupported STORAGE_BACKEND value: {other}"),
    }
}
