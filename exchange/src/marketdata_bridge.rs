use crate::marketdata::MarketEventEnvelope;
use crate::marketdata_ipc::{MarketBootstrapState, MarketDataRequest, MarketDataResponse};
use crate::orderbook::{BookLevel, Order};
use crate::state::AppState;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::runtime::Builder;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};

#[derive(Clone)]
pub(crate) struct MarketDataBridgeHandle {
    tx: tokio_mpsc::UnboundedSender<BridgeCommand>,
    connected: Arc<AtomicBool>,
    next_request_id: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
pub(crate) struct BridgeSnapshot {
    pub sequence: u64,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
}

enum BridgeCommand {
    Publish(MarketEventEnvelope),
    Bootstrap {
        markets: Vec<MarketBootstrapState>,
        open_orders: Vec<Order>,
    },
    Snapshot {
        request_id: u64,
        market: String,
        respond_to: oneshot::Sender<Option<BridgeSnapshot>>,
    },
}

impl MarketDataBridgeHandle {
    pub(crate) fn spawn(state: AppState) -> Self {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let connected = Arc::new(AtomicBool::new(false));
        let next_request_id = Arc::new(AtomicU64::new(1));
        let handle = Self {
            tx,
            connected: connected.clone(),
            next_request_id: next_request_id.clone(),
        };

        std::thread::Builder::new()
            .name("exchange-market-data-bridge".to_string())
            .spawn(move || {
                Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build market-data bridge runtime")
                    .block_on(async move {
                        run_bridge_loop(state, rx, connected).await;
                    });
            })
            .unwrap_or_else(|error| panic!("failed to spawn market-data bridge: {error}"));

        handle
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub(crate) fn publish_event(&self, envelope: MarketEventEnvelope) -> bool {
        if !self.is_connected() {
            return false;
        }
        self.tx.send(BridgeCommand::Publish(envelope)).is_ok()
    }

    pub(crate) fn sync_state(
        &self,
        markets: Vec<MarketBootstrapState>,
        open_orders: Vec<Order>,
    ) -> bool {
        if !self.is_connected() {
            return false;
        }
        self.tx
            .send(BridgeCommand::Bootstrap {
                markets,
                open_orders,
            })
            .is_ok()
    }

    pub(crate) async fn request_snapshot(&self, market: &str) -> Option<BridgeSnapshot> {
        if !self.is_connected() {
            return None;
        }
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (respond_to, rx) = oneshot::channel();
        if self
            .tx
            .send(BridgeCommand::Snapshot {
                request_id,
                market: market.to_string(),
                respond_to,
            })
            .is_err()
        {
            return None;
        }
        rx.await.ok().flatten()
    }
}

async fn run_bridge_loop(
    state: AppState,
    mut rx: tokio_mpsc::UnboundedReceiver<BridgeCommand>,
    connected: Arc<AtomicBool>,
) {
    let Some(socket_path) = state.config.market_data_service_socket.clone() else {
        return;
    };
    let retry_delay = Duration::from_millis(state.config.market_data_service_retry_backoff_ms);

    loop {
        match connect_bridge(&socket_path, &state).await {
            Ok((stream, pending_snapshots)) => {
                connected.store(true, Ordering::Relaxed);
                if bridge_connection_loop(&state, stream, &mut rx, pending_snapshots)
                    .await
                    .is_err()
                {
                    connected.store(false, Ordering::Relaxed);
                }
            }
            Err(_) => {
                connected.store(false, Ordering::Relaxed);
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}

async fn connect_bridge(
    socket_path: &str,
    state: &AppState,
) -> io::Result<(
    UnixStream,
    HashMap<u64, oneshot::Sender<Option<BridgeSnapshot>>>,
)> {
    let mut stream = UnixStream::connect(Path::new(socket_path)).await?;
    send_request(
        &mut stream,
        &MarketDataRequest::Bootstrap {
            markets: state.market_bootstrap_state(),
            open_orders: state.storage.list_all_open_orders(),
        },
    )
    .await?;
    Ok((stream, HashMap::new()))
}

async fn bridge_connection_loop(
    state: &AppState,
    stream: UnixStream,
    rx: &mut tokio_mpsc::UnboundedReceiver<BridgeCommand>,
    mut pending_snapshots: HashMap<u64, oneshot::Sender<Option<BridgeSnapshot>>>,
) -> io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    loop {
        tokio::select! {
            command = rx.recv() => {
                let Some(command) = command else {
                    return Ok(());
                };
                match command {
                    BridgeCommand::Publish(envelope) => {
                        write_request(&mut write_half, &MarketDataRequest::MarketEvent { envelope }).await?;
                    }
                    BridgeCommand::Bootstrap { markets, open_orders } => {
                        write_request(&mut write_half, &MarketDataRequest::Bootstrap { markets, open_orders }).await?;
                    }
                    BridgeCommand::Snapshot { request_id, market, respond_to } => {
                        pending_snapshots.insert(request_id, respond_to);
                        write_request(&mut write_half, &MarketDataRequest::SnapshotRequest { request_id, market }).await?;
                    }
                }
            }
            next_line = lines.next_line() => {
                let Some(line) = next_line? else {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "market-data service disconnected"));
                };
                let response = serde_json::from_str::<MarketDataResponse>(&line).map_err(|error| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("invalid market-data response: {error}"))
                })?;
                match response {
                    MarketDataResponse::Delta { batch } => {
                        state.apply_bridge_market_batch(batch);
                    }
                    MarketDataResponse::Snapshot { request_id, sequence, bids, asks, .. } => {
                        if let Some(respond_to) = pending_snapshots.remove(&request_id) {
                            let _ = respond_to.send(Some(BridgeSnapshot { sequence, bids, asks }));
                        }
                    }
                }
            }
        }
    }
}

async fn send_request(stream: &mut UnixStream, request: &MarketDataRequest) -> io::Result<()> {
    let payload = serde_json::to_vec(request).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize request: {error}"),
        )
    })?;
    stream.write_all(&payload).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await
}

async fn write_request(
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    request: &MarketDataRequest,
) -> io::Result<()> {
    let payload = serde_json::to_vec(request).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize request: {error}"),
        )
    })?;
    write_half.write_all(&payload).await?;
    write_half.write_all(b"\n").await?;
    write_half.flush().await
}
