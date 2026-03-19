use crate::auth::AuthenticatedAdmin;
use crate::marketdata::{
    BookDelta, BroadcastEvent, OrderStateStatus, ServerMessage, UserBroadcastEvent,
};
use crate::settlement::{SettlementEngine, SettlementError};
use crate::state::AppState;
use crate::storage::PersistenceStatus;
use crate::trading::{TradingError, TradingService};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminAuditEntry {
    pub audit_id: Uuid,
    pub actor_username: String,
    pub action: String,
    pub target_username: Option<String>,
    pub target_trader_id: Option<Uuid>,
    pub details: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketStatus {
    Enabled,
    Disabled,
    Settled,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct ExchangeControls {
    pub trading_enabled: bool,
    pub updated_at: DateTime<Utc>,
}

impl Default for ExchangeControls {
    fn default() -> Self {
        Self {
            trading_enabled: true,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct MarketDefinition {
    pub market_id: String,
    pub display_name: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub tick_size: u64,
    pub min_order_quantity: u64,
    pub reference_price: Option<u64>,
    pub settlement_price: Option<u64>,
    pub status: MarketStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdminMessageLevel {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct AdminMessageEntry {
    pub message_id: Uuid,
    pub target_username: Option<String>,
    pub target_trader_id: Option<Uuid>,
    pub market: Option<String>,
    pub level: AdminMessageLevel,
    pub title: Option<String>,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertMarketRequest {
    pub market_id: String,
    pub display_name: Option<String>,
    pub base_asset: String,
    pub quote_asset: String,
    pub tick_size: u64,
    pub min_order_quantity: u64,
    pub reference_price: Option<u64>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateMarketRequest {
    pub display_name: Option<String>,
    pub tick_size: Option<u64>,
    pub min_order_quantity: Option<u64>,
    pub reference_price: Option<u64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoadExchangeConfigRequest {
    pub trading_enabled: Option<bool>,
    pub markets: Vec<UpsertMarketRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoadExchangeConfigResponse {
    pub controls: ExchangeControls,
    pub markets: Vec<MarketDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SendAdminMessageRequest {
    pub target_username: Option<String>,
    pub market: Option<String>,
    pub level: AdminMessageLevel,
    pub title: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SettleMarketRequest {
    pub settlement_price: u64,
    pub announcement: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SettleMarketResponse {
    pub market: MarketDefinition,
    pub canceled_orders: usize,
    pub affected_traders: usize,
    pub settled_quantity: u64,
    pub settlement_price: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeaderboardRow {
    pub rank: usize,
    pub trader_id: Uuid,
    pub username: String,
    pub net_pnl: i64,
    pub realized_pnl: i64,
    pub unrealized_pnl: i64,
    pub gross_exposure: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminStateResponse {
    pub controls: ExchangeControls,
    pub markets: Vec<MarketDefinition>,
    pub recent_messages: Vec<AdminMessageEntry>,
    pub persistence: PersistenceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeleteMarketResponse {
    pub market_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TradingControlResponse {
    pub controls: ExchangeControls,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResetUsersResponse {
    pub cleared_orders: usize,
    pub cleared_positions: usize,
    pub cleared_fills: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ListQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdminError {
    #[error("market id is required")]
    MissingMarketId,
    #[error("market id must match {expected}")]
    MarketIdMismatch { expected: String },
    #[error("base asset is required")]
    MissingBaseAsset,
    #[error("quote asset is required")]
    MissingQuoteAsset,
    #[error("tick size must be greater than zero")]
    InvalidTickSize,
    #[error("minimum order quantity must be greater than zero")]
    InvalidMinimumOrderQuantity,
    #[error("market not found")]
    MarketNotFound,
    #[error("market already settled")]
    MarketAlreadySettled,
    #[error("cannot delete market with open orders")]
    MarketHasOpenOrders,
    #[error("message body is required")]
    MissingMessageBody,
    #[error("target user not found")]
    TargetUserNotFound,
    #[error("settlement price must be greater than zero")]
    InvalidSettlementPrice,
    #[error("numeric overflow")]
    Overflow,
    #[error("{0}")]
    SettlementFailed(String),
}

impl AdminError {
    pub fn status_code(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;

        match self {
            Self::MarketNotFound | Self::TargetUserNotFound => StatusCode::NOT_FOUND,
            Self::MarketAlreadySettled | Self::MarketHasOpenOrders | Self::SettlementFailed(_) => {
                StatusCode::CONFLICT
            }
            Self::Overflow => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MissingMarketId
            | Self::MarketIdMismatch { .. }
            | Self::MissingBaseAsset
            | Self::MissingQuoteAsset
            | Self::InvalidTickSize
            | Self::InvalidMinimumOrderQuantity
            | Self::MissingMessageBody
            | Self::InvalidSettlementPrice => StatusCode::BAD_REQUEST,
        }
    }
}

impl From<SettlementError> for AdminError {
    fn from(value: SettlementError) -> Self {
        match value {
            SettlementError::InvalidMarket => Self::MissingMarketId,
            SettlementError::PositionLimitExceeded { .. } => {
                Self::SettlementFailed(value.to_string())
            }
            SettlementError::Overflow => Self::Overflow,
            SettlementError::InvalidSettlementPrice => Self::InvalidSettlementPrice,
        }
    }
}

pub struct AdminService;

impl AdminService {
    pub fn get_state(state: &AppState, message_limit: usize) -> AdminStateResponse {
        AdminStateResponse {
            controls: state.storage.get_exchange_controls(),
            markets: state.storage.list_markets(),
            recent_messages: state.storage.list_admin_messages(Some(message_limit)),
            persistence: state.storage.persistence_status(),
        }
    }

    pub fn set_trading_enabled(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        trading_enabled: bool,
    ) -> TradingControlResponse {
        let controls = ExchangeControls {
            trading_enabled,
            updated_at: Utc::now(),
        };
        state.storage.set_exchange_controls(controls.clone());
        record_admin_audit(
            state,
            admin.username.clone(),
            if trading_enabled {
                "start_trading"
            } else {
                "stop_trading"
            },
            None,
            None,
            format!("trading_enabled set to {trading_enabled}"),
        );

        TradingControlResponse { controls }
    }

    pub fn list_markets(state: &AppState) -> Vec<MarketDefinition> {
        state.storage.list_markets()
    }

    pub fn list_admin_messages(state: &AppState, limit: usize) -> Vec<AdminMessageEntry> {
        state.storage.list_admin_messages(Some(limit))
    }

    pub fn upsert_market(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        request: UpsertMarketRequest,
    ) -> Result<MarketDefinition, AdminError> {
        let existing = state.storage.get_market(&request.market_id);
        if existing
            .as_ref()
            .map(|market| market.status == MarketStatus::Settled)
            == Some(true)
        {
            return Err(AdminError::MarketAlreadySettled);
        }

        let market = build_market_definition(existing.as_ref(), request)?;
        state.storage.upsert_market(market.clone());
        state
            .market_sequences
            .entry(market.market_id.clone())
            .or_insert(0);
        record_admin_audit(
            state,
            admin.username.clone(),
            if existing.is_some() {
                "update_market"
            } else {
                "create_market"
            },
            Some(market.market_id.clone()),
            None,
            format!(
                "market {} status={:?} tick_size={} min_order_quantity={}",
                market.market_id, market.status, market.tick_size, market.min_order_quantity
            ),
        );

        Ok(market)
    }

    pub fn update_market(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        market_id: &str,
        request: UpdateMarketRequest,
    ) -> Result<MarketDefinition, AdminError> {
        let mut market = state
            .storage
            .get_market(market_id)
            .ok_or(AdminError::MarketNotFound)?;
        if market.status == MarketStatus::Settled {
            return Err(AdminError::MarketAlreadySettled);
        }
        if let Some(display_name) = request.display_name {
            let trimmed = display_name.trim();
            market.display_name = if trimmed.is_empty() {
                market.market_id.clone()
            } else {
                trimmed.to_string()
            };
        }
        if let Some(tick_size) = request.tick_size {
            if tick_size == 0 {
                return Err(AdminError::InvalidTickSize);
            }
            market.tick_size = tick_size;
        }
        if let Some(min_order_quantity) = request.min_order_quantity {
            if min_order_quantity == 0 {
                return Err(AdminError::InvalidMinimumOrderQuantity);
            }
            market.min_order_quantity = min_order_quantity;
        }
        if let Some(reference_price) = request.reference_price {
            market.reference_price = Some(reference_price);
        }
        if let Some(enabled) = request.enabled {
            market.status = if enabled {
                MarketStatus::Enabled
            } else {
                MarketStatus::Disabled
            };
        }
        market.updated_at = Utc::now();
        state.storage.upsert_market(market.clone());
        record_admin_audit(
            state,
            admin.username.clone(),
            "patch_market",
            Some(market.market_id.clone()),
            None,
            format!(
                "market {} status={:?} tick_size={} min_order_quantity={}",
                market.market_id, market.status, market.tick_size, market.min_order_quantity
            ),
        );

        Ok(market)
    }

    pub fn delete_market(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        market_id: &str,
    ) -> Result<DeleteMarketResponse, AdminError> {
        let trimmed_market = market_id.trim();
        if trimmed_market.is_empty() {
            return Err(AdminError::MissingMarketId);
        }
        if state
            .storage
            .list_all_open_orders()
            .iter()
            .any(|order| order.market == trimmed_market)
        {
            return Err(AdminError::MarketHasOpenOrders);
        }
        state
            .storage
            .delete_market(trimmed_market)
            .ok_or(AdminError::MarketNotFound)?;
        state.orderbooks.remove(trimmed_market);
        state.market_sequences.remove(trimmed_market);
        record_admin_audit(
            state,
            admin.username.clone(),
            "delete_market",
            Some(trimmed_market.to_string()),
            None,
            format!("deleted market {trimmed_market}"),
        );
        Ok(DeleteMarketResponse {
            market_id: trimmed_market.to_string(),
        })
    }

    pub fn load_exchange_config(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        request: LoadExchangeConfigRequest,
    ) -> Result<LoadExchangeConfigResponse, AdminError> {
        let controls = if let Some(trading_enabled) = request.trading_enabled {
            let controls = ExchangeControls {
                trading_enabled,
                updated_at: Utc::now(),
            };
            state.storage.set_exchange_controls(controls.clone());
            controls
        } else {
            state.storage.get_exchange_controls()
        };

        let mut markets = Vec::with_capacity(request.markets.len());
        for market in request.markets {
            markets.push(Self::upsert_market(state, admin, market)?);
        }

        if markets.is_empty() {
            markets = state.storage.list_markets();
        }

        record_admin_audit(
            state,
            admin.username.clone(),
            "load_exchange_config",
            None,
            None,
            format!(
                "loaded {} market configs; trading_enabled={}",
                markets.len(),
                controls.trading_enabled
            ),
        );

        Ok(LoadExchangeConfigResponse { controls, markets })
    }

    pub fn send_message(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        request: SendAdminMessageRequest,
    ) -> Result<AdminMessageEntry, AdminError> {
        let body = request.body.trim().to_string();
        if body.is_empty() {
            return Err(AdminError::MissingMessageBody);
        }

        let target = match request.target_username.as_deref() {
            Some(username) => {
                let user = state
                    .storage
                    .get_user_by_username(username.trim())
                    .ok_or(AdminError::TargetUserNotFound)?;
                (Some(user.profile.username), Some(user.profile.trader_id))
            }
            None => (None, None),
        };

        let entry = AdminMessageEntry {
            message_id: Uuid::new_v4(),
            target_username: target.0.clone(),
            target_trader_id: target.1,
            market: request.market.clone(),
            level: request.level,
            title: request.title.map(|title| title.trim().to_string()),
            body,
            created_at: Utc::now(),
        };
        state.storage.append_admin_message(entry.clone());
        publish_admin_message(state, entry.clone());
        record_admin_audit(
            state,
            admin.username.clone(),
            "send_admin_message",
            target.0,
            target.1,
            format!(
                "level={:?} market={:?} title={:?}",
                entry.level, entry.market, entry.title
            ),
        );

        Ok(entry)
    }

    pub async fn settle_market(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        market_id: &str,
        request: SettleMarketRequest,
    ) -> Result<SettleMarketResponse, AdminError> {
        if request.settlement_price == 0 {
            return Err(AdminError::InvalidSettlementPrice);
        }

        let mut market = state
            .storage
            .get_market(market_id)
            .ok_or(AdminError::MarketNotFound)?;
        if market.status == MarketStatus::Settled {
            return Err(AdminError::MarketAlreadySettled);
        }

        market.status = MarketStatus::Disabled;
        market.updated_at = Utc::now();
        state.storage.upsert_market(market.clone());

        let open_orders = state
            .storage
            .list_all_open_orders()
            .into_iter()
            .filter(|order| order.market == market.market_id)
            .collect::<Vec<_>>();
        for order in &open_orders {
            match TradingService::cancel_order(state, order.trader_id, order.id).await {
                Ok(_) => {}
                Err(TradingError::OrderNotFound) => {}
                Err(error) => return Err(AdminError::SettlementFailed(error.to_string())),
            }
        }

        let summary =
            SettlementEngine::settle_market(state, &market.market_id, request.settlement_price)?;
        market.status = MarketStatus::Settled;
        market.settlement_price = Some(request.settlement_price);
        market.updated_at = Utc::now();
        state.storage.upsert_market(market.clone());

        let announcement = request.announcement.unwrap_or_else(|| {
            format!(
                "Market {} settled at {} {} per {}.",
                market.display_name, request.settlement_price, market.quote_asset, market.base_asset
            )
        });
        let _ = Self::send_message(
            state,
            admin,
            SendAdminMessageRequest {
                target_username: None,
                market: Some(market.market_id.clone()),
                level: AdminMessageLevel::Info,
                title: Some(format!("{} settled", market.display_name)),
                body: announcement,
            },
        )?;

        record_admin_audit(
            state,
            admin.username.clone(),
            "settle_market",
            Some(market.market_id.clone()),
            None,
            format!(
                "settlement_price={} canceled_orders={} affected_traders={} settled_quantity={}",
                request.settlement_price,
                open_orders.len(),
                summary.affected_traders,
                summary.settled_quantity
            ),
        );

        Ok(SettleMarketResponse {
            market,
            canceled_orders: open_orders.len(),
            affected_traders: summary.affected_traders,
            settled_quantity: summary.settled_quantity,
            settlement_price: request.settlement_price,
        })
    }

    pub fn reset_all_users(
        state: &AppState,
        admin: &AuthenticatedAdmin,
    ) -> ResetUsersResponse {
        let open_orders = state.storage.list_all_open_orders();
        let cleared_orders = open_orders.len();
        let cleared_positions = state
            .storage
            .list_all_positions()
            .into_iter()
            .map(|(_, positions)| positions.len())
            .sum();
        let cleared_fills = state
            .storage
            .list_users()
            .into_iter()
            .map(|user| state.storage.list_fills(user.profile.trader_id, None).len())
            .sum();

        for order in &open_orders {
            publish_user_event(
                state,
                order.trader_id,
                ServerMessage::OrderState {
                    order: order.clone(),
                    status: OrderStateStatus::Canceled,
                },
            );
            publish_market_delta(
                state,
                &order.market,
                BookDelta::OrderRemoved {
                    order_id: order.id,
                    side: order.side,
                    price: order.price,
                },
            );
        }

        state.storage.reset_all_trading_state();
        state.orderbooks.clear();
        let _ = state.system_events_tx.send(ServerMessage::ResyncRequired {
            channel: "account".to_string(),
            market: None,
            expected_sequence: None,
            current_sequence: None,
            reason: "admin reset all users".to_string(),
        });

        record_admin_audit(
            state,
            admin.username.clone(),
            "reset_all_users",
            None,
            None,
            format!(
                "cleared_orders={} cleared_positions={} cleared_fills={}",
                cleared_orders, cleared_positions, cleared_fills
            ),
        );

        ResetUsersResponse {
            cleared_orders,
            cleared_positions,
            cleared_fills,
        }
    }

    pub async fn leaderboard(state: &AppState, limit: Option<usize>) -> Vec<LeaderboardRow> {
        let markets = state.storage.list_markets();
        let mut market_marks = std::collections::BTreeMap::new();
        for market in &markets {
            market_marks.insert(market.market_id.clone(), market_mark_price(state, market).await);
        }

        let mut rows = state
            .storage
            .list_users()
            .into_iter()
            .map(|user| {
                let mut realized_pnl = 0_i64;
                let mut unrealized_pnl = 0_i64;
                let mut gross_exposure = 0_u64;
                for position in state.storage.list_positions(user.profile.trader_id) {
                    realized_pnl = realized_pnl.saturating_add(position.realized_pnl);
                    let mark = market_marks.get(&position.market).copied().unwrap_or(0);
                    gross_exposure = gross_exposure
                        .saturating_add(position.net_quantity.unsigned_abs().saturating_mul(mark));
                    if position.net_quantity != 0 {
                        if let Some(average_entry_price) = position.average_entry_price {
                            let mark_i64 = i64::try_from(mark).unwrap_or(i64::MAX);
                            let average_i64 =
                                i64::try_from(average_entry_price).unwrap_or(i64::MAX);
                            let delta = mark_i64.saturating_sub(average_i64);
                            unrealized_pnl = unrealized_pnl.saturating_add(
                                delta.saturating_mul(position.net_quantity),
                            );
                        }
                    }
                }
                let net_pnl = realized_pnl.saturating_add(unrealized_pnl);
                LeaderboardRow {
                    rank: 0,
                    trader_id: user.profile.trader_id,
                    username: user.profile.username,
                    net_pnl,
                    realized_pnl,
                    unrealized_pnl,
                    gross_exposure,
                }
            })
            .collect::<Vec<_>>();

        rows.sort_by(|left, right| {
            right
                .net_pnl
                .cmp(&left.net_pnl)
                .then_with(|| left.username.cmp(&right.username))
                .then_with(|| left.trader_id.cmp(&right.trader_id))
        });
        for (index, row) in rows.iter_mut().enumerate() {
            row.rank = index + 1;
        }
        if let Some(limit) = limit {
            rows.truncate(limit);
        }
        rows
    }
}

fn build_market_definition(
    existing: Option<&MarketDefinition>,
    request: UpsertMarketRequest,
) -> Result<MarketDefinition, AdminError> {
    let market_id = request.market_id.trim().to_string();
    if market_id.is_empty() {
        return Err(AdminError::MissingMarketId);
    }
    let base_asset = request.base_asset.trim().to_string();
    if base_asset.is_empty() {
        return Err(AdminError::MissingBaseAsset);
    }
    let quote_asset = request.quote_asset.trim().to_string();
    if quote_asset.is_empty() {
        return Err(AdminError::MissingQuoteAsset);
    }
    let expected_market_id = format!("{base_asset}-{quote_asset}");
    if market_id != expected_market_id {
        return Err(AdminError::MarketIdMismatch {
            expected: expected_market_id,
        });
    }
    if request.tick_size == 0 {
        return Err(AdminError::InvalidTickSize);
    }
    if request.min_order_quantity == 0 {
        return Err(AdminError::InvalidMinimumOrderQuantity);
    }

    let now = Utc::now();
    Ok(MarketDefinition {
        market_id: market_id.clone(),
        display_name: request
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&market_id)
            .to_string(),
        base_asset,
        quote_asset,
        tick_size: request.tick_size,
        min_order_quantity: request.min_order_quantity,
        reference_price: request.reference_price,
        settlement_price: existing.and_then(|market| market.settlement_price),
        status: if request.enabled {
            MarketStatus::Enabled
        } else {
            MarketStatus::Disabled
        },
        created_at: existing.map(|market| market.created_at).unwrap_or(now),
        updated_at: now,
    })
}

async fn market_mark_price(state: &AppState, market: &MarketDefinition) -> u64 {
    if let Some(settlement_price) = market.settlement_price {
        return settlement_price;
    }
    if let Some(book) = state.orderbooks.get(&market.market_id) {
        let book = book.lock().await;
        return match (book.best_bid_price(), book.best_ask_price()) {
            (Some(bid), Some(ask)) => bid.saturating_add(ask) / 2,
            (Some(bid), None) => bid,
            (None, Some(ask)) => ask,
            (None, None) => market.reference_price.unwrap_or(0),
        };
    }
    market.reference_price.unwrap_or(0)
}

fn publish_admin_message(state: &AppState, entry: AdminMessageEntry) {
    let message = ServerMessage::AdminMessage {
        message: entry.clone(),
    };
    if let Some(trader_id) = entry.target_trader_id {
        let _ = state
            .user_events_tx
            .send(UserBroadcastEvent { trader_id, message });
        return;
    }
    let _ = state.system_events_tx.send(message);
}

fn publish_market_delta(state: &AppState, market: &str, event: BookDelta) {
    let _ = state.events_tx.send(BroadcastEvent {
        market: market.to_string(),
        sequence: state.next_market_sequence(market),
        event,
    });
}

fn publish_user_event(state: &AppState, trader_id: Uuid, message: ServerMessage) {
    let _ = state
        .user_events_tx
        .send(UserBroadcastEvent { trader_id, message });
}

fn record_admin_audit(
    state: &AppState,
    actor_username: String,
    action: &str,
    target_username: Option<String>,
    target_trader_id: Option<Uuid>,
    details: impl Into<String>,
) {
    let details = details.into();
    let entry = AdminAuditEntry {
        audit_id: Uuid::new_v4(),
        actor_username: actor_username.clone(),
        action: action.to_string(),
        target_username: target_username.clone(),
        target_trader_id,
        details: details.clone(),
        occurred_at: Utc::now(),
    };

    info!(
        actor_username,
        action,
        target_username = ?target_username,
        target_trader_id = ?entry.target_trader_id,
        details,
        "admin audit event"
    );
    state.storage.append_admin_audit_log(entry);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{UserProfile, UserRecord};
    use crate::config::Config;
    use crate::settlement::SettlementEngine;

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

    fn admin() -> AuthenticatedAdmin {
        AuthenticatedAdmin {
            username: "ops".to_string(),
        }
    }

    #[test]
    fn upsert_market_validates_market_id_shape() {
        let state = test_state();
        let error = AdminService::upsert_market(
            &state,
            &admin(),
            UpsertMarketRequest {
                market_id: "BTCUSD".to_string(),
                display_name: None,
                base_asset: "BTC".to_string(),
                quote_asset: "USD".to_string(),
                tick_size: 1,
                min_order_quantity: 1,
                reference_price: None,
                enabled: true,
            },
        )
        .expect_err("market should be rejected");

        assert!(matches!(error, AdminError::MarketIdMismatch { .. }));
    }

    #[tokio::test]
    async fn leaderboard_marks_positions_using_market_reference_prices() {
        let state = test_state();
        let trader_a = UserRecord {
            profile: UserProfile {
                trader_id: Uuid::new_v4(),
                username: "alice".to_string(),
                api_key: "exch_alice".to_string(),
                created_at: Utc::now(),
            },
        };
        let trader_b = UserRecord {
            profile: UserProfile {
                trader_id: Uuid::new_v4(),
                username: "bob".to_string(),
                api_key: "exch_bob".to_string(),
                created_at: Utc::now(),
            },
        };
        state.storage.create_user(trader_a.clone()).expect("alice");
        state.storage.create_user(trader_b.clone()).expect("bob");
        AdminService::upsert_market(
            &state,
            &admin(),
            UpsertMarketRequest {
                market_id: "BTC-USD".to_string(),
                display_name: None,
                base_asset: "BTC".to_string(),
                quote_asset: "USD".to_string(),
                tick_size: 1,
                min_order_quantity: 1,
                reference_price: Some(100),
                enabled: true,
            },
        )
        .expect("market");
        SettlementEngine::seed_position(
            &state,
            trader_a.profile.trader_id,
            "BTC-USD",
            2,
            Some(80),
            0,
        );
        SettlementEngine::seed_position(
            &state,
            trader_b.profile.trader_id,
            "BTC-USD",
            1,
            Some(75),
            0,
        );

        let leaderboard = AdminService::leaderboard(&state, None).await;

        assert_eq!(leaderboard.len(), 2);
        assert_eq!(leaderboard[0].username, "alice");
        assert_eq!(leaderboard[0].net_pnl, 40);
        assert_eq!(leaderboard[1].username, "bob");
        assert_eq!(leaderboard[1].net_pnl, 25);
    }
}
