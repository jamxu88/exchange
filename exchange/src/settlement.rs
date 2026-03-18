use crate::orderbook::{Order, Side};
use crate::state::{AppState, Balance};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettlementError {
    #[error("invalid market symbol")]
    InvalidMarket,
    #[error("settlement price must be greater than zero")]
    InvalidSettlementPrice,
    #[error("insufficient free balance for asset {asset}")]
    InsufficientFreeBalance { asset: String },
    #[error("insufficient locked balance for asset {asset}")]
    InsufficientLockedBalance { asset: String },
    #[error(
        "persisted balances for trader {trader_id} cannot cover recovered locked amount for asset {asset} (total={total}, required_locked={required_locked})"
    )]
    RecoveryBalanceMismatch {
        trader_id: Uuid,
        asset: String,
        total: u64,
        required_locked: u64,
    },
    #[error("numeric overflow")]
    Overflow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettlementJournalReason {
    BalanceSeeded,
    OrderHoldLocked,
    OrderHoldReleased,
    FillSettled,
    MarketSettled,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct SettlementJournalEntry {
    pub journal_id: Uuid,
    pub trader_id: Uuid,
    pub asset: String,
    pub free_delta: i64,
    pub locked_delta: i64,
    pub reason: SettlementJournalReason,
    pub order_id: Option<Uuid>,
    pub fill_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketSettlementSummary {
    pub affected_traders: usize,
    pub settled_quantity: u64,
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

        persist_settlement_update(
            state,
            order.trader_id,
            balances,
            vec![SettlementJournalEntry {
                journal_id: Uuid::new_v4(),
                trader_id: order.trader_id,
                asset,
                free_delta: negative_delta(amount)?,
                locked_delta: positive_delta(amount)?,
                reason: SettlementJournalReason::OrderHoldLocked,
                order_id: Some(order.id),
                fill_id: None,
                occurred_at: Utc::now(),
            }],
        );
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

        persist_settlement_update(
            state,
            order.trader_id,
            balances,
            vec![SettlementJournalEntry {
                journal_id: Uuid::new_v4(),
                trader_id: order.trader_id,
                asset,
                free_delta: positive_delta(amount)?,
                locked_delta: negative_delta(amount)?,
                reason: SettlementJournalReason::OrderHoldReleased,
                order_id: Some(order.id),
                fill_id: None,
                occurred_at: Utc::now(),
            }],
        );
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
        fill_id: Uuid,
    ) -> Result<(), SettlementError> {
        if quantity == 0 {
            return Ok(());
        }

        let (base_asset, quote_asset) = parse_market(market)?;
        let fill_quote = fill_price
            .checked_mul(quantity)
            .ok_or(SettlementError::Overflow)?;

        let mut balances = state.storage.list_balances(trader_id);
        let occurred_at = Utc::now();
        let mut journal_entries = Vec::with_capacity(2);

        match side {
            Side::Buy => {
                let locked_quote = limit_price
                    .checked_mul(quantity)
                    .ok_or(SettlementError::Overflow)?;
                let quote = ensure_balance_entry(&mut balances, &quote_asset);
                if quote.locked < locked_quote {
                    return Err(SettlementError::InsufficientLockedBalance {
                        asset: quote_asset.clone(),
                    });
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

                journal_entries.push(SettlementJournalEntry {
                    journal_id: Uuid::new_v4(),
                    trader_id,
                    asset: quote_asset.clone(),
                    free_delta: positive_delta(quote_refund)?,
                    locked_delta: negative_delta(locked_quote)?,
                    reason: SettlementJournalReason::FillSettled,
                    order_id: None,
                    fill_id: Some(fill_id),
                    occurred_at,
                });
                journal_entries.push(SettlementJournalEntry {
                    journal_id: Uuid::new_v4(),
                    trader_id,
                    asset: base_asset.clone(),
                    free_delta: positive_delta(quantity)?,
                    locked_delta: 0,
                    reason: SettlementJournalReason::FillSettled,
                    order_id: None,
                    fill_id: Some(fill_id),
                    occurred_at,
                });
            }
            Side::Sell => {
                let base = ensure_balance_entry(&mut balances, &base_asset);
                if base.locked < quantity {
                    return Err(SettlementError::InsufficientLockedBalance {
                        asset: base_asset.clone(),
                    });
                }
                base.locked -= quantity;

                let quote = ensure_balance_entry(&mut balances, &quote_asset);
                quote.free = quote
                    .free
                    .checked_add(fill_quote)
                    .ok_or(SettlementError::Overflow)?;

                journal_entries.push(SettlementJournalEntry {
                    journal_id: Uuid::new_v4(),
                    trader_id,
                    asset: base_asset.clone(),
                    free_delta: 0,
                    locked_delta: negative_delta(quantity)?,
                    reason: SettlementJournalReason::FillSettled,
                    order_id: None,
                    fill_id: Some(fill_id),
                    occurred_at,
                });
                journal_entries.push(SettlementJournalEntry {
                    journal_id: Uuid::new_v4(),
                    trader_id,
                    asset: quote_asset.clone(),
                    free_delta: positive_delta(fill_quote)?,
                    locked_delta: 0,
                    reason: SettlementJournalReason::FillSettled,
                    order_id: None,
                    fill_id: Some(fill_id),
                    occurred_at,
                });
            }
        }

        persist_settlement_update(state, trader_id, balances, journal_entries);
        Ok(())
    }

    pub fn seed_balance(state: &AppState, trader_id: Uuid, asset: &str, free: u64) {
        persist_settlement_update(
            state,
            trader_id,
            vec![Balance {
                asset: asset.to_string(),
                free,
                locked: 0,
            }],
            vec![SettlementJournalEntry {
                journal_id: Uuid::new_v4(),
                trader_id,
                asset: asset.to_string(),
                free_delta: positive_delta(free).expect("seed balance should fit i64"),
                locked_delta: 0,
                reason: SettlementJournalReason::BalanceSeeded,
                order_id: None,
                fill_id: None,
                occurred_at: Utc::now(),
            }],
        );
    }

    pub fn reconcile_balances_after_restart(
        state: &AppState,
        open_orders: &[Order],
    ) -> Result<(), SettlementError> {
        use std::collections::{BTreeMap, BTreeSet};

        let mut expected_locks: BTreeMap<Uuid, BTreeMap<String, u64>> = BTreeMap::new();
        for order in open_orders {
            let (asset, amount) = hold_requirement(order)?;
            let trader_locks = expected_locks.entry(order.trader_id).or_default();
            let entry = trader_locks.entry(asset).or_insert(0);
            *entry = entry.checked_add(amount).ok_or(SettlementError::Overflow)?;
        }

        let all_balances = state.storage.list_all_balances();
        let mut affected_traders = BTreeSet::new();
        affected_traders.extend(all_balances.iter().map(|(trader_id, _)| *trader_id));
        affected_traders.extend(expected_locks.keys().copied());

        for trader_id in affected_traders {
            let mut balances = all_balances
                .iter()
                .find(|(candidate, _)| *candidate == trader_id)
                .map(|(_, balances)| balances.clone())
                .unwrap_or_default();
            let expected_for_trader = expected_locks.remove(&trader_id).unwrap_or_default();
            let before = balances.clone();

            for balance in &mut balances {
                if !expected_for_trader.contains_key(&balance.asset) {
                    let total = balance
                        .free
                        .checked_add(balance.locked)
                        .ok_or(SettlementError::Overflow)?;
                    balance.free = total;
                    balance.locked = 0;
                }
            }

            for (asset, required_locked) in expected_for_trader {
                let balance = ensure_balance_entry(&mut balances, &asset);
                let total = balance
                    .free
                    .checked_add(balance.locked)
                    .ok_or(SettlementError::Overflow)?;
                if total < required_locked {
                    return Err(SettlementError::RecoveryBalanceMismatch {
                        trader_id,
                        asset,
                        total,
                        required_locked,
                    });
                }
                balance.free = total - required_locked;
                balance.locked = required_locked;
            }

            balances.sort_by(|left, right| left.asset.cmp(&right.asset));
            if balances != before {
                state.storage.replace_balances(trader_id, balances);
            }
        }

        Ok(())
    }

    pub fn settle_market(
        state: &AppState,
        market: &str,
        settlement_price: u64,
    ) -> Result<MarketSettlementSummary, SettlementError> {
        if settlement_price == 0 {
            return Err(SettlementError::InvalidSettlementPrice);
        }
        let (base_asset, quote_asset) = parse_market(market)?;
        let all_balances = state.storage.list_all_balances();
        let occurred_at = Utc::now();
        let mut affected_traders = 0_usize;
        let mut settled_quantity = 0_u64;

        for (trader_id, mut balances) in all_balances {
            let Some(index) = balances.iter().position(|balance| balance.asset == base_asset) else {
                continue;
            };
            let base_free = balances[index].free;
            let base_locked = balances[index].locked;
            let total_base = base_free
                .checked_add(base_locked)
                .ok_or(SettlementError::Overflow)?;
            if total_base == 0 {
                continue;
            }

            let payout = total_base
                .checked_mul(settlement_price)
                .ok_or(SettlementError::Overflow)?;
            balances[index].free = 0;
            balances[index].locked = 0;
            let quote = ensure_balance_entry(&mut balances, &quote_asset);
            quote.free = quote
                .free
                .checked_add(payout)
                .ok_or(SettlementError::Overflow)?;

            persist_settlement_update(
                state,
                trader_id,
                balances,
                vec![
                    SettlementJournalEntry {
                        journal_id: Uuid::new_v4(),
                        trader_id,
                        asset: base_asset.clone(),
                        free_delta: negative_delta(base_free)?,
                        locked_delta: negative_delta(base_locked)?,
                        reason: SettlementJournalReason::MarketSettled,
                        order_id: None,
                        fill_id: None,
                        occurred_at,
                    },
                    SettlementJournalEntry {
                        journal_id: Uuid::new_v4(),
                        trader_id,
                        asset: quote_asset.clone(),
                        free_delta: positive_delta(payout)?,
                        locked_delta: 0,
                        reason: SettlementJournalReason::MarketSettled,
                        order_id: None,
                        fill_id: None,
                        occurred_at,
                    },
                ],
            );
            affected_traders += 1;
            settled_quantity = settled_quantity
                .checked_add(total_base)
                .ok_or(SettlementError::Overflow)?;
        }

        Ok(MarketSettlementSummary {
            affected_traders,
            settled_quantity,
        })
    }
}

fn persist_settlement_update(
    state: &AppState,
    trader_id: Uuid,
    balances: Vec<Balance>,
    journal_entries: Vec<SettlementJournalEntry>,
) {
    state
        .storage
        .apply_settlement_update(trader_id, balances, journal_entries);
}

fn positive_delta(value: u64) -> Result<i64, SettlementError> {
    i64::try_from(value).map_err(|_| SettlementError::Overflow)
}

fn negative_delta(value: u64) -> Result<i64, SettlementError> {
    let positive = positive_delta(value)?;
    positive.checked_neg().ok_or(SettlementError::Overflow)
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
    use crate::storage::StorageRepository;

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

    fn test_config() -> Config {
        Config {
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
        }
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
        assert_eq!(state.storage.list_settlement_journal().len(), 2);
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
        assert_eq!(state.storage.list_settlement_journal().len(), 3);
    }

    #[test]
    fn applying_buy_fill_releases_price_improvement_and_credits_base() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_balance(&state, trader_id, "USD", 1_000);
        let order = test_order(trader_id, Side::Buy, 105, 2);
        SettlementEngine::lock_order(&state, &order).expect("lock should succeed");

        SettlementEngine::apply_fill(
            &state,
            trader_id,
            Side::Buy,
            "BTC-USD",
            105,
            100,
            2,
            Uuid::new_v4(),
        )
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
        assert_eq!(state.storage.list_settlement_journal().len(), 4);
    }

    #[test]
    fn reconcile_balances_releases_stale_locks_without_open_orders() {
        let storage = StorageRepository::new_in_memory();
        let trader_id = Uuid::new_v4();
        storage.put_balance(
            trader_id,
            Balance {
                asset: "USD".to_string(),
                free: 700,
                locked: 300,
            },
        );

        let state = AppState::with_storage(test_config(), storage);

        let usd = state
            .storage
            .list_balances(trader_id)
            .into_iter()
            .find(|balance| balance.asset == "USD")
            .expect("usd balance");
        assert_eq!(usd.free, 1_000);
        assert_eq!(usd.locked, 0);
    }

    #[test]
    fn reconcile_balances_restores_locked_amount_for_recovered_open_orders() {
        let storage = StorageRepository::new_in_memory();
        let trader_id = Uuid::new_v4();
        storage.put_balance(
            trader_id,
            Balance {
                asset: "USD".to_string(),
                free: 1_000,
                locked: 0,
            },
        );
        storage.upsert_open_order(
            trader_id,
            Order {
                id: Uuid::new_v4(),
                trader_id,
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 2,
                remaining: 2,
                created_at: Utc::now(),
            },
        );

        let state = AppState::with_storage(test_config(), storage);

        let usd = state
            .storage
            .list_balances(trader_id)
            .into_iter()
            .find(|balance| balance.asset == "USD")
            .expect("usd balance");
        assert_eq!(usd.free, 800);
        assert_eq!(usd.locked, 200);
    }
}
