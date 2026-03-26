use crate::orderbook::{Fill, Order, OrderBook, Side};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchExecution {
    pub maker_order_id: Uuid,
    pub maker_trader_id: Uuid,
    pub maker_side: Side,
    pub maker_limit_price: u64,
    pub maker_order_quantity: u64,
    pub maker_created_at: DateTime<Utc>,
    pub price: u64,
    pub quantity: u64,
}

pub struct MatchingEngine;

impl MatchingEngine {
    pub fn process_limit_order(orderbook: &mut OrderBook, incoming: Order) -> Vec<Fill> {
        let taker_order_id = incoming.id;
        let market = incoming.market.clone();
        let occurred_at = Utc::now();
        let executions = Self::process_limit_order_executions(orderbook, incoming);

        executions
            .into_iter()
            .map(|execution| Fill {
                fill_id: Uuid::new_v4(),
                market: market.clone(),
                maker_order_id: execution.maker_order_id,
                taker_order_id,
                price: execution.price,
                quantity: execution.quantity,
                occurred_at,
            })
            .collect()
    }

    pub fn process_limit_order_executions(
        orderbook: &mut OrderBook,
        mut incoming: Order,
    ) -> Vec<MatchExecution> {
        let mut executions = Vec::new();

        while incoming.remaining > 0 {
            let opposite_best_price = match incoming.side {
                Side::Buy => orderbook.best_ask_price(),
                Side::Sell => orderbook.best_bid_price(),
            };

            let Some(price) = opposite_best_price else {
                break;
            };

            let price_crossed = match incoming.side {
                Side::Buy => incoming.price >= price,
                Side::Sell => incoming.price <= price,
            };
            if !price_crossed {
                break;
            }

            let Some((
                price,
                maker_order_id,
                maker_trader_id,
                maker_side,
                maker_limit_price,
                maker_order_quantity,
                maker_created_at,
                matched_qty,
            )) = orderbook.execute_against_best(incoming.side, incoming.remaining)
            else {
                break;
            };
            incoming.remaining -= matched_qty;

            executions.push(MatchExecution {
                maker_order_id,
                maker_trader_id,
                maker_side,
                maker_limit_price,
                maker_order_quantity,
                maker_created_at,
                price,
                quantity: matched_qty,
            });
        }

        if incoming.remaining > 0 {
            orderbook.add_order(incoming);
        }

        executions
    }

    pub fn process_market_order_executions(
        orderbook: &mut OrderBook,
        incoming_side: Side,
        mut quantity: u64,
    ) -> Vec<MatchExecution> {
        let mut executions = Vec::new();

        while quantity > 0 {
            let Some((
                price,
                maker_order_id,
                maker_trader_id,
                maker_side,
                maker_limit_price,
                maker_order_quantity,
                maker_created_at,
                matched_qty,
            )) = orderbook.execute_against_best(incoming_side, quantity)
            else {
                break;
            };

            quantity -= matched_qty;
            executions.push(MatchExecution {
                maker_order_id,
                maker_trader_id,
                maker_side,
                maker_limit_price,
                maker_order_quantity,
                maker_created_at,
                price,
                quantity: matched_qty,
            });
        }

        executions
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
    fn buy_order_crosses_best_asks_with_price_priority() {
        let mut book = OrderBook::default();
        let ask_100 = make_order(Side::Sell, 100, 5);
        let ask_101 = make_order(Side::Sell, 101, 5);
        let ask_101_id = ask_101.id;
        book.add_order(ask_100);
        book.add_order(ask_101);

        let incoming = make_order(Side::Buy, 101, 8);
        let fills = MatchingEngine::process_limit_order(&mut book, incoming);

        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].price, 100);
        assert_eq!(fills[0].quantity, 5);
        assert_eq!(fills[1].price, 101);
        assert_eq!(fills[1].quantity, 3);

        assert!(!book.asks.contains_key(&100));
        let remaining_id = book
            .top_order_id_at_price(Side::Sell, 101)
            .expect("remaining ask id");
        let remaining = book.get_order(remaining_id).expect("remaining ask order");
        assert_eq!(remaining.id, ask_101_id);
        assert_eq!(remaining.remaining, 2);
    }

    #[test]
    fn non_crossing_order_is_added_to_book() {
        let mut book = OrderBook::default();
        book.add_order(make_order(Side::Sell, 100, 5));

        let incoming = make_order(Side::Buy, 99, 4);
        let fills = MatchingEngine::process_limit_order(&mut book, incoming);

        assert!(fills.is_empty());
        let resting_id = book
            .top_order_id_at_price(Side::Buy, 99)
            .expect("resting bid id");
        let resting = book.get_order(resting_id).expect("resting bid order");
        assert_eq!(resting.remaining, 4);
    }

    #[test]
    fn sell_order_matches_highest_bids_first() {
        let mut book = OrderBook::default();
        book.add_order(make_order(Side::Buy, 101, 4));
        book.add_order(make_order(Side::Buy, 100, 4));

        let incoming = make_order(Side::Sell, 100, 6);
        let fills = MatchingEngine::process_limit_order(&mut book, incoming);

        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].price, 101);
        assert_eq!(fills[0].quantity, 4);
        assert_eq!(fills[1].price, 100);
        assert_eq!(fills[1].quantity, 2);

        let remaining_id = book
            .top_order_id_at_price(Side::Buy, 100)
            .expect("remaining bid id");
        let remaining = book
            .get_order(remaining_id)
            .expect("remaining bid order should exist");
        assert_eq!(remaining.remaining, 2);
    }

    #[test]
    fn market_buy_order_consumes_best_asks_without_resting() {
        let mut book = OrderBook::default();
        book.add_order(make_order(Side::Sell, 100, 5));
        book.add_order(make_order(Side::Sell, 101, 5));

        let executions = MatchingEngine::process_market_order_executions(&mut book, Side::Buy, 8);

        assert_eq!(executions.len(), 2);
        assert_eq!(executions[0].price, 100);
        assert_eq!(executions[0].quantity, 5);
        assert_eq!(executions[1].price, 101);
        assert_eq!(executions[1].quantity, 3);

        let remaining_id = book
            .top_order_id_at_price(Side::Sell, 101)
            .expect("remaining ask id");
        let remaining = book.get_order(remaining_id).expect("remaining ask order");
        assert_eq!(remaining.remaining, 2);
    }
}
