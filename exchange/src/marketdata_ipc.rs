use crate::marketdata::{BroadcastEvent, MarketEventEnvelope};
use crate::orderbook::{BookLevel, Order};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketBootstrapState {
    pub market: String,
    pub event_sequence: u64,
    pub book_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MarketDataRequest {
    Bootstrap {
        markets: Vec<MarketBootstrapState>,
        open_orders: Vec<Order>,
    },
    MarketEvent {
        envelope: MarketEventEnvelope,
    },
    SnapshotRequest {
        request_id: u64,
        market: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MarketDataResponse {
    Delta {
        batch: BroadcastEvent,
    },
    Snapshot {
        request_id: u64,
        market: String,
        sequence: u64,
        bids: Vec<BookLevel>,
        asks: Vec<BookLevel>,
    },
}
