use exchange::{build_app, config::Config, state::AppState};
use tracing::info;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "exchange=info,tower_http=info".to_string()),
        )
        .init();

    let config = Config::from_env();
    let app_state = AppState::new(config.clone());

    let app = build_app(app_state.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&config.bind_addr)
            .await
            .expect("bind listener");
        info!("exchange template listening on {}", config.bind_addr);
        info!("swagger docs at http://{}/docs", config.bind_addr);
        info!("runtime storage backend: {:?}", app_state.storage.kind());
        info!(
            checkpoint_path = ?config.checkpoint_path,
            checkpoint_interval_seconds = config.checkpoint_interval_seconds,
            "checkpoint persistence configured"
        );

        axum::serve(listener, app).await.expect("serve app");
    });
}
