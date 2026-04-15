use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, hash_map::Entry};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct Order {
    pub id: Uuid,
    pub trader_id: Uuid,
    pub market: String,
    pub side: Side,
    pub price: u64,
    pub quantity: u64,
    pub remaining: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct Fill {
    pub fill_id: Uuid,
    pub market: String,
    pub maker_order_id: Uuid,
    pub taker_order_id: Uuid,
    pub price: u64,
    pub quantity: u64,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct BookLevel {
    pub price: u64,
    pub quantity: u64,
}

#[derive(Debug, Default, Clone)]
pub struct PriceLevel {
    pub head: Option<usize>,
    pub tail: Option<usize>,
    pub total_qty: u64,
    pub len: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct OrderLocator {
    pub side: Side,
    pub price: u64,
    pub handle: usize,
}

#[derive(Debug, Clone)]
struct OrderNode {
    order: Order,
    prev: Option<usize>,
    next: Option<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct OrderBook {
    pub bids: HashMap<u64, PriceLevel>,
    pub asks: HashMap<u64, PriceLevel>,
    pub bid_prices: BTreeSet<Reverse<u64>>,
    pub ask_prices: BTreeSet<u64>,
    nodes: Vec<Option<OrderNode>>,
    free_nodes: Vec<usize>,
    pub order_index: HashMap<Uuid, OrderLocator>,
}

impl OrderBook {
    fn checked_add_u64(value: u64, delta: u64, context: &str) -> u64 {
        value.checked_add(delta).expect(context)
    }

    fn checked_sub_u64(value: u64, delta: u64, context: &str) -> u64 {
        value.checked_sub(delta).expect(context)
    }

    fn checked_sub_usize(value: usize, delta: usize, context: &str) -> usize {
        value.checked_sub(delta).expect(context)
    }

    pub fn add_order(&mut self, order: Order) {
        self.insert_order(order);
    }

    pub fn best_ask_price(&self) -> Option<u64> {
        self.ask_prices.first().copied()
    }

    pub fn best_bid_price(&self) -> Option<u64> {
        self.bid_prices.first().map(|price| price.0)
    }

    pub fn top_order_id_at_price(&self, side: Side, price: u64) -> Option<Uuid> {
        match side {
            Side::Buy => self.bids.get(&price).and_then(|level| {
                let handle = level.head?;
                Some(self.node(handle)?.order.id)
            }),
            Side::Sell => self.asks.get(&price).and_then(|level| {
                let handle = level.head?;
                Some(self.node(handle)?.order.id)
            }),
        }
    }

    pub fn get_order(&self, order_id: Uuid) -> Option<&Order> {
        let locator = self.order_index.get(&order_id)?;
        Some(&self.node(locator.handle)?.order)
    }

    pub fn orders_for_side(&self, side: Side) -> Vec<Order> {
        let mut orders = Vec::new();
        match side {
            Side::Buy => {
                for price in &self.bid_prices {
                    self.collect_level_orders(Side::Buy, price.0, &mut orders);
                }
            }
            Side::Sell => {
                for price in &self.ask_prices {
                    self.collect_level_orders(Side::Sell, *price, &mut orders);
                }
            }
        }
        orders
    }

    pub fn levels_for_side(&self, side: Side) -> Vec<BookLevel> {
        match side {
            Side::Buy => self
                .bid_prices
                .iter()
                .filter_map(|price| {
                    let quantity = self.level_quantity(Side::Buy, price.0);
                    (quantity > 0).then_some(BookLevel {
                        price: price.0,
                        quantity,
                    })
                })
                .collect(),
            Side::Sell => self
                .ask_prices
                .iter()
                .filter_map(|price| {
                    let quantity = self.level_quantity(Side::Sell, *price);
                    (quantity > 0).then_some(BookLevel {
                        price: *price,
                        quantity,
                    })
                })
                .collect(),
        }
    }

    pub fn level_quantity(&self, side: Side, price: u64) -> u64 {
        self.get_level(side, price)
            .map(|level| level.total_qty)
            .unwrap_or(0)
    }

    pub fn cancel_order(&mut self, order_id: Uuid) -> Option<Order> {
        let locator = *self.order_index.get(&order_id)?;
        let removed = self.unlink_node(locator.side, locator.price, locator.handle)?;
        self.order_index.remove(&order_id);
        Some(removed)
    }

    pub fn amend_order_remaining(&mut self, order_id: Uuid, new_remaining: u64) -> Option<()> {
        let locator = *self.order_index.get(&order_id)?;
        let old_remaining = self.node(locator.handle)?.order.remaining;
        if new_remaining == old_remaining {
            return Some(());
        }

        let delta = old_remaining.abs_diff(new_remaining);
        let level = match locator.side {
            Side::Buy => self.bids.get_mut(&locator.price)?,
            Side::Sell => self.asks.get_mut(&locator.price)?,
        };

        if new_remaining > old_remaining {
            level.total_qty = Self::checked_add_u64(
                level.total_qty,
                delta,
                "order book: level total_qty add (amend)",
            );
        } else {
            level.total_qty = Self::checked_sub_u64(
                level.total_qty,
                delta,
                "order book: level total_qty sub (amend)",
            );
        }

        let node = self.node_mut(locator.handle)?;
        node.order.remaining = new_remaining;
        Some(())
    }

    pub fn execute_against_best(
        &mut self,
        incoming_side: Side,
        max_qty: u64,
    ) -> Option<(u64, Uuid, Uuid, Side, u64, u64, DateTime<Utc>, u64)> {
        let (side, price) = match incoming_side {
            Side::Buy => (Side::Sell, self.best_ask_price()?),
            Side::Sell => (Side::Buy, self.best_bid_price()?),
        };

        let head_handle = {
            let level = self.get_level(side, price)?;
            level.head?
        };

        let (
            maker_id,
            maker_trader_id,
            maker_side,
            maker_limit_price,
            maker_quantity,
            maker_created_at,
            traded_qty,
            is_filled,
        ) = {
            let node = self.node_mut(head_handle)?;
            let traded_qty = node.order.remaining.min(max_qty);
            node.order.remaining = Self::checked_sub_u64(
                node.order.remaining,
                traded_qty,
                "order book: remaining after match",
            );
            (
                node.order.id,
                node.order.trader_id,
                node.order.side,
                node.order.price,
                node.order.quantity,
                node.order.created_at,
                traded_qty,
                node.order.remaining == 0,
            )
        };

        let level = match side {
            Side::Buy => self.bids.get_mut(&price)?,
            Side::Sell => self.asks.get_mut(&price)?,
        };
        level.total_qty = Self::checked_sub_u64(
            level.total_qty,
            traded_qty,
            "order book: level total_qty sub (execute)",
        );

        if is_filled {
            self.unlink_node(side, price, head_handle)?;
            self.order_index.remove(&maker_id);
        }

        Some((
            price,
            maker_id,
            maker_trader_id,
            maker_side,
            maker_limit_price,
            maker_quantity,
            maker_created_at,
            traded_qty,
        ))
    }

    fn insert_order(&mut self, order: Order) {
        let order_id = order.id;
        let side = order.side;
        let price = order.price;
        let remaining = order.remaining;
        let handle = self.allocate_node(OrderNode {
            order,
            prev: None,
            next: None,
        });

        let previous_tail = self.get_level(side, price).and_then(|level| level.tail);
        if let Some(tail) = previous_tail {
            if let Some(tail_node) = self.node_mut(tail) {
                tail_node.next = Some(handle);
            }
            if let Some(new_node) = self.node_mut(handle) {
                new_node.prev = Some(tail);
            }
        }

        let level = self.get_or_create_level_mut(side, price);
        if previous_tail.is_some() {
            level.tail = Some(handle);
        } else {
            level.head = Some(handle);
            level.tail = Some(handle);
        }
        level.total_qty = Self::checked_add_u64(
            level.total_qty,
            remaining,
            "order book: level total_qty add (insert)",
        );
        level.len += 1;

        self.order_index.insert(
            order_id,
            OrderLocator {
                side,
                price,
                handle,
            },
        );
    }

    fn allocate_node(&mut self, node: OrderNode) -> usize {
        if let Some(index) = self.free_nodes.pop() {
            self.nodes[index] = Some(node);
            return index;
        }
        self.nodes.push(Some(node));
        self.nodes.len() - 1
    }

    fn node(&self, handle: usize) -> Option<&OrderNode> {
        self.nodes.get(handle)?.as_ref()
    }

    fn node_mut(&mut self, handle: usize) -> Option<&mut OrderNode> {
        self.nodes.get_mut(handle)?.as_mut()
    }

    fn get_level(&self, side: Side, price: u64) -> Option<&PriceLevel> {
        match side {
            Side::Buy => self.bids.get(&price),
            Side::Sell => self.asks.get(&price),
        }
    }

    fn get_or_create_level_mut(&mut self, side: Side, price: u64) -> &mut PriceLevel {
        match side {
            Side::Buy => match self.bids.entry(price) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    self.bid_prices.insert(Reverse(price));
                    entry.insert(PriceLevel::default())
                }
            },
            Side::Sell => match self.asks.entry(price) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    self.ask_prices.insert(price);
                    entry.insert(PriceLevel::default())
                }
            },
        }
    }

    fn remove_level_if_empty(&mut self, side: Side, price: u64) {
        let should_remove = match side {
            Side::Buy => self
                .bids
                .get(&price)
                .map(|level| level.len == 0)
                .unwrap_or(false),
            Side::Sell => self
                .asks
                .get(&price)
                .map(|level| level.len == 0)
                .unwrap_or(false),
        };

        if !should_remove {
            return;
        }

        match side {
            Side::Buy => {
                self.bids.remove(&price);
                self.bid_prices.remove(&Reverse(price));
            }
            Side::Sell => {
                self.asks.remove(&price);
                self.ask_prices.remove(&price);
            }
        }
    }

    fn unlink_node(&mut self, side: Side, price: u64, handle: usize) -> Option<Order> {
        let (prev, next, order_remaining) = {
            let node = self.node(handle)?;
            (node.prev, node.next, node.order.remaining)
        };

        if let Some(prev_handle) = prev {
            if let Some(prev_node) = self.node_mut(prev_handle) {
                prev_node.next = next;
            }
        }
        if let Some(next_handle) = next {
            if let Some(next_node) = self.node_mut(next_handle) {
                next_node.prev = prev;
            }
        }

        {
            let level = match side {
                Side::Buy => self.bids.get_mut(&price)?,
                Side::Sell => self.asks.get_mut(&price)?,
            };

            if level.head == Some(handle) {
                level.head = next;
            }
            if level.tail == Some(handle) {
                level.tail = prev;
            }
            level.total_qty = Self::checked_sub_u64(
                level.total_qty,
                order_remaining,
                "order book: level total_qty sub (unlink)",
            );
            level.len = Self::checked_sub_usize(level.len, 1, "order book: level len sub (unlink)");
        }

        self.remove_level_if_empty(side, price);

        let node = self.nodes.get_mut(handle)?.take()?;
        self.free_nodes.push(handle);
        Some(node.order)
    }

    fn collect_level_orders(&self, side: Side, price: u64, orders: &mut Vec<Order>) {
        let mut cursor = self.get_level(side, price).and_then(|level| level.head);
        while let Some(handle) = cursor {
            let Some(node) = self.node(handle) else {
                break;
            };
            orders.push(node.order.clone());
            cursor = node.next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_order(side: Side, price: u64, quantity: u64) -> Order {
        Order {
            id: Uuid::new_v4(),
            trader_id: Uuid::new_v4(),
            market: "BTC-USD".to_string(),
            side,
            price,
            quantity,
            remaining: quantity,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn add_order_routes_orders_by_side() {
        let mut book = OrderBook::default();
        book.add_order(make_order(Side::Buy, 100, 5));
        book.add_order(make_order(Side::Sell, 101, 2));

        assert_eq!(book.bids.len(), 1);
        assert_eq!(book.asks.len(), 1);
    }

    #[test]
    fn add_order_preserves_fifo_for_same_price_level() {
        let mut book = OrderBook::default();
        let first = make_order(Side::Buy, 100, 2);
        let second = make_order(Side::Buy, 100, 4);
        let first_id = first.id;
        let second_id = second.id;

        book.add_order(first);
        book.add_order(second);

        assert_eq!(book.top_order_id_at_price(Side::Buy, 100), Some(first_id));
        assert_eq!(
            book.get_order(second_id).expect("second order").remaining,
            4
        );
    }

    #[test]
    fn best_prices_are_available_in_o1_tree_head_access() {
        let mut book = OrderBook::default();
        book.add_order(make_order(Side::Buy, 99, 1));
        book.add_order(make_order(Side::Buy, 101, 1));
        book.add_order(make_order(Side::Sell, 103, 1));
        book.add_order(make_order(Side::Sell, 102, 1));

        assert_eq!(book.best_bid_price(), Some(101));
        assert_eq!(book.best_ask_price(), Some(102));
    }

    #[test]
    fn cancel_order_removes_target_in_o1_by_handle() {
        let mut book = OrderBook::default();
        let first = make_order(Side::Buy, 100, 2);
        let first_id = first.id;
        let middle = make_order(Side::Buy, 100, 4);
        let last = make_order(Side::Buy, 100, 6);

        book.add_order(first);
        book.add_order(middle.clone());
        book.add_order(last);

        let removed = book
            .cancel_order(middle.id)
            .expect("middle order should cancel");
        assert_eq!(removed.id, middle.id);
        assert!(!book.order_index.contains_key(&middle.id));
        assert!(book.get_order(middle.id).is_none());
        assert_eq!(book.top_order_id_at_price(Side::Buy, 100), Some(first_id));
    }

    #[test]
    fn amend_order_remaining_updates_level_quantity_totals() {
        let mut book = OrderBook::default();
        let order = make_order(Side::Buy, 100, 10);
        let order_id = order.id;
        book.add_order(order);

        assert_eq!(book.level_quantity(Side::Buy, 100), 10);
        book.amend_order_remaining(order_id, 4)
            .expect("amend should succeed");
        assert_eq!(book.level_quantity(Side::Buy, 100), 4);
        book.amend_order_remaining(order_id, 9)
            .expect("amend should succeed");
        assert_eq!(book.level_quantity(Side::Buy, 100), 9);
    }

    #[test]
    fn execute_against_best_updates_level_totals_and_removes_empty_level() {
        let mut book = OrderBook::default();
        let maker = make_order(Side::Sell, 101, 7);
        book.add_order(maker.clone());

        let first = book
            .execute_against_best(Side::Buy, 3)
            .expect("first execution");
        assert_eq!(first.7, 3);
        assert_eq!(book.level_quantity(Side::Sell, 101), 4);

        let second = book
            .execute_against_best(Side::Buy, 4)
            .expect("second execution");
        assert_eq!(second.7, 4);
        assert_eq!(book.level_quantity(Side::Sell, 101), 0);
        assert!(book.best_ask_price().is_none());
    }
}
