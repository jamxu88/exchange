use crate::accounts::{UserProfile, UserRole};
use crate::admin::{AdminAuditEntry, MarketStatus};
use crate::auth::{AuthError, AuthService, AuthenticatedAdmin, ProvisionUserRequest};
use crate::orderbook::Side;
use crate::settlement::SettlementEngine;
use crate::state::AppState;
use crate::trading::{
    OrderType, SubmitOrderRequest, SubmitOrderResponse, TradingError, TradingService,
};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::info;
use utoipa::ToSchema;
use uuid::Uuid;

pub const ADMIN_DESK_USERNAME: &str = "admin-desk";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BotSideMode {
    Buy,
    Sell,
    Both,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BotStatus {
    Paused,
    Running,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BotStrategy {
    Maker,
    Taker,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct AdminBotState {
    pub bot_id: String,
    pub display_name: String,
    pub trader_id: Uuid,
    pub trader_username: String,
    pub market_id: String,
    pub strategy: BotStrategy,
    pub side_mode: BotSideMode,
    pub status: BotStatus,
    pub min_quantity: u64,
    pub max_quantity: u64,
    pub interval_ms: u64,
    pub max_open_orders: usize,
    pub min_price: u64,
    pub max_price: u64,
    pub last_error: Option<String>,
    pub last_submitted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertAdminBotRequest {
    pub bot_id: String,
    pub display_name: Option<String>,
    pub market_id: String,
    #[serde(default)]
    pub strategy: BotStrategy,
    pub side_mode: BotSideMode,
    pub min_quantity: u64,
    pub max_quantity: u64,
    pub interval_ms: u64,
    pub max_open_orders: usize,
    pub min_price: u64,
    pub max_price: u64,
    #[serde(default)]
    pub start_immediately: bool,
}

impl Default for BotStrategy {
    fn default() -> Self {
        Self::Maker
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct AdminDeskSummary {
    pub trader_id: Uuid,
    pub username: String,
    pub position_limit: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminDeskOrderRequest {
    pub market: String,
    pub side: Side,
    #[serde(default)]
    pub order_type: OrderType,
    pub price: u64,
    pub quantity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdminDeskOrderResponse {
    pub desk: AdminDeskSummary,
    pub submission: SubmitOrderResponse,
}

#[derive(Debug, Error)]
pub enum BotControlError {
    #[error("bot id is required")]
    MissingBotId,
    #[error("bot id may only contain lowercase letters, numbers, and hyphens")]
    InvalidBotId,
    #[error("market id is required")]
    MissingMarketId,
    #[error("market is not configured")]
    MarketNotFound,
    #[error("minimum quantity must be greater than zero")]
    InvalidMinimumQuantity,
    #[error("maximum quantity must be at least the minimum quantity")]
    InvalidMaximumQuantity,
    #[error("max open orders must be greater than zero")]
    InvalidOpenOrderLimit,
    #[error("minimum price must be at least one tick ({tick_size})")]
    InvalidMinimumPrice { tick_size: u64 },
    #[error("maximum price must be at least the minimum price")]
    InvalidMaximumPrice,
    #[error("price bounds must align to market tick size {tick_size}")]
    PriceBoundsTickSizeViolation { tick_size: u64 },
    #[error("bot not found")]
    BotNotFound,
    #[error(transparent)]
    Auth(#[from] AuthError),
}

impl BotControlError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::MissingBotId
            | Self::InvalidBotId
            | Self::MissingMarketId
            | Self::InvalidMinimumQuantity
            | Self::InvalidMaximumQuantity
            | Self::InvalidOpenOrderLimit
            | Self::InvalidMinimumPrice { .. }
            | Self::InvalidMaximumPrice
            | Self::PriceBoundsTickSizeViolation { .. } => StatusCode::BAD_REQUEST,
            Self::MarketNotFound | Self::BotNotFound => StatusCode::NOT_FOUND,
            Self::Auth(error) => error.status_code(),
        }
    }
}

#[derive(Debug, Error)]
pub enum AdminDeskError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Trading(#[from] TradingError),
}

impl AdminDeskError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Auth(error) => error.status_code(),
            Self::Trading(error) => error.status_code(),
        }
    }
}

#[derive(Clone, Default)]
pub struct BotManager {
    inner: Arc<Mutex<BTreeMap<String, BotRecord>>>,
}

struct BotRecord {
    state: AdminBotState,
    stop_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl BotManager {
    pub fn list(&self) -> Vec<AdminBotState> {
        let bots = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bots.values().map(|record| record.state.clone()).collect()
    }

    pub async fn upsert(
        &self,
        state: &AppState,
        admin: &AuthenticatedAdmin,
        request: UpsertAdminBotRequest,
    ) -> Result<AdminBotState, BotControlError> {
        let bot_id = normalize_bot_id(&request.bot_id)?;
        let market_id = request.market_id.trim().to_ascii_uppercase();
        if market_id.is_empty() {
            return Err(BotControlError::MissingMarketId);
        }
        if request.min_quantity == 0 {
            return Err(BotControlError::InvalidMinimumQuantity);
        }
        if request.max_quantity < request.min_quantity {
            return Err(BotControlError::InvalidMaximumQuantity);
        }
        if request.max_open_orders == 0 {
            return Err(BotControlError::InvalidOpenOrderLimit);
        }
        let market = state
            .storage
            .get_market(&market_id)
            .ok_or(BotControlError::MarketNotFound)?;
        if request.min_price < market.tick_size {
            return Err(BotControlError::InvalidMinimumPrice {
                tick_size: market.tick_size,
            });
        }
        if request.max_price < request.min_price {
            return Err(BotControlError::InvalidMaximumPrice);
        }
        if request.min_price % market.tick_size != 0 || request.max_price % market.tick_size != 0 {
            return Err(BotControlError::PriceBoundsTickSizeViolation {
                tick_size: market.tick_size,
            });
        }

        let should_restart = {
            let bots = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            bots.get(&bot_id)
                .map(|record| record.state.status == BotStatus::Running)
                .unwrap_or(false)
        };
        if should_restart {
            let _ = self.pause(state, admin, &bot_id).await;
        }

        let trader_profile = ensure_bot_user(state, admin, &bot_id)?;
        let now = Utc::now();
        let display_name = request
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&bot_id)
            .to_string();
        let next_state = {
            let mut bots = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let created_at = bots
                .get(&bot_id)
                .map(|record| record.state.created_at)
                .unwrap_or(now);
            let last_submitted_at = bots
                .get(&bot_id)
                .and_then(|record| record.state.last_submitted_at);
            let last_error = bots
                .get(&bot_id)
                .and_then(|record| record.state.last_error.clone());
            let next_state = AdminBotState {
                bot_id: bot_id.clone(),
                display_name,
                trader_id: trader_profile.trader_id,
                trader_username: trader_profile.username.clone(),
                market_id,
                strategy: request.strategy,
                side_mode: request.side_mode,
                status: BotStatus::Paused,
                min_quantity: request.min_quantity,
                max_quantity: request.max_quantity,
                interval_ms: request.interval_ms,
                max_open_orders: request.max_open_orders,
                min_price: request.min_price,
                max_price: request.max_price,
                last_error,
                last_submitted_at,
                created_at,
                updated_at: now,
            };
            bots.insert(
                bot_id.clone(),
                BotRecord {
                    state: next_state.clone(),
                    stop_tx: None,
                    task: None,
                },
            );
            next_state
        };

        record_admin_audit(
            state,
            admin,
            "save_bot",
            Some(trader_profile.username.clone()),
            Some(trader_profile.trader_id),
            format!(
                "bot_id={} market={} strategy={:?} side_mode={:?} interval_ms={} quantity={}..{} price_bounds={}..{}",
                next_state.bot_id,
                next_state.market_id,
                next_state.strategy,
                next_state.side_mode,
                next_state.interval_ms,
                next_state.min_quantity,
                next_state.max_quantity,
                next_state.min_price,
                next_state.max_price
            ),
        );

        if request.start_immediately || should_restart {
            self.start(state.clone(), admin, &bot_id).await
        } else {
            Ok(next_state)
        }
    }

    pub async fn start(
        &self,
        state: AppState,
        admin: &AuthenticatedAdmin,
        bot_id: &str,
    ) -> Result<AdminBotState, BotControlError> {
        let normalized_bot_id = normalize_bot_id(bot_id)?;
        let snapshot = {
            let mut bots = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let record = bots
                .get_mut(&normalized_bot_id)
                .ok_or(BotControlError::BotNotFound)?;
            if record.state.status == BotStatus::Running {
                return Ok(record.state.clone());
            }
            let (stop_tx, stop_rx) = oneshot::channel();
            record.state.status = BotStatus::Running;
            record.state.updated_at = Utc::now();
            record.stop_tx = Some(stop_tx);
            let snapshot = record.state.clone();
            let task_snapshot = snapshot.clone();
            let manager = self.clone();
            let app_state = state.clone();
            let spawned_bot_id = normalized_bot_id.clone();
            record.task = Some(tokio::spawn(async move {
                run_bot_loop(app_state, manager, spawned_bot_id, task_snapshot, stop_rx).await;
            }));
            snapshot
        };

        record_admin_audit(
            &state,
            admin,
            "start_bot",
            Some(snapshot.trader_username.clone()),
            Some(snapshot.trader_id),
            format!("bot_id={} market={}", snapshot.bot_id, snapshot.market_id),
        );

        Ok(snapshot)
    }

    pub async fn pause(
        &self,
        state: &AppState,
        admin: &AuthenticatedAdmin,
        bot_id: &str,
    ) -> Result<AdminBotState, BotControlError> {
        let normalized_bot_id = normalize_bot_id(bot_id)?;
        let (snapshot, stop_tx, handle) = {
            let mut bots = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let record = bots
                .get_mut(&normalized_bot_id)
                .ok_or(BotControlError::BotNotFound)?;
            record.state.status = BotStatus::Paused;
            record.state.updated_at = Utc::now();
            (
                record.state.clone(),
                record.stop_tx.take(),
                record.task.take(),
            )
        };

        if let Some(stop_tx) = stop_tx {
            let _ = stop_tx.send(());
        }
        if let Some(handle) = handle {
            let _ = handle.await;
        }

        record_admin_audit(
            state,
            admin,
            "pause_bot",
            Some(snapshot.trader_username.clone()),
            Some(snapshot.trader_id),
            format!("bot_id={} market={}", snapshot.bot_id, snapshot.market_id),
        );

        Ok(snapshot)
    }

    pub async fn delete(
        &self,
        state: &AppState,
        admin: &AuthenticatedAdmin,
        bot_id: &str,
    ) -> Result<AdminBotState, BotControlError> {
        let normalized_bot_id = normalize_bot_id(bot_id)?;
        if self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&normalized_bot_id)
            .is_none()
        {
            return Err(BotControlError::BotNotFound);
        }
        let _ = self.pause(state, admin, &normalized_bot_id).await;
        let deleted = {
            let mut bots = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            bots.remove(&normalized_bot_id)
                .map(|record| record.state)
                .ok_or(BotControlError::BotNotFound)?
        };

        record_admin_audit(
            state,
            admin,
            "delete_bot",
            Some(deleted.trader_username.clone()),
            Some(deleted.trader_id),
            format!("bot_id={} market={}", deleted.bot_id, deleted.market_id),
        );

        Ok(deleted)
    }

    pub async fn start_all(
        &self,
        state: AppState,
        admin: &AuthenticatedAdmin,
    ) -> Result<Vec<AdminBotState>, BotControlError> {
        let bot_ids = self
            .list()
            .into_iter()
            .map(|bot| bot.bot_id)
            .collect::<Vec<_>>();
        let mut started = Vec::with_capacity(bot_ids.len());
        for bot_id in bot_ids {
            started.push(self.start(state.clone(), admin, &bot_id).await?);
        }
        Ok(started)
    }

    pub async fn pause_all(
        &self,
        state: &AppState,
        admin: &AuthenticatedAdmin,
    ) -> Result<Vec<AdminBotState>, BotControlError> {
        let bot_ids = self
            .list()
            .into_iter()
            .map(|bot| bot.bot_id)
            .collect::<Vec<_>>();
        let mut paused = Vec::with_capacity(bot_ids.len());
        for bot_id in bot_ids {
            paused.push(self.pause(state, admin, &bot_id).await?);
        }
        Ok(paused)
    }

    pub async fn delete_all(
        &self,
        state: &AppState,
        admin: &AuthenticatedAdmin,
    ) -> Result<Vec<AdminBotState>, BotControlError> {
        let bot_ids = self
            .list()
            .into_iter()
            .map(|bot| bot.bot_id)
            .collect::<Vec<_>>();
        let mut deleted = Vec::with_capacity(bot_ids.len());
        for bot_id in bot_ids {
            deleted.push(self.delete(state, admin, &bot_id).await?);
        }
        Ok(deleted)
    }

    fn record_submission(&self, bot_id: &str) {
        if let Some(record) = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get_mut(bot_id)
        {
            record.state.last_error = None;
            record.state.last_submitted_at = Some(Utc::now());
            record.state.updated_at = Utc::now();
        }
    }

    fn record_error(&self, bot_id: &str, error: impl Into<String>) {
        if let Some(record) = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get_mut(bot_id)
        {
            record.state.last_error = Some(error.into());
            record.state.updated_at = Utc::now();
        }
    }

    fn mark_stopped(&self, bot_id: &str) {
        if let Some(record) = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get_mut(bot_id)
        {
            record.state.status = BotStatus::Paused;
            record.state.updated_at = Utc::now();
            record.stop_tx = None;
            record.task = None;
        }
    }
}

pub fn admin_desk_summary(state: &AppState) -> Option<AdminDeskSummary> {
    state
        .storage
        .get_user_by_username(ADMIN_DESK_USERNAME)
        .map(|user| profile_to_desk_summary(&user.profile))
}

pub fn ensure_admin_desk(
    state: &AppState,
    admin: &AuthenticatedAdmin,
) -> Result<AdminDeskSummary, AuthError> {
    let profile = ensure_admin_desk_profile(state, admin)?;
    Ok(profile_to_desk_summary(&profile))
}

pub async fn submit_admin_desk_order(
    state: &AppState,
    admin: &AuthenticatedAdmin,
    request: AdminDeskOrderRequest,
) -> Result<AdminDeskOrderResponse, AdminDeskError> {
    let desk = ensure_admin_desk_profile(state, admin)?;
    let summary = profile_to_desk_summary(&desk);
    let submission = TradingService::submit_order(
        state,
        desk.trader_id,
        SubmitOrderRequest {
            market: request.market,
            side: request.side,
            order_type: request.order_type,
            price: request.price,
            quantity: request.quantity,
        },
    )
    .await?;

    record_admin_audit(
        state,
        admin,
        "submit_admin_desk_order",
        Some(summary.username.clone()),
        Some(summary.trader_id),
        format!(
            "market={} side={:?} order_type={:?} quantity={} price={} resting={}",
            submission.order.market,
            submission.order.side,
            request.order_type,
            submission.order.quantity,
            request.price,
            submission.resting
        ),
    );

    Ok(AdminDeskOrderResponse {
        desk: summary,
        submission,
    })
}

async fn run_bot_loop(
    state: AppState,
    manager: BotManager,
    bot_id: String,
    config: AdminBotState,
    mut stop_rx: oneshot::Receiver<()>,
) {
    let mut side_toggle = false;
    let mut rng = BotRng::seeded(&config.bot_id, config.trader_id);
    let mut first_iteration = true;

    loop {
        if first_iteration {
            first_iteration = false;
        } else if config.interval_ms == 0 {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = tokio::task::yield_now() => {}
            }
        } else {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = tokio::time::sleep(std::time::Duration::from_millis(config.interval_ms)) => {}
            }
        }

        let Some(market) = state.storage.get_market(&config.market_id) else {
            manager.record_error(&bot_id, "market is not configured");
            continue;
        };
        if market.status == MarketStatus::Settled {
            manager.record_error(&bot_id, "market has already been settled");
            continue;
        }
        let open_orders = state
            .storage
            .list_open_orders(config.trader_id, Some(&config.market_id));
        if open_orders.len() >= config.max_open_orders {
            continue;
        }

        let side = select_side(config.side_mode, &mut side_toggle);
        let quantity = rng.range_u64(config.min_quantity, config.max_quantity);
        let Some(request) = build_bot_order_request(
            &state,
            &config,
            &market.market_id,
            market.tick_size,
            side,
            quantity,
            &mut rng,
        )
        .await
        else {
            continue;
        };

        match TradingService::submit_order(&state, config.trader_id, request).await {
            Ok(_) => manager.record_submission(&bot_id),
            Err(TradingError::NoLiquidity) if config.strategy == BotStrategy::Taker => {}
            Err(error) => manager.record_error(&bot_id, error.to_string()),
        }
    }

    manager.mark_stopped(&bot_id);
}

async fn build_bot_order_request(
    state: &AppState,
    config: &AdminBotState,
    market_id: &str,
    tick_size: u64,
    side: Side,
    quantity: u64,
    rng: &mut BotRng,
) -> Option<SubmitOrderRequest> {
    match config.strategy {
        BotStrategy::Taker => {
            if !taker_opposite_best_in_price_range(state, config, side).await {
                return None;
            }
            Some(SubmitOrderRequest {
                market: market_id.to_string(),
                side,
                order_type: OrderType::Market,
                price: 0,
                quantity,
            })
        }
        BotStrategy::Maker => {
            let random_price = bounded_price(config.min_price, config.max_price, tick_size, rng);
            let price =
                maker_price_within_bounds(state, config, side, tick_size, random_price).await;
            Some(SubmitOrderRequest {
                market: market_id.to_string(),
                side,
                order_type: OrderType::Limit,
                price,
                quantity,
            })
        }
    }
}

/// For taker (market) bots: only act when the book's best opposite price lies in
/// `[min_price, max_price]`. Otherwise we skip this tick (no resting limits).
async fn taker_opposite_best_in_price_range(
    state: &AppState,
    config: &AdminBotState,
    side: Side,
) -> bool {
    let (best_bid, best_ask) = state.market_best_prices(&config.market_id).await;
    let in_range = |price: u64| price >= config.min_price && price <= config.max_price;
    match side {
        Side::Buy => best_ask.map(in_range).unwrap_or(false),
        Side::Sell => best_bid.map(in_range).unwrap_or(false),
    }
}

async fn maker_price_within_bounds(
    state: &AppState,
    config: &AdminBotState,
    side: Side,
    tick_size: u64,
    random_price: u64,
) -> u64 {
    let (best_bid, best_ask) = state.market_best_prices(&config.market_id).await;
    match side {
        Side::Buy => {
            let ceiling = best_ask
                .map(|ask| ask.saturating_sub(tick_size).max(tick_size))
                .unwrap_or(config.max_price);
            random_price
                .min(ceiling)
                .clamp(config.min_price, config.max_price)
        }
        Side::Sell => {
            let floor = best_bid
                .map(|bid| bid.saturating_add(tick_size).max(tick_size))
                .unwrap_or(config.min_price);
            random_price
                .max(floor)
                .clamp(config.min_price, config.max_price)
        }
    }
}

fn bounded_price(min_price: u64, max_price: u64, tick_size: u64, rng: &mut BotRng) -> u64 {
    let min_ticks = min_price / tick_size;
    let max_ticks = max_price / tick_size;
    rng.range_u64(min_ticks, max_ticks)
        .saturating_mul(tick_size)
}

fn select_side(side_mode: BotSideMode, toggle: &mut bool) -> Side {
    match side_mode {
        BotSideMode::Buy => Side::Buy,
        BotSideMode::Sell => Side::Sell,
        BotSideMode::Both => {
            *toggle = !*toggle;
            if *toggle { Side::Buy } else { Side::Sell }
        }
    }
}

fn normalize_bot_id(value: &str) -> Result<String, BotControlError> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(BotControlError::MissingBotId);
    }
    if normalized.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    }) {
        Ok(normalized)
    } else {
        Err(BotControlError::InvalidBotId)
    }
}

fn ensure_bot_user(
    state: &AppState,
    admin: &AuthenticatedAdmin,
    bot_id: &str,
) -> Result<UserProfile, AuthError> {
    let username = format!("bot-{bot_id}");
    if let Some(user) = state.storage.get_user_by_username(&username) {
        return Ok(user.profile);
    }

    Ok(AuthService::provision_user_as_admin(
        state,
        admin,
        ProvisionUserRequest {
            username,
            team_number: None,
            role: Some(UserRole::Admin),
        },
    )?
    .profile)
}

fn ensure_admin_desk_profile(
    state: &AppState,
    admin: &AuthenticatedAdmin,
) -> Result<UserProfile, AuthError> {
    if let Some(user) = state.storage.get_user_by_username(ADMIN_DESK_USERNAME) {
        return Ok(user.profile);
    }

    Ok(AuthService::provision_user_as_admin(
        state,
        admin,
        ProvisionUserRequest {
            username: ADMIN_DESK_USERNAME.to_string(),
            team_number: None,
            role: Some(UserRole::Admin),
        },
    )?
    .profile)
}

fn profile_to_desk_summary(profile: &UserProfile) -> AdminDeskSummary {
    AdminDeskSummary {
        trader_id: profile.trader_id,
        username: profile.username.clone(),
        position_limit: SettlementEngine::position_limit_for_role(profile.role),
        created_at: profile.created_at,
    }
}

fn record_admin_audit(
    state: &AppState,
    admin: &AuthenticatedAdmin,
    action: &str,
    target_username: Option<String>,
    target_trader_id: Option<Uuid>,
    details: impl Into<String>,
) {
    let details = details.into();
    let entry = AdminAuditEntry {
        audit_id: Uuid::new_v4(),
        actor_username: admin.username.clone(),
        action: action.to_string(),
        target_username: target_username.clone(),
        target_trader_id,
        details: details.clone(),
        occurred_at: Utc::now(),
    };

    info!(
        actor_username = admin.username,
        action,
        target_username = ?target_username,
        target_trader_id = ?entry.target_trader_id,
        details,
        "admin audit event"
    );
    state.storage.append_admin_audit_log(entry);
}

struct BotRng {
    state: u64,
}

impl BotRng {
    fn seeded(bot_id: &str, trader_id: Uuid) -> Self {
        let mut seed = trader_id.as_u128() as u64;
        for byte in bot_id.as_bytes() {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(u64::from(*byte) + 1);
        }
        Self { state: seed.max(1) }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }

    fn range_u64(&mut self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        let span = max.saturating_sub(min).saturating_add(1);
        min.saturating_add(self.next_u64() % span)
    }
}
