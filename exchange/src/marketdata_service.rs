use crate::config::Config;
use crate::derived_marketdata::DerivedMarketDataHandle;
use crate::marketdata::BroadcastEvent;
use crate::marketdata_ipc::{MarketBootstrapState, MarketDataRequest, MarketDataResponse};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

pub async fn run_market_data_service(config: Config) -> io::Result<()> {
    let socket_path = config
        .market_data_service_socket
        .clone()
        .unwrap_or_else(|| "/tmp/exchange-marketdata.sock".to_string());
    let socket_path_ref = Path::new(&socket_path);
    if socket_path_ref.exists() {
        std::fs::remove_file(socket_path_ref)?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    loop {
        let (stream, _) = listener.accept().await?;
        handle_connection(stream, config.ws_market_delta_batch_interval_ms).await?;
    }
}

async fn handle_connection(stream: UnixStream, batch_interval_ms: u64) -> io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();
    let mut pending_by_market = BTreeMap::<String, BroadcastEvent>::new();
    let derived = DerivedMarketDataHandle::default();
    let mut bootstrap_markets = BTreeMap::<String, MarketBootstrapState>::new();
    let mut flush_interval = tokio::time::interval(Duration::from_millis(batch_interval_ms.max(1)));

    loop {
        tokio::select! {
            _ = flush_interval.tick() => {
                flush_pending_batches(&mut pending_by_market, &mut write_half).await?;
            }
            next_line = lines.next_line() => {
                let Some(line) = next_line? else {
                    flush_pending_batches(&mut pending_by_market, &mut write_half).await?;
                    return Ok(());
                };
                let request = serde_json::from_str::<MarketDataRequest>(&line).map_err(|error| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("invalid market-data request: {error}"))
                })?;
                match request {
                    MarketDataRequest::Bootstrap { markets, open_orders } => {
                        bootstrap_markets = markets
                            .iter()
                            .map(|market| (market.market.clone(), market.clone()))
                            .collect();
                        derived.replace_from_open_orders(
                            markets
                                .into_iter()
                                .map(|market| (market.market, market.event_sequence))
                                .collect(),
                            open_orders,
                        );
                        pending_by_market.clear();
                    }
                    MarketDataRequest::MarketEvent { envelope } => {
                        let market = envelope.market.clone();
                        let deltas = derived.apply_market_event(envelope);
                        if deltas.is_empty() {
                            continue;
                        }
                        let market_state = bootstrap_markets.entry(market.clone()).or_insert(MarketBootstrapState {
                            market: market.clone(),
                            event_sequence: 0,
                            book_sequence: 0,
                        });
                        let start_sequence = market_state.book_sequence.saturating_add(1);
                        market_state.book_sequence = market_state
                            .book_sequence
                            .saturating_add(deltas.len() as u64);
                        merge_market_batch(
                            &mut pending_by_market,
                            BroadcastEvent {
                                market,
                                start_sequence,
                                sequence: market_state.book_sequence,
                                events: deltas,
                            },
                        );
                    }
                    MarketDataRequest::SnapshotRequest { request_id, market } => {
                        let snapshot = derived.book_snapshot(&market);
                        let sequence = bootstrap_markets
                            .get(&market)
                            .map(|state| state.book_sequence)
                            .unwrap_or(0);
                        write_response(
                            &mut write_half,
                            &MarketDataResponse::Snapshot {
                                request_id,
                                market,
                                sequence,
                                bids: snapshot.bids,
                                asks: snapshot.asks,
                            },
                        )
                        .await?;
                    }
                }
            }
        }
    }
}

fn merge_market_batch(
    pending_by_market: &mut BTreeMap<String, BroadcastEvent>,
    next: BroadcastEvent,
) {
    if let Some(pending) = pending_by_market.get_mut(&next.market) {
        if pending.sequence.saturating_add(1) == next.start_sequence {
            pending.sequence = next.sequence;
            pending.events.extend(next.events);
            return;
        }
    }

    pending_by_market.insert(next.market.clone(), next);
}

async fn flush_pending_batches(
    pending_by_market: &mut BTreeMap<String, BroadcastEvent>,
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
) -> io::Result<()> {
    for (_, batch) in std::mem::take(pending_by_market) {
        write_response(write_half, &MarketDataResponse::Delta { batch }).await?;
    }
    Ok(())
}

async fn write_response(
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    response: &MarketDataResponse,
) -> io::Result<()> {
    let payload = serde_json::to_vec(response).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize response: {error}"),
        )
    })?;
    write_half.write_all(&payload).await?;
    write_half.write_all(b"\n").await?;
    write_half.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketdata::{BookDelta, MarketEvent, MarketEventEnvelope};
    use crate::orderbook::Side;
    use chrono::Utc;
    use tokio::net::UnixStream;
    use uuid::Uuid;

    fn test_config(socket_path: String) -> Config {
        Config {
            bind_addr: "127.0.0.1:0".to_string(),
            checkpoint_path: None,
            checkpoint_interval_seconds: 5,
            ws_broadcast_buffer: 64,
            ws_market_delta_batch_interval_ms: 10,
            ws_market_broadcast_workers: 1,
            market_data_service_socket: Some(socket_path),
            market_data_service_retry_backoff_ms: 250,
            runtime_dispatch_queue_capacity: 4_096,
            account_dispatch_queue_capacity: 4_096,
            per_user_rate_limit_burst_capacity: 500,
            per_user_rate_limit_burst_window_seconds: 10,
            admin_api_token: "test-admin-token".to_string(),
        }
    }

    async fn write_request(stream: &mut UnixStream, request: &MarketDataRequest) {
        let payload = serde_json::to_vec(request).expect("serialize request");
        stream.write_all(&payload).await.expect("write payload");
        stream.write_all(b"\n").await.expect("write newline");
        stream.flush().await.expect("flush request");
    }

    #[tokio::test]
    async fn market_data_service_bootstraps_and_emits_snapshot_and_delta() {
        let socket_path = format!("/tmp/ex-md-{}.sock", &Uuid::new_v4().simple());
        let config = test_config(socket_path.clone());
        let service_task = tokio::spawn(async move {
            run_market_data_service(config).await.expect("run service");
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut stream = UnixStream::connect(&socket_path)
            .await
            .expect("connect market-data socket");

        write_request(
            &mut stream,
            &MarketDataRequest::Bootstrap {
                markets: vec![MarketBootstrapState {
                    market: "BTC-USD".to_string(),
                    event_sequence: 0,
                    book_sequence: 0,
                }],
                open_orders: Vec::new(),
            },
        )
        .await;

        write_request(
            &mut stream,
            &MarketDataRequest::MarketEvent {
                envelope: MarketEventEnvelope {
                    market: "BTC-USD".to_string(),
                    sequence: 1,
                    recorded_at: Utc::now(),
                    event: MarketEvent::OrderAdded {
                        order_id: Uuid::new_v4(),
                        side: Side::Buy,
                        price: 100,
                        remaining: 5,
                        created_at: Utc::now(),
                    },
                },
            },
        )
        .await;

        write_request(
            &mut stream,
            &MarketDataRequest::SnapshotRequest {
                request_id: 7,
                market: "BTC-USD".to_string(),
            },
        )
        .await;

        let mut lines = BufReader::new(stream).lines();
        let mut saw_snapshot = false;
        let mut saw_delta = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while !(saw_snapshot && saw_delta) && tokio::time::Instant::now() < deadline {
            let line = tokio::time::timeout_at(deadline, lines.next_line())
                .await
                .expect("timed out waiting for service response")
                .expect("read line")
                .expect("response line");
            let response =
                serde_json::from_str::<MarketDataResponse>(&line).expect("decode response");
            match response {
                MarketDataResponse::Snapshot {
                    request_id,
                    sequence,
                    bids,
                    asks,
                    ..
                } => {
                    assert_eq!(request_id, 7);
                    assert_eq!(sequence, 1);
                    assert_eq!(bids.len(), 1);
                    assert_eq!(bids[0].price, 100);
                    assert_eq!(bids[0].quantity, 5);
                    assert!(asks.is_empty());
                    saw_snapshot = true;
                }
                MarketDataResponse::Delta { batch } => {
                    assert_eq!(batch.market, "BTC-USD");
                    assert_eq!(batch.start_sequence, 1);
                    assert_eq!(batch.sequence, 1);
                    assert_eq!(
                        batch.events,
                        vec![BookDelta::LevelUpdated {
                            side: Side::Buy,
                            price: 100,
                            quantity: 5,
                        }]
                    );
                    saw_delta = true;
                }
            }
        }

        assert!(saw_snapshot);
        assert!(saw_delta);
        service_task.abort();
        let _ = std::fs::remove_file(socket_path);
    }

    #[tokio::test]
    async fn market_data_service_replaces_existing_snapshot_state_on_bootstrap_refresh() {
        let socket_path = format!("/tmp/ex-md-{}.sock", &Uuid::new_v4().simple());
        let config = test_config(socket_path.clone());
        let service_task = tokio::spawn(async move {
            run_market_data_service(config).await.expect("run service");
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut stream = UnixStream::connect(&socket_path)
            .await
            .expect("connect market-data socket");

        let order = crate::orderbook::Order {
            id: Uuid::new_v4(),
            trader_id: Uuid::new_v4(),
            market: "BTC-USD".to_string(),
            side: Side::Buy,
            price: 100,
            quantity: 5,
            remaining: 5,
            created_at: Utc::now(),
        };

        write_request(
            &mut stream,
            &MarketDataRequest::Bootstrap {
                markets: vec![MarketBootstrapState {
                    market: "BTC-USD".to_string(),
                    event_sequence: 4,
                    book_sequence: 9,
                }],
                open_orders: vec![order],
            },
        )
        .await;

        write_request(
            &mut stream,
            &MarketDataRequest::SnapshotRequest {
                request_id: 1,
                market: "BTC-USD".to_string(),
            },
        )
        .await;

        let mut lines = BufReader::new(stream).lines();
        let first = serde_json::from_str::<MarketDataResponse>(
            &lines
                .next_line()
                .await
                .expect("read line")
                .expect("response line"),
        )
        .expect("decode response");
        match first {
            MarketDataResponse::Snapshot {
                request_id,
                sequence,
                bids,
                asks,
                ..
            } => {
                assert_eq!(request_id, 1);
                assert_eq!(sequence, 9);
                assert_eq!(bids.len(), 1);
                assert_eq!(bids[0].price, 100);
                assert_eq!(bids[0].quantity, 5);
                assert!(asks.is_empty());
            }
            other => panic!("unexpected response: {other:?}"),
        }

        write_request(
            lines.get_mut().get_mut(),
            &MarketDataRequest::Bootstrap {
                markets: vec![MarketBootstrapState {
                    market: "BTC-USD".to_string(),
                    event_sequence: 4,
                    book_sequence: 9,
                }],
                open_orders: Vec::new(),
            },
        )
        .await;
        write_request(
            lines.get_mut().get_mut(),
            &MarketDataRequest::SnapshotRequest {
                request_id: 2,
                market: "BTC-USD".to_string(),
            },
        )
        .await;

        let second = serde_json::from_str::<MarketDataResponse>(
            &lines
                .next_line()
                .await
                .expect("read line")
                .expect("response line"),
        )
        .expect("decode response");
        match second {
            MarketDataResponse::Snapshot {
                request_id,
                sequence,
                bids,
                asks,
                ..
            } => {
                assert_eq!(request_id, 2);
                assert_eq!(sequence, 9);
                assert!(bids.is_empty());
                assert!(asks.is_empty());
            }
            other => panic!("unexpected response: {other:?}"),
        }

        service_task.abort();
        let _ = std::fs::remove_file(socket_path);
    }
}
