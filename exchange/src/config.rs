use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: String,
    pub checkpoint_path: Option<String>,
    pub checkpoint_interval_seconds: u64,
    pub ws_broadcast_buffer: usize,
    pub ws_market_delta_batch_interval_ms: u64,
    pub ws_market_broadcast_workers: usize,
    pub market_data_service_socket: Option<String>,
    pub market_data_service_retry_backoff_ms: u64,
    pub runtime_dispatch_queue_capacity: usize,
    pub account_dispatch_queue_capacity: usize,
    pub per_user_rate_limit_burst_capacity: u64,
    pub per_user_rate_limit_burst_window_seconds: u64,
    pub admin_api_token: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            checkpoint_path: parse_checkpoint_path(),
            checkpoint_interval_seconds: env::var("CHECKPOINT_INTERVAL_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(5),
            ws_broadcast_buffer: env::var("WS_BROADCAST_BUFFER")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1_024),
            ws_market_delta_batch_interval_ms: 100,
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
                .unwrap_or_else(|_| "Quant2024!".to_string()),
        }
    }
}

fn parse_checkpoint_path() -> Option<String> {
    match env::var("CHECKPOINT_PATH") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => Some("exchange.checkpoint.json".to_string()),
    }
}

fn default_market_broadcast_workers() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, 8))
        .unwrap_or(4)
}
