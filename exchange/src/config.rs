use crate::storage::StorageBackendKind;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub storage_backend: StorageBackendKind,
    pub ws_broadcast_buffer: usize,
    pub per_user_requests_per_second: u64,
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
            per_user_requests_per_second: env::var("PER_USER_REQUESTS_PER_SECOND")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(100),
            admin_api_token: env::var("ADMIN_API_TOKEN")
                .unwrap_or_else(|_| "local-admin-token".to_string()),
            postgres_write_batch_size: env::var("POSTGRES_WRITE_BATCH_SIZE")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(128),
            postgres_write_flush_interval_ms: env::var("POSTGRES_WRITE_FLUSH_INTERVAL_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(25),
            postgres_write_queue_capacity: env::var("POSTGRES_WRITE_QUEUE_CAPACITY")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(4_096),
            postgres_write_retry_backoff_ms: env::var("POSTGRES_WRITE_RETRY_BACKOFF_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(250),
        }
    }
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
