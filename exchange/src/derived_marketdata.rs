use crate::marketdata::{BookDelta, MarketEvent, MarketEventEnvelope};
use crate::orderbook::{BookLevel, Order, Side};
use crate::trading::MarketBookSnapshot;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

const MARKET_EVENT_REPLAY_LIMIT: usize = 4_096;

#[derive(Clone, Default)]
pub(crate) struct DerivedMarketDataHandle {
    inner: Arc<RwLock<HashMap<String, DerivedMarketState>>>,
}

#[derive(Debug, Clone)]
struct DerivedOrder {
    order_id: Uuid,
    side: Side,
    price: u64,
    remaining: u64,
}

#[derive(Debug, Default, Clone)]
struct DerivedMarketState {
    sequence: u64,
    orders: HashMap<Uuid, DerivedOrder>,
    bids: BTreeMap<u64, u64>,
    asks: BTreeMap<u64, u64>,
    recent_events: VecDeque<MarketEventEnvelope>,
}

impl DerivedMarketDataHandle {
    pub(crate) fn replace_from_open_orders(
        &self,
        market_sequences: Vec<(String, u64)>,
        open_orders: Vec<Order>,
    ) {
        let mut rebuilt = HashMap::new();

        for (market, sequence) in market_sequences {
            rebuilt.insert(
                market,
                DerivedMarketState {
                    sequence,
                    ..DerivedMarketState::default()
                },
            );
        }

        for order in open_orders {
            let market = rebuilt.entry(order.market.clone()).or_default();
            market.insert_order(DerivedOrder {
                order_id: order.id,
                side: order.side,
                price: order.price,
                remaining: order.remaining,
            });
        }

        *self
            .inner
            .write()
            .expect("derived market data lock poisoned during rebuild") = rebuilt;
    }

    pub(crate) fn apply_market_event(&self, envelope: MarketEventEnvelope) -> Vec<BookDelta> {
        let market = envelope.market.clone();
        let mut markets = self
            .inner
            .write()
            .expect("derived market data lock poisoned during event apply");
        let state = markets.entry(market).or_default();
        state.apply_market_event(envelope)
    }

    pub(crate) fn clear_market(&self, market: &str) {
        self.inner
            .write()
            .expect("derived market data lock poisoned during clear")
            .remove(market);
    }

    pub(crate) fn clear_all(&self) {
        self.inner
            .write()
            .expect("derived market data lock poisoned during clear_all")
            .clear();
    }

    pub(crate) fn book_snapshot(&self, market: &str) -> MarketBookSnapshot {
        let markets = self
            .inner
            .read()
            .expect("derived market data lock poisoned during book snapshot");
        markets
            .get(market)
            .map(DerivedMarketState::book_snapshot)
            .unwrap_or_default()
    }

    pub(crate) fn best_prices(&self, market: &str) -> (Option<u64>, Option<u64>) {
        let markets = self
            .inner
            .read()
            .expect("derived market data lock poisoned during best_prices");
        markets
            .get(market)
            .map(DerivedMarketState::best_prices)
            .unwrap_or((None, None))
    }
}

impl DerivedMarketState {
    fn apply_market_event(&mut self, envelope: MarketEventEnvelope) -> Vec<BookDelta> {
        self.sequence = self.sequence.max(envelope.sequence);
        self.push_recent_event(envelope.clone());

        match envelope.event {
            MarketEvent::OrderAdded {
                order_id,
                side,
                price,
                remaining,
                created_at: _,
            } => {
                self.insert_order(DerivedOrder {
                    order_id,
                    side,
                    price,
                    remaining,
                });
                vec![BookDelta::LevelUpdated {
                    side,
                    price,
                    quantity: self.level_quantity(side, price),
                }]
            }
            MarketEvent::OrderUpdated {
                order_id,
                side,
                price,
                remaining,
            } => {
                self.upsert_order(DerivedOrder {
                    order_id,
                    side,
                    price,
                    remaining,
                });
                vec![BookDelta::LevelUpdated {
                    side,
                    price,
                    quantity: self.level_quantity(side, price),
                }]
            }
            MarketEvent::OrderRemoved {
                order_id,
                side,
                price,
                ..
            } => {
                self.remove_order(order_id);
                vec![BookDelta::LevelUpdated {
                    side,
                    price,
                    quantity: self.level_quantity(side, price),
                }]
            }
            MarketEvent::Trade {
                price, quantity, ..
            } => vec![BookDelta::Trade { price, quantity }],
        }
    }

    fn book_snapshot(&self) -> MarketBookSnapshot {
        MarketBookSnapshot {
            bids: self
                .bids
                .iter()
                .rev()
                .filter_map(|(price, quantity)| {
                    (*quantity > 0).then_some(BookLevel {
                        price: *price,
                        quantity: *quantity,
                    })
                })
                .collect(),
            asks: self
                .asks
                .iter()
                .filter_map(|(price, quantity)| {
                    (*quantity > 0).then_some(BookLevel {
                        price: *price,
                        quantity: *quantity,
                    })
                })
                .collect(),
        }
    }

    fn best_prices(&self) -> (Option<u64>, Option<u64>) {
        (
            self.bids.iter().next_back().map(|(price, _)| *price),
            self.asks.iter().next().map(|(price, _)| *price),
        )
    }

    fn level_quantity(&self, side: Side, price: u64) -> u64 {
        match side {
            Side::Buy => self.bids.get(&price).copied().unwrap_or(0),
            Side::Sell => self.asks.get(&price).copied().unwrap_or(0),
        }
    }

    fn push_recent_event(&mut self, envelope: MarketEventEnvelope) {
        self.recent_events.push_back(envelope);
        while self.recent_events.len() > MARKET_EVENT_REPLAY_LIMIT {
            self.recent_events.pop_front();
        }
    }

    fn insert_order(&mut self, order: DerivedOrder) {
        self.adjust_level(order.side, order.price, order.remaining as i64);
        self.orders.insert(order.order_id, order);
    }

    fn upsert_order(&mut self, order: DerivedOrder) {
        if let Some(previous) = self.orders.insert(order.order_id, order.clone()) {
            self.adjust_level(previous.side, previous.price, -(previous.remaining as i64));
        }
        self.adjust_level(order.side, order.price, order.remaining as i64);
    }

    fn remove_order(&mut self, order_id: Uuid) {
        if let Some(previous) = self.orders.remove(&order_id) {
            self.adjust_level(previous.side, previous.price, -(previous.remaining as i64));
        }
    }

    fn adjust_level(&mut self, side: Side, price: u64, delta: i64) {
        let levels = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        let updated = levels
            .get(&price)
            .copied()
            .unwrap_or(0)
            .saturating_add_signed(delta);
        if updated == 0 {
            levels.remove(&price);
        } else {
            levels.insert(price, updated);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketdata::{MarketEvent, MarketEventRemoveReason};
    use chrono::{TimeZone, Utc};

    fn event(sequence: u64, market: &str, event: MarketEvent) -> MarketEventEnvelope {
        MarketEventEnvelope {
            market: market.to_string(),
            sequence,
            recorded_at: Utc.timestamp_opt(sequence as i64, 0).single().unwrap(),
            event,
        }
    }

    #[test]
    fn derived_market_data_builds_book_state_from_canonical_events() {
        let handle = DerivedMarketDataHandle::default();
        let order_id = Uuid::from_u128(1);

        let deltas = handle.apply_market_event(event(
            1,
            "BTC-USD",
            MarketEvent::OrderAdded {
                order_id,
                side: Side::Buy,
                price: 100,
                remaining: 5,
                created_at: Utc.timestamp_opt(1, 0).single().unwrap(),
            },
        ));
        assert_eq!(
            deltas,
            vec![BookDelta::LevelUpdated {
                side: Side::Buy,
                price: 100,
                quantity: 5,
            }]
        );

        let snapshot = handle.book_snapshot("BTC-USD");
        assert_eq!(
            snapshot.bids,
            vec![BookLevel {
                price: 100,
                quantity: 5,
            }]
        );
        assert!(snapshot.asks.is_empty());

        let deltas = handle.apply_market_event(event(
            2,
            "BTC-USD",
            MarketEvent::OrderUpdated {
                order_id,
                side: Side::Buy,
                price: 100,
                remaining: 2,
            },
        ));
        assert_eq!(
            deltas,
            vec![BookDelta::LevelUpdated {
                side: Side::Buy,
                price: 100,
                quantity: 2,
            }]
        );
        let deltas = handle.apply_market_event(event(
            3,
            "BTC-USD",
            MarketEvent::OrderRemoved {
                order_id,
                side: Side::Buy,
                price: 100,
                reason: MarketEventRemoveReason::Canceled,
            },
        ));
        assert_eq!(
            deltas,
            vec![BookDelta::LevelUpdated {
                side: Side::Buy,
                price: 100,
                quantity: 0,
            }]
        );

        assert!(handle.book_snapshot("BTC-USD").bids.is_empty());
    }

    #[test]
    fn derived_market_data_rebuilds_from_open_orders() {
        let handle = DerivedMarketDataHandle::default();
        handle.replace_from_open_orders(
            vec![("BTC-USD".to_string(), 4), ("ETH-USD".to_string(), 9)],
            vec![
                Order {
                    id: Uuid::from_u128(10),
                    trader_id: Uuid::from_u128(100),
                    market: "BTC-USD".to_string(),
                    side: Side::Buy,
                    price: 99,
                    quantity: 7,
                    remaining: 7,
                    created_at: Utc.timestamp_opt(10, 0).single().unwrap(),
                },
                Order {
                    id: Uuid::from_u128(11),
                    trader_id: Uuid::from_u128(101),
                    market: "ETH-USD".to_string(),
                    side: Side::Sell,
                    price: 201,
                    quantity: 3,
                    remaining: 3,
                    created_at: Utc.timestamp_opt(11, 0).single().unwrap(),
                },
            ],
        );

        let btc_snapshot = handle.book_snapshot("BTC-USD");
        assert_eq!(
            btc_snapshot.bids,
            vec![BookLevel {
                price: 99,
                quantity: 7,
            }]
        );
        assert!(btc_snapshot.asks.is_empty());

        let eth_snapshot = handle.book_snapshot("ETH-USD");
        assert!(eth_snapshot.bids.is_empty());
        assert_eq!(
            eth_snapshot.asks,
            vec![BookLevel {
                price: 201,
                quantity: 3,
            }]
        );
    }
}
