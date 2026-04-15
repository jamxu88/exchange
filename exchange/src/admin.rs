use crate::accounts::UserRole;
use crate::auth::AuthenticatedAdmin;
use crate::bots::{
    AdminBotState, AdminDeskError, AdminDeskOrderRequest, AdminDeskOrderResponse, AdminDeskSummary,
    BotControlError, UpsertAdminBotRequest, admin_desk_summary, ensure_admin_desk,
    submit_admin_desk_order,
};
use crate::marketdata::{DATA_STREAM_CHANNEL, OrderStateStatus, ServerMessage};
use crate::settlement::{SettlementEngine, SettlementError};
use crate::state::{AccountBarrierStatus, AppState, DispatchQueueMode, DispatchQueueStatus};
use crate::storage::PersistenceStatus;
use crate::telemetry::OperatorTelemetrySnapshot;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;
use tracing::info;
use utoipa::ToSchema;
use uuid::Uuid;

const DEFAULT_COMPETITION_QUOTE_ASSET: &str = "USD";
const DEFAULT_COMPETITION_MARKET_SUFFIX: &str = "MARKET";

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
    #[serde(default)]
    pub min_price: Option<u64>,
    #[serde(default)]
    pub max_price: Option<u64>,
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
    #[serde(default)]
    pub market_id: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub base_asset: String,
    #[serde(default)]
    pub quote_asset: String,
    pub tick_size: u64,
    pub min_order_quantity: u64,
    #[serde(default, rename = "min", alias = "min_price")]
    pub min_price: Option<u64>,
    #[serde(default, rename = "max", alias = "max_price")]
    pub max_price: Option<u64>,
    pub reference_price: Option<u64>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateMarketRequest {
    pub display_name: Option<String>,
    pub tick_size: Option<u64>,
    pub min_order_quantity: Option<u64>,
    #[serde(default, rename = "min", alias = "min_price")]
    pub min_price: Option<u64>,
    #[serde(default, rename = "max", alias = "max_price")]
    pub max_price: Option<u64>,
    pub reference_price: Option<u64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoadExchangeConfigRequest {
    pub trading_enabled: Option<bool>,
    pub markets: Vec<UpsertMarketRequest>,
    #[serde(default)]
    pub bots: Vec<UpsertAdminBotRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoadExchangeConfigResponse {
    pub controls: ExchangeControls,
    pub markets: Vec<MarketDefinition>,
    pub bots: Vec<AdminBotState>,
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct SettleMarketResponse {
    pub market: MarketDefinition,
    pub canceled_orders: usize,
    pub affected_traders: usize,
    pub settled_quantity: u64,
    pub settlement_price: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct LeaderboardRow {
    pub rank: usize,
    pub trader_id: Uuid,
    pub team_number: String,
    pub net_pnl: i64,
    pub realized_pnl: i64,
    pub unrealized_pnl: i64,
    pub gross_exposure: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct CompetitionSettlementRequest {
    pub market_id: String,
    pub settlement_price: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct FinalizeCompetitionRequest {
    #[serde(default = "default_competition_id")]
    pub competition_id: String,
    pub label: Option<String>,
    #[serde(default)]
    pub settlements: Vec<CompetitionSettlementRequest>,
    #[serde(default)]
    pub eligible_usernames: Vec<String>,
    #[serde(default)]
    pub eligible_trader_ids: Vec<Uuid>,
    #[serde(default)]
    pub include_all_traders: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct CompetitionLeaderboardSnapshot {
    pub snapshot_id: Uuid,
    pub competition_id: String,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub entrants: usize,
    pub leaderboard: Vec<LeaderboardRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct FinalizeCompetitionResponse {
    pub controls: ExchangeControls,
    pub settled_markets: Vec<SettleMarketResponse>,
    pub snapshot: CompetitionLeaderboardSnapshot,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct ProvisionedUsersQuery {
    pub username_prefix: Option<String>,
    pub role: Option<UserRole>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct ProvisionedUserCredential {
    pub trader_id: Uuid,
    pub username: String,
    pub api_key: String,
    pub role: UserRole,
    pub position_limit: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct ProvisionedUsersResponse {
    pub users: Vec<ProvisionedUserCredential>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct CompetitionSnapshotQuery {
    #[serde(default = "default_competition_id")]
    pub competition_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminStateResponse {
    pub controls: ExchangeControls,
    pub markets: Vec<MarketDefinition>,
    pub bots: Vec<AdminBotState>,
    pub admin_desk: Option<AdminDeskSummary>,
    pub recent_messages: Vec<AdminMessageEntry>,
    pub persistence: PersistenceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminTelemetryResponse {
    pub status: String,
    pub service: String,
    pub now: String,
    pub persistence: PersistenceStatus,
    pub runtime_dispatch: DispatchQueueStatus,
    pub account_dispatch: DispatchQueueStatus,
    pub persistence_dispatch: DispatchQueueStatus,
    pub account_barrier: AccountBarrierStatus,
    pub traffic: OperatorTelemetrySnapshot,
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
    #[error("market id must include a non-empty suffix")]
    InvalidMarketId,
    #[error("display name, base asset, or market id is required")]
    MissingMarketLabel,
    #[error("tick size must be greater than zero")]
    InvalidTickSize,
    #[error("minimum order quantity must be greater than zero")]
    InvalidMinimumOrderQuantity,
    #[error("minimum allowable price must be greater than zero")]
    InvalidMinimumAllowedPrice,
    #[error("maximum allowable price must be greater than zero")]
    InvalidMaximumAllowedPrice,
    #[error("maximum allowable price must be greater than or equal to minimum allowable price")]
    InvalidMaximumAllowedPriceRange,
    #[error("allowable market price bounds must align to tick size {tick_size}")]
    PriceBoundsTickSizeViolation { tick_size: u64 },
    #[error("market not found")]
    MarketNotFound,
    #[error("market already settled")]
    MarketAlreadySettled,
    #[error("competition settlement list is required")]
    MissingCompetitionSettlements,
    #[error("competition entrant filter is required unless include_all_traders is true")]
    MissingCompetitionEntrants,
    #[error("duplicate settlement entry for market {market_id}")]
    DuplicateCompetitionSettlementMarket { market_id: String },
    #[error("competition user not found: {identifier}")]
    CompetitionUserNotFound { identifier: String },
    #[error("user {username} is not eligible for competition standings")]
    CompetitionUserIneligible { username: String },
    #[error("competition snapshot not found")]
    CompetitionSnapshotNotFound,
    #[error("cannot delete market with open orders")]
    MarketHasOpenOrders,
    #[error("message body is required")]
    MissingMessageBody,
    #[error("target user not found")]
    TargetUserNotFound,
    #[error("settlement price must be zero or greater")]
    InvalidSettlementPrice,
    #[error("numeric overflow")]
    Overflow,
    #[error("{message}")]
    BotControl { message: String, status: u16 },
    #[error("{0}")]
    SettlementFailed(String),
}

impl AdminError {
    pub fn status_code(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;

        match self {
            Self::MarketNotFound
            | Self::TargetUserNotFound
            | Self::CompetitionUserNotFound { .. }
            | Self::CompetitionSnapshotNotFound => StatusCode::NOT_FOUND,
            Self::BotControl { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
            }
            Self::MarketAlreadySettled | Self::MarketHasOpenOrders | Self::SettlementFailed(_) => {
                StatusCode::CONFLICT
            }
            Self::Overflow => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MissingMarketId
            | Self::MissingCompetitionSettlements
            | Self::MissingCompetitionEntrants
            | Self::DuplicateCompetitionSettlementMarket { .. }
            | Self::CompetitionUserIneligible { .. }
            | Self::InvalidMarketId
            | Self::MissingMarketLabel
            | Self::InvalidTickSize
            | Self::InvalidMinimumOrderQuantity
            | Self::InvalidMinimumAllowedPrice
            | Self::InvalidMaximumAllowedPrice
            | Self::InvalidMaximumAllowedPriceRange
            | Self::PriceBoundsTickSizeViolation { .. }
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

impl From<BotControlError> for AdminError {
    fn from(value: BotControlError) -> Self {
        Self::BotControl {
            message: value.to_string(),
            status: value.status_code().as_u16(),
        }
    }
}

pub struct AdminService;

impl AdminService {
    pub fn get_state(state: &AppState, message_limit: usize) -> AdminStateResponse {
        AdminStateResponse {
            controls: state.storage.get_exchange_controls(),
            markets: state.storage.list_markets(),
            bots: state.bot_manager.list(),
            admin_desk: admin_desk_summary(state),
            recent_messages: state.storage.list_admin_messages(Some(message_limit)),
            persistence: state.persistence_status(),
        }
    }

    pub fn get_telemetry(state: &AppState) -> AdminTelemetryResponse {
        let persistence = state.persistence_status();
        let runtime_dispatch = state.runtime_dispatch_status();
        let account_dispatch = state.account_dispatch_status();
        let persistence_dispatch = state.persistence_dispatch_status();
        let account_barrier = state.account_barrier_status();
        let status = if matches!(
            persistence.mode,
            crate::storage::PersistenceMode::Retrying
                | crate::storage::PersistenceMode::Backpressured
                | crate::storage::PersistenceMode::Stopped
        ) || matches!(
            runtime_dispatch.mode,
            DispatchQueueMode::Backpressured | DispatchQueueMode::Stopped
        ) || matches!(
            account_dispatch.mode,
            DispatchQueueMode::Backpressured | DispatchQueueMode::Stopped
        ) || matches!(
            persistence_dispatch.mode,
            DispatchQueueMode::Backpressured | DispatchQueueMode::Stopped
        ) {
            "degraded"
        } else {
            "ok"
        };

        AdminTelemetryResponse {
            status: status.to_string(),
            service: "exchange".to_string(),
            now: Utc::now().to_rfc3339(),
            persistence,
            runtime_dispatch,
            account_dispatch,
            persistence_dispatch,
            account_barrier,
            traffic: state.operator_telemetry_snapshot(),
        }
    }

    pub async fn upsert_bot(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        request: UpsertAdminBotRequest,
    ) -> Result<AdminBotState, BotControlError> {
        state.bot_manager.upsert(state, admin, request).await
    }

    pub async fn start_bot(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        bot_id: &str,
    ) -> Result<AdminBotState, BotControlError> {
        state.bot_manager.start(state.clone(), admin, bot_id).await
    }

    pub async fn start_all_bots(
        state: &AppState,
        admin: &AuthenticatedAdmin,
    ) -> Result<Vec<AdminBotState>, BotControlError> {
        state.bot_manager.start_all(state.clone(), admin).await
    }

    pub async fn pause_bot(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        bot_id: &str,
    ) -> Result<AdminBotState, BotControlError> {
        state.bot_manager.pause(state, admin, bot_id).await
    }

    pub async fn pause_all_bots(
        state: &AppState,
        admin: &AuthenticatedAdmin,
    ) -> Result<Vec<AdminBotState>, BotControlError> {
        state.bot_manager.pause_all(state, admin).await
    }

    pub async fn delete_bot(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        bot_id: &str,
    ) -> Result<AdminBotState, BotControlError> {
        state.bot_manager.delete(state, admin, bot_id).await
    }

    pub async fn delete_all_bots(
        state: &AppState,
        admin: &AuthenticatedAdmin,
    ) -> Result<Vec<AdminBotState>, BotControlError> {
        state.bot_manager.delete_all(state, admin).await
    }

    pub fn ensure_admin_desk(
        state: &AppState,
        admin: &AuthenticatedAdmin,
    ) -> Result<AdminDeskSummary, crate::auth::AuthError> {
        ensure_admin_desk(state, admin)
    }

    pub async fn submit_admin_desk_order(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        request: AdminDeskOrderRequest,
    ) -> Result<AdminDeskOrderResponse, AdminDeskError> {
        submit_admin_desk_order(state, admin, request).await
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
        state.request_checkpoint_save();

        TradingControlResponse { controls }
    }

    pub fn list_markets(state: &AppState) -> Vec<MarketDefinition> {
        state.storage.list_markets()
    }

    pub fn list_admin_messages(state: &AppState, limit: usize) -> Vec<AdminMessageEntry> {
        state.storage.list_admin_messages(Some(limit))
    }

    pub fn list_provisioned_users(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        query: ProvisionedUsersQuery,
    ) -> ProvisionedUsersResponse {
        let response = ProvisionedUsersResponse {
            users: provisioned_users_for_query(state, &query),
        };
        record_admin_audit(
            state,
            admin.username.clone(),
            "list_provisioned_users",
            None,
            None,
            format!(
                "count={} username_prefix={:?} role={:?} limit={:?}",
                response.users.len(),
                query.username_prefix.as_deref().map(str::trim),
                query.role,
                query.limit
            ),
        );
        response
    }

    pub async fn finalize_competition(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        request: FinalizeCompetitionRequest,
    ) -> Result<FinalizeCompetitionResponse, AdminError> {
        let competition_id = normalized_competition_id(&request.competition_id);
        let label = normalized_competition_label(request.label.as_deref(), &competition_id);
        let entrant_users = resolve_competition_users(
            state,
            &request.eligible_usernames,
            &request.eligible_trader_ids,
            request.include_all_traders,
        )?;
        let settlements = validate_competition_settlements(state, &request.settlements)?;

        let controls = Self::set_trading_enabled(state, admin, false).controls;
        let mut settled_markets = Vec::with_capacity(settlements.len());
        for settlement in settlements {
            settled_markets.push(
                Self::settle_market(
                    state,
                    admin,
                    &settlement.market_id,
                    SettleMarketRequest {
                        settlement_price: settlement.settlement_price,
                        announcement: None,
                    },
                )
                .await?,
            );
        }

        let snapshot = CompetitionLeaderboardSnapshot {
            snapshot_id: Uuid::new_v4(),
            competition_id,
            label,
            created_at: Utc::now(),
            entrants: entrant_users.len(),
            leaderboard: build_leaderboard_rows(state, entrant_users, None).await,
        };
        state.storage.append_competition_snapshot(snapshot.clone());

        record_admin_audit(
            state,
            admin.username.clone(),
            "finalize_competition",
            Some(snapshot.competition_id.clone()),
            None,
            format!(
                "snapshot_id={} entrants={} settled_markets={}",
                snapshot.snapshot_id,
                snapshot.entrants,
                settled_markets.len()
            ),
        );
        state.request_checkpoint_save();

        Ok(FinalizeCompetitionResponse {
            controls,
            settled_markets,
            snapshot,
        })
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
        publish_market_state(state, market.clone());
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
                "market {} status={:?} tick_size={} min_order_quantity={} min_price={:?} max_price={:?}",
                market.market_id,
                market.status,
                market.tick_size,
                market.min_order_quantity,
                market.min_price,
                market.max_price
            ),
        );
        state.request_checkpoint_save();

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
        if let Some(min_price) = request.min_price {
            market.min_price = Some(min_price);
        }
        if let Some(max_price) = request.max_price {
            market.max_price = Some(max_price);
        }
        validate_market_price_bounds(market.min_price, market.max_price, market.tick_size)?;
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
        publish_market_state(state, market.clone());
        record_admin_audit(
            state,
            admin.username.clone(),
            "patch_market",
            Some(market.market_id.clone()),
            None,
            format!(
                "market {} status={:?} tick_size={} min_order_quantity={} min_price={:?} max_price={:?}",
                market.market_id,
                market.status,
                market.tick_size,
                market.min_order_quantity,
                market.min_price,
                market.max_price
            ),
        );
        state.request_checkpoint_save();

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
        state.remove_market_runtime(trimmed_market);
        state.sync_market_data_snapshot_state();
        publish_market_deleted(state, trimmed_market);
        record_admin_audit(
            state,
            admin.username.clone(),
            "delete_market",
            Some(trimmed_market.to_string()),
            None,
            format!("deleted market {trimmed_market}"),
        );
        state.request_checkpoint_save();
        Ok(DeleteMarketResponse {
            market_id: trimmed_market.to_string(),
        })
    }

    pub async fn load_exchange_config(
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

        let mut bots = Vec::with_capacity(request.bots.len());
        for bot in request.bots {
            bots.push(
                state
                    .bot_manager
                    .upsert(state, admin, bot)
                    .await
                    .map_err(AdminError::from)?,
            );
        }

        if bots.is_empty() {
            bots = state.bot_manager.list();
        }

        record_admin_audit(
            state,
            admin.username.clone(),
            "load_exchange_config",
            None,
            None,
            format!(
                "loaded {} market configs and {} bot configs; trading_enabled={}",
                markets.len(),
                bots.len(),
                controls.trading_enabled
            ),
        );
        state.request_checkpoint_save();

        Ok(LoadExchangeConfigResponse {
            controls,
            markets,
            bots,
        })
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
        publish_market_state(state, market.clone());

        let canceled_orders = state
            .storage
            .close_open_orders_for_market(&market.market_id);
        state.remove_market_runtime(&market.market_id);
        state.sync_market_data_snapshot_state();
        publish_market_resync(
            state,
            &market.market_id,
            "market settled; resubscribe for a fresh snapshot",
        );
        publish_user_resync(
            state,
            "market settlement cleared resting orders; refresh account state and reconnect if needed",
        );

        let summary =
            SettlementEngine::settle_market(state, &market.market_id, request.settlement_price)?;
        market.status = MarketStatus::Settled;
        market.settlement_price = Some(request.settlement_price);
        market.updated_at = Utc::now();
        state.storage.upsert_market(market.clone());
        publish_market_state(state, market.clone());

        let announcement = request.announcement.unwrap_or_else(|| {
            if market.quote_asset == DEFAULT_COMPETITION_QUOTE_ASSET {
                format!(
                    "Market {} settled at ${} per {}.",
                    market.display_name, request.settlement_price, market.base_asset
                )
            } else {
                format!(
                    "Market {} settled at {} {} per {}.",
                    market.display_name,
                    request.settlement_price,
                    market.quote_asset,
                    market.base_asset
                )
            }
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
                canceled_orders,
                summary.affected_traders,
                summary.settled_quantity
            ),
        );
        state.request_checkpoint_save();

        Ok(SettleMarketResponse {
            market,
            canceled_orders,
            affected_traders: summary.affected_traders,
            settled_quantity: summary.settled_quantity,
            settlement_price: request.settlement_price,
        })
    }

    pub fn reset_all_users(state: &AppState, admin: &AuthenticatedAdmin) -> ResetUsersResponse {
        let open_orders = state.storage.list_all_open_orders();
        let market_ids = state
            .storage
            .list_markets()
            .into_iter()
            .map(|market| market.market_id)
            .collect::<Vec<_>>();
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
        }

        state.storage.reset_all_trading_state();
        state.clear_market_runtime();
        state.sync_market_data_snapshot_state();

        for market_id in &market_ids {
            publish_market_resync(
                state,
                market_id,
                "admin reset all users cleared resting orders; resubscribe for a fresh snapshot",
            );
        }
        publish_user_resync(
            state,
            "admin reset all users cleared account state; refresh account state and reconnect if needed",
        );

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
        state.request_checkpoint_save();

        ResetUsersResponse {
            cleared_orders,
            cleared_positions,
            cleared_fills,
        }
    }

    pub async fn leaderboard(state: &AppState, limit: Option<usize>) -> Vec<LeaderboardRow> {
        build_leaderboard_rows(state, state.storage.list_users(), limit).await
    }

    pub fn get_competition_snapshot(
        state: &AppState,
        snapshot_id: Uuid,
    ) -> Result<CompetitionLeaderboardSnapshot, AdminError> {
        state
            .storage
            .get_competition_snapshot(snapshot_id)
            .ok_or(AdminError::CompetitionSnapshotNotFound)
    }

    pub fn latest_competition_snapshot(
        state: &AppState,
        competition_id: &str,
    ) -> Result<CompetitionLeaderboardSnapshot, AdminError> {
        state
            .storage
            .latest_competition_snapshot(&normalized_competition_id(competition_id))
            .ok_or(AdminError::CompetitionSnapshotNotFound)
    }

    pub fn export_competition_snapshot_csv(snapshot: &CompetitionLeaderboardSnapshot) -> String {
        let mut csv = String::from(
            "rank,trader_id,team_number,net_pnl,realized_pnl,unrealized_pnl,gross_exposure\n",
        );
        for row in &snapshot.leaderboard {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                row.rank,
                row.trader_id,
                csv_escape(&row.team_number),
                row.net_pnl,
                row.realized_pnl,
                row.unrealized_pnl,
                row.gross_exposure
            ));
        }
        csv
    }

    pub fn export_provisioned_users_csv(
        state: &AppState,
        admin: &AuthenticatedAdmin,
        query: ProvisionedUsersQuery,
    ) -> String {
        let users = provisioned_users_for_query(state, &query);
        record_admin_audit(
            state,
            admin.username.clone(),
            "export_provisioned_users_csv",
            None,
            None,
            format!(
                "count={} username_prefix={:?} role={:?} limit={:?}",
                users.len(),
                query.username_prefix.as_deref().map(str::trim),
                query.role,
                query.limit
            ),
        );

        let mut csv = String::from("trader_id,username,api_key,role,position_limit,created_at\n");
        for user in users {
            csv.push_str(&format!(
                "{},{},{},{},{},{}\n",
                user.trader_id,
                csv_escape(&user.username),
                csv_escape(&user.api_key),
                user_role_slug(user.role),
                user.position_limit
                    .map(|limit| limit.to_string())
                    .unwrap_or_else(|| "unlimited".to_string()),
                user.created_at.to_rfc3339()
            ));
        }
        csv
    }
}

fn default_competition_id() -> String {
    "default".to_string()
}

fn normalized_competition_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        default_competition_id()
    } else {
        trimmed.to_string()
    }
}

fn normalized_competition_label(raw: Option<&str>, competition_id: &str) -> String {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(label) => label.to_string(),
        None => format!("{competition_id} final standings"),
    }
}

fn validate_competition_settlements(
    state: &AppState,
    settlements: &[CompetitionSettlementRequest],
) -> Result<Vec<CompetitionSettlementRequest>, AdminError> {
    if settlements.is_empty() {
        return Err(AdminError::MissingCompetitionSettlements);
    }

    let mut seen = BTreeSet::new();
    let mut validated = Vec::with_capacity(settlements.len());
    for settlement in settlements {
        let market_id = settlement.market_id.trim().to_ascii_uppercase();
        if !seen.insert(market_id.clone()) {
            return Err(AdminError::DuplicateCompetitionSettlementMarket { market_id });
        }
        let market = state
            .storage
            .get_market(&market_id)
            .ok_or(AdminError::MarketNotFound)?;
        if market.status == MarketStatus::Settled {
            return Err(AdminError::MarketAlreadySettled);
        }
        validated.push(CompetitionSettlementRequest {
            market_id,
            settlement_price: settlement.settlement_price,
        });
    }

    Ok(validated)
}

fn resolve_competition_users(
    state: &AppState,
    usernames: &[String],
    trader_ids: &[Uuid],
    include_all_traders: bool,
) -> Result<Vec<crate::accounts::UserRecord>, AdminError> {
    if usernames.is_empty() && trader_ids.is_empty() && !include_all_traders {
        return Err(AdminError::MissingCompetitionEntrants);
    }

    let mut trader_id_set = BTreeSet::new();
    if include_all_traders {
        for user in state.storage.list_users() {
            if user.profile.role == crate::accounts::UserRole::Trader {
                trader_id_set.insert(user.profile.trader_id);
            }
        }
    }

    for username in usernames {
        let normalized = username.trim();
        let user = state
            .storage
            .get_user_by_username(normalized)
            .ok_or_else(|| AdminError::CompetitionUserNotFound {
                identifier: normalized.to_string(),
            })?;
        if user.profile.role != crate::accounts::UserRole::Trader {
            return Err(AdminError::CompetitionUserIneligible {
                username: user.profile.username,
            });
        }
        trader_id_set.insert(user.profile.trader_id);
    }

    for trader_id in trader_ids {
        let user = state.storage.get_user(*trader_id).ok_or_else(|| {
            AdminError::CompetitionUserNotFound {
                identifier: trader_id.to_string(),
            }
        })?;
        if user.profile.role != crate::accounts::UserRole::Trader {
            return Err(AdminError::CompetitionUserIneligible {
                username: user.profile.username,
            });
        }
        trader_id_set.insert(*trader_id);
    }

    if trader_id_set.is_empty() {
        return Err(AdminError::MissingCompetitionEntrants);
    }

    let mut users = trader_id_set
        .into_iter()
        .filter_map(|trader_id| state.storage.get_user(trader_id))
        .collect::<Vec<_>>();
    users.sort_by(|left, right| {
        left.profile
            .username
            .cmp(&right.profile.username)
            .then_with(|| left.profile.trader_id.cmp(&right.profile.trader_id))
    });
    Ok(users)
}

async fn build_leaderboard_rows(
    state: &AppState,
    users: Vec<crate::accounts::UserRecord>,
    limit: Option<usize>,
) -> Vec<LeaderboardRow> {
    let markets = state.storage.list_markets();
    let mut market_marks = std::collections::BTreeMap::new();
    for market in &markets {
        market_marks.insert(
            market.market_id.clone(),
            market_mark_price(state, market).await,
        );
    }

    let mut rows = users
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
                        let average_i64 = i64::try_from(average_entry_price).unwrap_or(i64::MAX);
                        let delta = mark_i64.saturating_sub(average_i64);
                        unrealized_pnl = unrealized_pnl
                            .saturating_add(delta.saturating_mul(position.net_quantity));
                    }
                }
            }
            let net_pnl = realized_pnl.saturating_add(unrealized_pnl);
            LeaderboardRow {
                rank: 0,
                trader_id: user.profile.trader_id,
                team_number: user.profile.public_team_number().to_string(),
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
            .then_with(|| left.team_number.cmp(&right.team_number))
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

fn provisioned_users_for_query(
    state: &AppState,
    query: &ProvisionedUsersQuery,
) -> Vec<ProvisionedUserCredential> {
    let username_prefix = query
        .username_prefix
        .as_deref()
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty());
    let mut users = state
        .storage
        .list_users()
        .into_iter()
        .filter(|user| {
            query
                .role
                .map(|role| user.profile.role == role)
                .unwrap_or(true)
        })
        .filter(|user| {
            username_prefix
                .map(|prefix| user.profile.username.starts_with(prefix))
                .unwrap_or(true)
        })
        .map(|user| ProvisionedUserCredential {
            trader_id: user.profile.trader_id,
            username: user.profile.username,
            api_key: user.profile.api_key,
            role: user.profile.role,
            position_limit: SettlementEngine::position_limit_for_role(user.profile.role),
            created_at: user.profile.created_at,
        })
        .collect::<Vec<_>>();
    if let Some(limit) = query.limit {
        users.truncate(limit);
    }
    users
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn user_role_slug(role: UserRole) -> &'static str {
    match role {
        UserRole::Trader => "trader",
        UserRole::Admin => "admin",
    }
}

fn normalize_market_stem(value: &str) -> String {
    value
        .trim()
        .to_ascii_uppercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn strip_market_suffix(value: &str) -> String {
    let suffix = format!("-{DEFAULT_COMPETITION_MARKET_SUFFIX}");
    value.strip_suffix(&suffix).unwrap_or(value).to_string()
}

fn derive_market_id_from_label(value: &str) -> String {
    let stem = strip_market_suffix(&normalize_market_stem(value));
    if stem.is_empty() {
        return String::new();
    }

    format!("{stem}-{DEFAULT_COMPETITION_MARKET_SUFFIX}")
}

fn normalize_market_id(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let Some((stem, suffix)) = trimmed.rsplit_once('-') else {
        return trimmed.to_ascii_uppercase();
    };

    let normalized_stem = normalize_market_stem(stem);
    let normalized_suffix = normalize_market_stem(suffix);
    if normalized_stem.is_empty() || normalized_suffix.is_empty() {
        return String::new();
    }

    format!("{normalized_stem}-{normalized_suffix}")
}

fn validate_market_id(market_id: &str) -> Result<(), AdminError> {
    let Some((stem, suffix)) = market_id.rsplit_once('-') else {
        return Err(AdminError::InvalidMarketId);
    };
    if stem.is_empty() || suffix.is_empty() {
        return Err(AdminError::InvalidMarketId);
    }

    Ok(())
}

fn derive_base_asset(
    requested_base_asset: &str,
    display_name: Option<&str>,
    market_id: &str,
) -> String {
    let normalized_base_asset = normalize_market_stem(requested_base_asset);
    if !normalized_base_asset.is_empty() {
        return normalized_base_asset;
    }

    let derived_display_name = display_name
        .map(derive_market_id_from_label)
        .unwrap_or_default();
    if let Some((stem, _)) = derived_display_name.rsplit_once('-') {
        if !stem.is_empty() {
            return stem.to_string();
        }
    }

    market_id
        .rsplit_once('-')
        .map(|(stem, _)| stem.to_string())
        .unwrap_or_default()
}

fn build_market_definition(
    existing: Option<&MarketDefinition>,
    request: UpsertMarketRequest,
) -> Result<MarketDefinition, AdminError> {
    let requested_market_id = normalize_market_id(&request.market_id);
    let display_name = request
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let quote_asset = {
        let normalized = request.quote_asset.trim().to_ascii_uppercase();
        if normalized.is_empty() {
            DEFAULT_COMPETITION_QUOTE_ASSET.to_string()
        } else {
            normalized
        }
    };
    let market_id = if requested_market_id.is_empty() {
        let derived_market_id = display_name
            .map(derive_market_id_from_label)
            .or_else(|| {
                let derived = derive_market_id_from_label(&request.base_asset);
                (!derived.is_empty()).then_some(derived)
            })
            .ok_or(AdminError::MissingMarketLabel)?;
        derived_market_id
    } else {
        requested_market_id
    };
    validate_market_id(&market_id)?;
    let base_asset = derive_base_asset(&request.base_asset, display_name, &market_id);
    if base_asset.is_empty() {
        return Err(AdminError::MissingMarketLabel);
    }
    if request.tick_size == 0 {
        return Err(AdminError::InvalidTickSize);
    }
    if request.min_order_quantity == 0 {
        return Err(AdminError::InvalidMinimumOrderQuantity);
    }
    validate_market_price_bounds(request.min_price, request.max_price, request.tick_size)?;

    let now = Utc::now();
    Ok(MarketDefinition {
        market_id: market_id.clone(),
        display_name: display_name.unwrap_or(&market_id).to_string(),
        base_asset,
        quote_asset,
        tick_size: request.tick_size,
        min_order_quantity: request.min_order_quantity,
        min_price: request.min_price,
        max_price: request.max_price,
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

fn validate_market_price_bounds(
    min_price: Option<u64>,
    max_price: Option<u64>,
    tick_size: u64,
) -> Result<(), AdminError> {
    if let Some(min_price) = min_price {
        if min_price == 0 {
            return Err(AdminError::InvalidMinimumAllowedPrice);
        }
        if min_price % tick_size != 0 {
            return Err(AdminError::PriceBoundsTickSizeViolation { tick_size });
        }
    }
    if let Some(max_price) = max_price {
        if max_price == 0 {
            return Err(AdminError::InvalidMaximumAllowedPrice);
        }
        if max_price % tick_size != 0 {
            return Err(AdminError::PriceBoundsTickSizeViolation { tick_size });
        }
    }
    if let (Some(min_price), Some(max_price)) = (min_price, max_price) {
        if max_price < min_price {
            return Err(AdminError::InvalidMaximumAllowedPriceRange);
        }
    }
    Ok(())
}

async fn market_mark_price(state: &AppState, market: &MarketDefinition) -> u64 {
    if let Some(settlement_price) = market.settlement_price {
        return settlement_price;
    }
    let (best_bid, best_ask) = state.market_best_prices(&market.market_id).await;
    match (best_bid, best_ask) {
        (Some(bid), Some(ask)) => bid.saturating_add(ask) / 2,
        (Some(bid), None) => bid,
        (None, Some(ask)) => ask,
        (None, None) => market.reference_price.unwrap_or(0),
    }
}

fn publish_admin_message(state: &AppState, entry: AdminMessageEntry) {
    let message = ServerMessage::AdminMessage {
        message: entry.clone(),
    };
    if let Some(trader_id) = entry.target_trader_id {
        state.dispatch_user_event(trader_id, message);
        return;
    }
    state.dispatch_system_message(message);
}

fn publish_market_state(state: &AppState, market: MarketDefinition) {
    state.dispatch_public_message(ServerMessage::MarketState { market });
}

fn publish_market_deleted(state: &AppState, market_id: &str) {
    state.dispatch_public_message(ServerMessage::MarketDeleted {
        market_id: market_id.to_string(),
    });
}

fn publish_user_event(state: &AppState, trader_id: Uuid, message: ServerMessage) {
    state.dispatch_user_event(trader_id, message);
}

fn publish_market_resync(state: &AppState, market: &str, reason: &str) {
    state.dispatch_public_message(ServerMessage::ResyncRequired {
        channel: DATA_STREAM_CHANNEL.to_string(),
        market: Some(market.to_string()),
        expected_sequence: None,
        current_sequence: None,
        reason: reason.to_string(),
    });
}

fn publish_user_resync(state: &AppState, reason: &str) {
    state.dispatch_system_message(ServerMessage::ResyncRequired {
        channel: "user".to_string(),
        market: None,
        expected_sequence: None,
        current_sequence: None,
        reason: reason.to_string(),
    });
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
    use crate::accounts::{UserProfile, UserRecord, UserRole};
    use crate::config::Config;
    use crate::settlement::SettlementEngine;

    fn test_state() -> AppState {
        AppState::new(Config {
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
                min_price: None,
                max_price: None,
                reference_price: None,
                enabled: true,
            },
        )
        .expect_err("market should be rejected");

        assert_eq!(error, AdminError::InvalidMarketId);
    }

    #[test]
    fn upsert_market_defaults_quote_asset_and_generates_market_id_from_display_name() {
        let state = test_state();
        let market = AdminService::upsert_market(
            &state,
            &admin(),
            UpsertMarketRequest {
                market_id: String::new(),
                display_name: Some("Solana Winner".to_string()),
                base_asset: String::new(),
                quote_asset: String::new(),
                tick_size: 1,
                min_order_quantity: 1,
                min_price: Some(10),
                max_price: Some(200),
                reference_price: Some(100),
                enabled: true,
            },
        )
        .expect("market should be created");

        assert_eq!(market.market_id, "SOLANA-WINNER-MARKET");
        assert_eq!(market.base_asset, "SOLANA-WINNER");
        assert_eq!(market.quote_asset, DEFAULT_COMPETITION_QUOTE_ASSET);
        assert_eq!(market.min_price, Some(10));
        assert_eq!(market.max_price, Some(200));
    }

    #[test]
    fn upsert_market_request_accepts_min_and_max_config_keys() {
        let request: UpsertMarketRequest = serde_json::from_value(serde_json::json!({
            "market_id": "BTC-USD",
            "display_name": "Bitcoin",
            "base_asset": "BTC",
            "quote_asset": "USD",
            "tick_size": 5,
            "min_order_quantity": 1,
            "min": 50,
            "max": 150,
            "enabled": true
        }))
        .expect("request should deserialize");

        assert_eq!(request.min_price, Some(50));
        assert_eq!(request.max_price, Some(150));
    }

    #[tokio::test]
    async fn load_exchange_config_upserts_bots_after_markets() {
        let state = test_state();

        let response = AdminService::load_exchange_config(
            &state,
            &admin(),
            LoadExchangeConfigRequest {
                trading_enabled: Some(true),
                markets: vec![UpsertMarketRequest {
                    market_id: "BTC-USD".to_string(),
                    display_name: None,
                    base_asset: "BTC".to_string(),
                    quote_asset: "USD".to_string(),
                    tick_size: 1,
                    min_order_quantity: 1,
                    min_price: Some(90),
                    max_price: Some(110),
                    reference_price: Some(100),
                    enabled: true,
                }],
                bots: vec![UpsertAdminBotRequest {
                    bot_id: "depth-maker-1".to_string(),
                    display_name: Some("Depth maker".to_string()),
                    market_id: "BTC-USD".to_string(),
                    strategy: crate::bots::BotStrategy::Maker,
                    side_mode: crate::bots::BotSideMode::Both,
                    min_quantity: 1,
                    max_quantity: 2,
                    interval_ms: 1_000,
                    max_open_orders: 2,
                    min_price: 99,
                    max_price: 101,
                    start_immediately: false,
                }],
            },
        )
        .await
        .expect("config should load");

        assert_eq!(response.markets.len(), 1);
        assert_eq!(response.markets[0].min_price, Some(90));
        assert_eq!(response.markets[0].max_price, Some(110));
        assert_eq!(response.bots.len(), 1);
        assert_eq!(response.bots[0].bot_id, "depth-maker-1");
        assert_eq!(response.bots[0].market_id, "BTC-USD");
        assert_eq!(state.bot_manager.list().len(), 1);
    }

    #[tokio::test]
    async fn leaderboard_marks_positions_using_market_reference_prices() {
        let state = test_state();
        let trader_a = UserRecord {
            profile: UserProfile {
                trader_id: Uuid::new_v4(),
                username: "alice".to_string(),
                team_number: "TEAM-ALICE".to_string(),
                api_key: "exch_alice".to_string(),
                role: UserRole::Trader,
                created_at: Utc::now(),
            },
        };
        let trader_b = UserRecord {
            profile: UserProfile {
                trader_id: Uuid::new_v4(),
                username: "bob".to_string(),
                team_number: "TEAM-BOB".to_string(),
                api_key: "exch_bob".to_string(),
                role: UserRole::Trader,
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
                min_price: None,
                max_price: None,
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
        assert_eq!(leaderboard[0].team_number, "TEAM-ALICE");
        assert_eq!(leaderboard[0].net_pnl, 40);
        assert_eq!(leaderboard[1].team_number, "TEAM-BOB");
        assert_eq!(leaderboard[1].net_pnl, 25);
    }
}
