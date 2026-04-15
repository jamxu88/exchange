use crate::admin::{AdminMessageEntry, MarketDefinition};
use crate::orderbook::{BookLevel, Fill, Order, Side};
use crate::trading::OrderType;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DATA_STREAM_CHANNEL: &str = "data";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientMessage {
    Authenticate {
        api_key: String,
    },
    Subscribe {
        channel: String,
        market: String,
        #[serde(default)]
        last_sequence: Option<u64>,
    },
    Unsubscribe {
        channel: String,
        market: String,
    },
    SubmitOrder {
        request_id: Option<String>,
        market: String,
        side: Side,
        #[serde(default)]
        order_type: OrderType,
        price: u64,
        quantity: u64,
    },
    CancelOrder {
        request_id: Option<String>,
        order_id: Uuid,
    },
    AmendOrder {
        request_id: Option<String>,
        order_id: Uuid,
        remaining: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrderStateStatus {
    Open,
    Filled,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Heartbeat,
    Authenticated {
        trader_id: Uuid,
        team_number: String,
    },
    Snapshot {
        channel: String,
        market: String,
        sequence: u64,
        bids: Vec<BookLevel>,
        asks: Vec<BookLevel>,
    },
    Delta {
        channel: String,
        market: String,
        start_sequence: u64,
        sequence: u64,
        events: Vec<BookDelta>,
    },
    Unsubscribed {
        channel: String,
        market: String,
    },
    Ack {
        op: String,
        request_id: Option<String>,
    },
    Reject {
        op: String,
        request_id: Option<String>,
        code: String,
        message: String,
    },
    Fill {
        fill: Fill,
    },
    OrderState {
        order: Order,
        status: OrderStateStatus,
    },
    MarketState {
        market: MarketDefinition,
    },
    MarketDeleted {
        market_id: String,
    },
    AdminMessage {
        message: AdminMessageEntry,
    },
    ResyncRequired {
        channel: String,
        market: Option<String>,
        expected_sequence: Option<u64>,
        current_sequence: Option<u64>,
        reason: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BroadcastEvent {
    pub market: String,
    pub start_sequence: u64,
    pub sequence: u64,
    pub events: Vec<BookDelta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketEventRemoveReason {
    Filled,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarketEvent {
    OrderAdded {
        order_id: Uuid,
        side: Side,
        price: u64,
        remaining: u64,
        created_at: DateTime<Utc>,
    },
    OrderUpdated {
        order_id: Uuid,
        side: Side,
        price: u64,
        remaining: u64,
    },
    OrderRemoved {
        order_id: Uuid,
        side: Side,
        price: u64,
        reason: MarketEventRemoveReason,
    },
    Trade {
        maker_order_id: Uuid,
        taker_order_id: Uuid,
        taker_side: Side,
        price: u64,
        quantity: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketEventEnvelope {
    pub market: String,
    pub sequence: u64,
    pub recorded_at: DateTime<Utc>,
    pub event: MarketEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserBroadcastEvent {
    pub trader_id: Uuid,
    pub message: ServerMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BookDelta {
    LevelUpdated {
        side: Side,
        price: u64,
        quantity: u64,
    },
    Trade {
        price: u64,
        quantity: u64,
    },
}
