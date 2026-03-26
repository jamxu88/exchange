use exchange::{config::Config, marketdata_service::run_market_data_service};
use tracing::info;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "exchange=info,tower_http=info".to_string()),
        )
        .init();

    let config = Config::from_env();
    let socket_path = config
        .market_data_service_socket
        .clone()
        .unwrap_or_else(|| "/tmp/exchange-marketdata.sock".to_string());

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async move {
        info!("market-data service listening on {}", socket_path);
        run_market_data_service(config)
            .await
            .expect("run market-data service");
    });
}
