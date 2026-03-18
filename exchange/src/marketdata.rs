use crate::orderbook::{Fill, Order, Side};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const L3_CHANNEL: &str = "l3";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientMessage {
    Authenticate {
        api_key: String,
    },
    Subscribe {
        channel: String,
        market: String,
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
        username: String,
    },
    Snapshot {
        channel: String,
        market: String,
        sequence: u64,
        bids: Vec<L3Order>,
        asks: Vec<L3Order>,
    },
    Delta {
        channel: String,
        market: String,
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
    pub sequence: u64,
    pub event: BookDelta,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserBroadcastEvent {
    pub trader_id: Uuid,
    pub message: ServerMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct L3Order {
    pub order_id: Uuid,
    pub side: Side,
    pub price: u64,
    pub remaining: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BookDelta {
    OrderAdded {
        order: L3Order,
    },
    OrderUpdated {
        order: L3Order,
    },
    OrderRemoved {
        order_id: Uuid,
        side: Side,
        price: u64,
    },
    Trade {
        maker_order_id: Uuid,
        taker_order_id: Uuid,
        price: u64,
        quantity: u64,
    },
}

impl From<&Order> for L3Order {
    fn from(order: &Order) -> Self {
        Self {
            order_id: order.id,
            side: order.side,
            price: order.price,
            remaining: order.remaining,
            created_at: order.created_at.to_rfc3339(),
        }
    }
}
