use crate::orderbook::{Order, Side};
use crate::state::{AppState, Balance};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettlementError {
    #[error("invalid market symbol")]
    InvalidMarket,
    #[error("insufficient free balance for asset {asset}")]
    InsufficientFreeBalance { asset: String },
    #[error("insufficient locked balance for asset {asset}")]
    InsufficientLockedBalance { asset: String },
    #[error("numeric overflow")]
    Overflow,
}

pub struct SettlementEngine;

impl SettlementEngine {
    pub fn lock_order(state: &AppState, order: &Order) -> Result<(), SettlementError> {
        let (asset, amount) = hold_requirement(order)?;
        if amount == 0 {
            return Ok(());
        }

        let mut balances = state.storage.list_balances(order.trader_id);
        let balance = ensure_balance_entry(&mut balances, &asset);
        if balance.free < amount {
            return Err(SettlementError::InsufficientFreeBalance { asset });
        }

        balance.free -= amount;
        balance.locked = balance
            .locked
            .checked_add(amount)
            .ok_or(SettlementError::Overflow)?;
        state.storage.replace_balances(order.trader_id, balances);
        Ok(())
    }

    pub fn release_order_hold(state: &AppState, order: &Order) -> Result<(), SettlementError> {
        let (asset, amount) = hold_requirement(order)?;
        if amount == 0 {
            return Ok(());
        }

        let mut balances = state.storage.list_balances(order.trader_id);
        let balance = ensure_balance_entry(&mut balances, &asset);
        if balance.locked < amount {
            return Err(SettlementError::InsufficientLockedBalance { asset });
        }

        balance.locked -= amount;
        balance.free = balance
            .free
            .checked_add(amount)
            .ok_or(SettlementError::Overflow)?;
        state.storage.replace_balances(order.trader_id, balances);
        Ok(())
    }

    pub fn apply_fill(
        state: &AppState,
        trader_id: Uuid,
        side: Side,
        market: &str,
        limit_price: u64,
        fill_price: u64,
        quantity: u64,
    ) -> Result<(), SettlementError> {
        if quantity == 0 {
            return Ok(());
        }

        let (base_asset, quote_asset) = parse_market(market)?;
        let fill_quote = fill_price
            .checked_mul(quantity)
            .ok_or(SettlementError::Overflow)?;

        let mut balances = state.storage.list_balances(trader_id);
        match side {
            Side::Buy => {
                let locked_quote = limit_price
                    .checked_mul(quantity)
                    .ok_or(SettlementError::Overflow)?;
                let quote = ensure_balance_entry(&mut balances, &quote_asset);
                if quote.locked < locked_quote {
                    return Err(SettlementError::InsufficientLockedBalance { asset: quote_asset });
                }
                quote.locked -= locked_quote;
                let quote_refund = locked_quote
                    .checked_sub(fill_quote)
                    .ok_or(SettlementError::Overflow)?;
                quote.free = quote
                    .free
                    .checked_add(quote_refund)
                    .ok_or(SettlementError::Overflow)?;

                let base = ensure_balance_entry(&mut balances, &base_asset);
                base.free = base
                    .free
                    .checked_add(quantity)
                    .ok_or(SettlementError::Overflow)?;
            }
            Side::Sell => {
                let base = ensure_balance_entry(&mut balances, &base_asset);
                if base.locked < quantity {
                    return Err(SettlementError::InsufficientLockedBalance { asset: base_asset });
                }
                base.locked -= quantity;

                let quote = ensure_balance_entry(&mut balances, &quote_asset);
                quote.free = quote
                    .free
                    .checked_add(fill_quote)
                    .ok_or(SettlementError::Overflow)?;
            }
        }

        state.storage.replace_balances(trader_id, balances);
        Ok(())
    }

    pub fn seed_balance(state: &AppState, trader_id: Uuid, asset: &str, free: u64) {
        state.storage.put_balance(
            trader_id,
            Balance {
                asset: asset.to_string(),
                free,
                locked: 0,
            },
        );
    }
}

fn hold_requirement(order: &Order) -> Result<(String, u64), SettlementError> {
    let (base_asset, quote_asset) = parse_market(&order.market)?;
    match order.side {
        Side::Buy => Ok((
            quote_asset,
            order
                .price
                .checked_mul(order.remaining)
                .ok_or(SettlementError::Overflow)?,
        )),
        Side::Sell => Ok((base_asset, order.remaining)),
    }
}

fn parse_market(market: &str) -> Result<(String, String), SettlementError> {
    let Some((base, quote)) = market.split_once('-') else {
        return Err(SettlementError::InvalidMarket);
    };
    if base.is_empty() || quote.is_empty() {
        return Err(SettlementError::InvalidMarket);
    }
    Ok((base.to_string(), quote.to_string()))
}

fn ensure_balance_entry<'a>(balances: &'a mut Vec<Balance>, asset: &str) -> &'a mut Balance {
    if let Some(idx) = balances.iter().position(|balance| balance.asset == asset) {
        return &mut balances[idx];
    }

    balances.push(Balance {
        asset: asset.to_string(),
        free: 0,
        locked: 0,
    });
    balances
        .last_mut()
        .expect("balance entry was just inserted")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use chrono::Utc;

    fn test_state() -> AppState {
        AppState::new(Config {
            bind_addr: "127.0.0.1:0".to_string(),
            database_url: "postgres://test".to_string(),
            storage_backend: crate::storage::StorageBackendKind::InMemory,
            ws_broadcast_buffer: 64,
            per_user_requests_per_second: 100,
            admin_api_token: "test-admin-token".to_string(),
            postgres_write_batch_size: 128,
            postgres_write_flush_interval_ms: 25,
            postgres_write_queue_capacity: 4_096,
            postgres_write_retry_backoff_ms: 250,
        })
    }

    fn test_order(trader_id: Uuid, side: Side, price: u64, quantity: u64) -> Order {
        Order {
            id: Uuid::new_v4(),
            trader_id,
            market: "BTC-USD".to_string(),
            side,
            price,
            quantity,
            remaining: quantity,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn locking_buy_order_moves_quote_balance_from_free_to_locked() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, trader_id, "USD", 1_000);
        let order = test_order(trader_id, Side::Buy, 100, 3);

        SettlementEngine::lock_order(&state, &order).expect("lock should succeed");

        let balances = state.storage.list_balances(trader_id);
        let usd = balances
            .iter()
            .find(|balance| balance.asset == "USD")
            .expect("usd balance");
        assert_eq!(usd.free, 700);
        assert_eq!(usd.locked, 300);
    }

    #[test]
    fn releasing_sell_order_restores_locked_base_balance() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, trader_id, "BTC", 5);
        let order = test_order(trader_id, Side::Sell, 100, 2);
        SettlementEngine::lock_order(&state, &order).expect("lock should succeed");

        SettlementEngine::release_order_hold(&state, &order).expect("release should succeed");

        let balances = state.storage.list_balances(trader_id);
        let btc = balances
            .iter()
            .find(|balance| balance.asset == "BTC")
            .expect("btc balance");
        assert_eq!(btc.free, 5);
        assert_eq!(btc.locked, 0);
    }

    #[test]
    fn applying_buy_fill_releases_price_improvement_and_credits_base() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, trader_id, "USD", 1_000);
        let order = test_order(trader_id, Side::Buy, 105, 2);
        SettlementEngine::lock_order(&state, &order).expect("lock should succeed");

        SettlementEngine::apply_fill(&state, trader_id, Side::Buy, "BTC-USD", 105, 100, 2)
            .expect("fill should settle");

        let balances = state.storage.list_balances(trader_id);
        let usd = balances
            .iter()
            .find(|balance| balance.asset == "USD")
            .expect("usd balance");
        let btc = balances
            .iter()
            .find(|balance| balance.asset == "BTC")
            .expect("btc balance");
        assert_eq!(usd.free, 800);
        assert_eq!(usd.locked, 0);
        assert_eq!(btc.free, 2);
    }
}
