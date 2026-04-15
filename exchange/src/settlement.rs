use crate::accounts::UserRole;
#[cfg(test)]
use crate::orderbook::Order;
use crate::orderbook::Side;
use crate::state::{AppState, NET_POSITION_LIMIT, Position};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettlementError {
    #[error("invalid market symbol")]
    InvalidMarket,
    #[error("settlement price must be zero or greater")]
    InvalidSettlementPrice,
    #[error("projected net position for {market} would be {projected}; limit is +/-{limit}")]
    PositionLimitExceeded {
        market: String,
        projected: i64,
        limit: i64,
    },
    #[error("numeric overflow")]
    Overflow,
}

/// Integer average `numerator / denominator` with half-up rounding (`denominator` > 0).
fn u64_average_half_up(numerator: u128, denominator: u64) -> Result<u64, SettlementError> {
    if denominator == 0 {
        return Err(SettlementError::Overflow);
    }
    let d = denominator as u128;
    let q = (numerator + d / 2) / d;
    u64::try_from(q).map_err(|_| SettlementError::Overflow)
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
    pub fn position_limit() -> i64 {
        NET_POSITION_LIMIT
    }

    pub fn position_limit_for_role(role: UserRole) -> Option<i64> {
        if role.has_unlimited_position_power() {
            None
        } else {
            Some(NET_POSITION_LIMIT)
        }
    }

    pub fn ensure_order_within_limit(
        state: &AppState,
        trader_id: Uuid,
        market: &str,
        side: Side,
        quantity: u64,
        skip_order_id: Option<Uuid>,
    ) -> Result<(), SettlementError> {
        validate_market_symbol(market)?;
        if quantity == 0 {
            return Ok(());
        }
        if state
            .storage
            .get_user(trader_id)
            .map(|user| user.profile.role.has_unlimited_position_power())
            .unwrap_or(false)
        {
            return Ok(());
        }

        let current_net = state
            .storage
            .get_position(trader_id, market)
            .map(|position| position.net_quantity)
            .unwrap_or(0);

        let mut pending_buy_quantity = 0_i64;
        let mut pending_sell_quantity = 0_i64;
        for order in state.storage.list_open_orders(trader_id, Some(market)) {
            if skip_order_id == Some(order.id) {
                continue;
            }
            let remaining =
                i64::try_from(order.remaining).map_err(|_| SettlementError::Overflow)?;
            match order.side {
                Side::Buy => {
                    pending_buy_quantity = pending_buy_quantity
                        .checked_add(remaining)
                        .ok_or(SettlementError::Overflow)?;
                }
                Side::Sell => {
                    pending_sell_quantity = pending_sell_quantity
                        .checked_add(remaining)
                        .ok_or(SettlementError::Overflow)?;
                }
            }
        }

        let requested = i64::try_from(quantity).map_err(|_| SettlementError::Overflow)?;
        match side {
            Side::Buy => {
                pending_buy_quantity = pending_buy_quantity
                    .checked_add(requested)
                    .ok_or(SettlementError::Overflow)?;
            }
            Side::Sell => {
                pending_sell_quantity = pending_sell_quantity
                    .checked_add(requested)
                    .ok_or(SettlementError::Overflow)?;
            }
        }

        let max_long = current_net
            .checked_add(pending_buy_quantity)
            .ok_or(SettlementError::Overflow)?;
        if max_long > NET_POSITION_LIMIT {
            return Err(SettlementError::PositionLimitExceeded {
                market: market.to_string(),
                projected: max_long,
                limit: NET_POSITION_LIMIT,
            });
        }

        let max_short = current_net
            .checked_sub(pending_sell_quantity)
            .ok_or(SettlementError::Overflow)?;
        if max_short < -NET_POSITION_LIMIT {
            return Err(SettlementError::PositionLimitExceeded {
                market: market.to_string(),
                projected: max_short,
                limit: NET_POSITION_LIMIT,
            });
        };

        Ok(())
    }

    pub fn projected_bounds_with_open_orders(
        state: &AppState,
        trader_id: Uuid,
        market: &str,
    ) -> Result<(i64, i64), SettlementError> {
        validate_market_symbol(market)?;

        let current_net = state
            .storage
            .get_position(trader_id, market)
            .map(|position| position.net_quantity)
            .unwrap_or(0);

        let mut pending_buy_quantity = 0_i64;
        let mut pending_sell_quantity = 0_i64;
        for order in state.storage.list_open_orders(trader_id, Some(market)) {
            let remaining =
                i64::try_from(order.remaining).map_err(|_| SettlementError::Overflow)?;
            match order.side {
                Side::Buy => {
                    pending_buy_quantity = pending_buy_quantity
                        .checked_add(remaining)
                        .ok_or(SettlementError::Overflow)?;
                }
                Side::Sell => {
                    pending_sell_quantity = pending_sell_quantity
                        .checked_add(remaining)
                        .ok_or(SettlementError::Overflow)?;
                }
            }
        }

        Ok((
            current_net
                .checked_add(pending_buy_quantity)
                .ok_or(SettlementError::Overflow)?,
            current_net
                .checked_sub(pending_sell_quantity)
                .ok_or(SettlementError::Overflow)?,
        ))
    }

    pub fn apply_fill(
        state: &AppState,
        trader_id: Uuid,
        side: Side,
        market: &str,
        fill_price: u64,
        quantity: u64,
    ) -> Result<(), SettlementError> {
        validate_market_symbol(market)?;
        if quantity == 0 {
            return Ok(());
        }

        let mut position = state
            .storage
            .get_position(trader_id, market)
            .unwrap_or_else(|| Position {
                market: market.to_string(),
                net_quantity: 0,
                average_entry_price: None,
                realized_pnl: 0,
                updated_at: Utc::now(),
            });

        apply_fill_to_position(&mut position, side, fill_price, quantity)?;
        position.updated_at = Utc::now();

        if should_persist_position(&position) {
            state.storage.upsert_position(trader_id, position);
        } else {
            let _ = state.storage.delete_position(trader_id, market);
        }
        Ok(())
    }

    pub fn settle_market(
        state: &AppState,
        market: &str,
        settlement_price: u64,
    ) -> Result<MarketSettlementSummary, SettlementError> {
        validate_market_symbol(market)?;

        let mut summary = MarketSettlementSummary {
            affected_traders: 0,
            settled_quantity: 0,
        };

        for (trader_id, mut positions) in state.storage.list_all_positions() {
            let Some(position) = positions
                .iter_mut()
                .find(|position| position.market == market)
            else {
                continue;
            };
            if position.net_quantity == 0 {
                continue;
            }

            summary.affected_traders += 1;
            summary.settled_quantity = summary
                .settled_quantity
                .checked_add(position.net_quantity.unsigned_abs())
                .ok_or(SettlementError::Overflow)?;

            settle_position(position, settlement_price)?;
            position.updated_at = Utc::now();
            normalize_positions(&mut positions);
            state.storage.replace_positions(trader_id, positions);
        }

        Ok(summary)
    }

    pub fn seed_position(
        state: &AppState,
        trader_id: Uuid,
        market: &str,
        net_quantity: i64,
        average_entry_price: Option<u64>,
        realized_pnl: i64,
    ) {
        state.storage.upsert_position(
            trader_id,
            Position {
                market: market.to_string(),
                net_quantity,
                average_entry_price,
                realized_pnl,
                updated_at: Utc::now(),
            },
        );
    }

    pub fn seed_balance(_state: &AppState, _trader_id: Uuid, _asset: &str, _free: u64) {
        // Legacy tests still call this helper, but the runtime no longer uses balances
        // for trading eligibility. Position-based tests should prefer `seed_position`.
    }
}

fn validate_market_symbol(market: &str) -> Result<(), SettlementError> {
    let Some((base, quote)) = market.split_once('-') else {
        return Err(SettlementError::InvalidMarket);
    };
    if base.is_empty() || quote.is_empty() {
        return Err(SettlementError::InvalidMarket);
    }
    Ok(())
}

fn normalize_positions(positions: &mut Vec<Position>) {
    positions.retain(should_persist_position);
    positions.sort_by(|left, right| left.market.cmp(&right.market));
}

pub(crate) fn should_persist_position(position: &Position) -> bool {
    position.net_quantity != 0 || position.realized_pnl != 0
}

pub(crate) fn apply_fill_to_position(
    position: &mut Position,
    side: Side,
    fill_price: u64,
    quantity: u64,
) -> Result<(), SettlementError> {
    let fill_delta = match side {
        Side::Buy => i64::try_from(quantity).map_err(|_| SettlementError::Overflow)?,
        Side::Sell => -i64::try_from(quantity).map_err(|_| SettlementError::Overflow)?,
    };
    let current_net = position.net_quantity;
    let fill_price_i64 = i64::try_from(fill_price).map_err(|_| SettlementError::Overflow)?;

    if current_net == 0 {
        position.net_quantity = fill_delta;
        position.average_entry_price = Some(fill_price);
        return Ok(());
    }

    if current_net.signum() == fill_delta.signum() {
        let current_abs = current_net.unsigned_abs();
        let fill_abs = fill_delta.unsigned_abs();
        let next_abs = current_abs
            .checked_add(fill_abs)
            .ok_or(SettlementError::Overflow)?;
        let average = position.average_entry_price.unwrap_or(fill_price);
        let weighted = average
            .checked_mul(current_abs)
            .and_then(|value| {
                fill_price
                    .checked_mul(fill_abs)
                    .and_then(|delta| value.checked_add(delta))
            })
            .ok_or(SettlementError::Overflow)?;
        position.net_quantity = current_net
            .checked_add(fill_delta)
            .ok_or(SettlementError::Overflow)?;
        position.average_entry_price = Some(u64_average_half_up(weighted as u128, next_abs)?);
        return Ok(());
    }

    let current_abs = current_net.unsigned_abs();
    let fill_abs = fill_delta.unsigned_abs();
    let closed_quantity = current_abs.min(fill_abs);
    let average = position.average_entry_price.unwrap_or(fill_price);
    let average_i64 = i64::try_from(average).map_err(|_| SettlementError::Overflow)?;
    let closed_i64 = i64::try_from(closed_quantity).map_err(|_| SettlementError::Overflow)?;
    let realized_delta = if current_net > 0 {
        fill_price_i64
            .checked_sub(average_i64)
            .and_then(|delta| delta.checked_mul(closed_i64))
            .ok_or(SettlementError::Overflow)?
    } else {
        average_i64
            .checked_sub(fill_price_i64)
            .and_then(|delta| delta.checked_mul(closed_i64))
            .ok_or(SettlementError::Overflow)?
    };
    position.realized_pnl = position
        .realized_pnl
        .checked_add(realized_delta)
        .ok_or(SettlementError::Overflow)?;

    let next_net = current_net
        .checked_add(fill_delta)
        .ok_or(SettlementError::Overflow)?;
    position.net_quantity = next_net;
    position.average_entry_price = if next_net == 0 {
        None
    } else if next_net.signum() == current_net.signum() {
        position.average_entry_price
    } else {
        Some(fill_price)
    };
    Ok(())
}

fn settle_position(position: &mut Position, settlement_price: u64) -> Result<(), SettlementError> {
    if position.net_quantity == 0 {
        return Ok(());
    }
    let average = position.average_entry_price.unwrap_or(settlement_price);
    let settlement_i64 = i64::try_from(settlement_price).map_err(|_| SettlementError::Overflow)?;
    let average_i64 = i64::try_from(average).map_err(|_| SettlementError::Overflow)?;
    let quantity_i64 = i64::try_from(position.net_quantity.unsigned_abs())
        .map_err(|_| SettlementError::Overflow)?;
    let realized_delta = if position.net_quantity > 0 {
        settlement_i64
            .checked_sub(average_i64)
            .and_then(|delta| delta.checked_mul(quantity_i64))
            .ok_or(SettlementError::Overflow)?
    } else {
        average_i64
            .checked_sub(settlement_i64)
            .and_then(|delta| delta.checked_mul(quantity_i64))
            .ok_or(SettlementError::Overflow)?
    };

    position.realized_pnl = position
        .realized_pnl
        .checked_add(realized_delta)
        .ok_or(SettlementError::Overflow)?;
    position.net_quantity = 0;
    position.average_entry_price = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{UserProfile, UserRecord, UserRole};
    use crate::admin::{MarketDefinition, MarketStatus};
    use crate::config::Config;

    fn test_state() -> AppState {
        let state = AppState::new(Config {
            bind_addr: "127.0.0.1:0".to_string(),
            checkpoint_path: None,
            checkpoint_interval_seconds: 5,
            ws_broadcast_buffer: 64,
            ws_market_delta_batch_interval_ms: 10,
            ws_market_broadcast_workers: 1,
            market_data_service_socket: None,
            market_data_service_retry_backoff_ms: 250,
            runtime_dispatch_queue_capacity: 4_096,
            account_dispatch_queue_capacity: 4_096,
            per_user_rate_limit_burst_capacity: 500,
            per_user_rate_limit_burst_window_seconds: 10,
            admin_api_token: "test-admin-token".to_string(),
        });
        let now = Utc::now();
        state.storage.upsert_market(MarketDefinition {
            market_id: "BTC-USD".to_string(),
            display_name: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USD".to_string(),
            tick_size: 1,
            min_order_quantity: 1,
            min_price: None,
            max_price: None,
            reference_price: Some(100),
            settlement_price: None,
            status: MarketStatus::Enabled,
            created_at: now,
            updated_at: now,
        });
        state
    }

    #[test]
    fn reject_when_same_side_resting_orders_would_break_limit() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_position(&state, trader_id, "BTC-USD", 900, Some(100), 0);
        state.storage.upsert_open_order(
            trader_id,
            Order {
                id: Uuid::new_v4(),
                trader_id,
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 50,
                remaining: 50,
                created_at: Utc::now(),
            },
        );

        let error = SettlementEngine::ensure_order_within_limit(
            &state,
            trader_id,
            "BTC-USD",
            Side::Buy,
            75,
            None,
        )
        .expect_err("should reject");

        assert!(matches!(
            error,
            SettlementError::PositionLimitExceeded {
                projected: 1_025,
                ..
            }
        ));
    }

    #[test]
    fn reject_when_existing_open_orders_already_imply_limit_breach() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_position(&state, trader_id, "BTC-USD", 950, Some(100), 0);
        state.storage.upsert_open_order(
            trader_id,
            Order {
                id: Uuid::new_v4(),
                trader_id,
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 100,
                remaining: 100,
                created_at: Utc::now(),
            },
        );

        let error = SettlementEngine::ensure_order_within_limit(
            &state,
            trader_id,
            "BTC-USD",
            Side::Sell,
            1,
            None,
        )
        .expect_err("should reject when existing worst-case exposure is already invalid");

        assert!(matches!(
            error,
            SettlementError::PositionLimitExceeded {
                projected: 1_050,
                ..
            }
        ));
    }

    #[test]
    fn admin_trader_is_exempt_from_position_limit_checks() {
        let state = test_state();
        let trader = UserRecord {
            profile: UserProfile {
                trader_id: Uuid::new_v4(),
                username: "desk-admin".to_string(),
                team_number: "desk-admin".to_string(),
                api_key: "exch_admin".to_string(),
                role: UserRole::Admin,
                created_at: Utc::now(),
            },
        };
        state
            .storage
            .create_user(trader.clone())
            .expect("admin user");

        SettlementEngine::ensure_order_within_limit(
            &state,
            trader.profile.trader_id,
            "BTC-USD",
            Side::Buy,
            (NET_POSITION_LIMIT + 10_000) as u64,
            None,
        )
        .expect("admin trader should bypass fixed position checks");
    }

    #[test]
    fn projected_bounds_include_existing_open_orders() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_position(&state, trader_id, "BTC-USD", 100, Some(100), 0);
        state.storage.upsert_open_order(
            trader_id,
            Order {
                id: Uuid::new_v4(),
                trader_id,
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                price: 100,
                quantity: 75,
                remaining: 75,
                created_at: Utc::now(),
            },
        );
        state.storage.upsert_open_order(
            trader_id,
            Order {
                id: Uuid::new_v4(),
                trader_id,
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                price: 101,
                quantity: 40,
                remaining: 40,
                created_at: Utc::now(),
            },
        );

        let bounds =
            SettlementEngine::projected_bounds_with_open_orders(&state, trader_id, "BTC-USD")
                .expect("bounds should compute");

        assert_eq!(bounds, (175, 60));
    }

    #[test]
    fn fill_updates_short_position_and_realized_pnl() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_position(&state, trader_id, "BTC-USD", -10, Some(100), 5);

        SettlementEngine::apply_fill(&state, trader_id, Side::Buy, "BTC-USD", 90, 4)
            .expect("fill should apply");

        let positions = state.storage.list_positions(trader_id);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].net_quantity, -6);
        assert_eq!(positions[0].average_entry_price, Some(100));
        assert_eq!(positions[0].realized_pnl, 45);
    }

    #[test]
    fn flat_zero_pnl_fill_removes_position() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_position(&state, trader_id, "BTC-USD", 1, Some(100), 0);

        SettlementEngine::apply_fill(&state, trader_id, Side::Sell, "BTC-USD", 100, 1)
            .expect("fill should flatten position");

        assert!(state.storage.list_positions(trader_id).is_empty());
    }

    #[test]
    fn settlement_flattens_positions_and_realizes_pnl() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        SettlementEngine::seed_position(&state, trader_id, "BTC-USD", 5, Some(80), 10);

        let summary =
            SettlementEngine::settle_market(&state, "BTC-USD", 90).expect("settlement should work");

        assert_eq!(summary.affected_traders, 1);
        assert_eq!(summary.settled_quantity, 5);
        let positions = state.storage.list_positions(trader_id);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].net_quantity, 0);
        assert_eq!(positions[0].average_entry_price, None);
        assert_eq!(positions[0].realized_pnl, 60);
    }
}
