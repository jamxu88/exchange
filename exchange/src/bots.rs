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
use tokio::time::{self, MissedTickBehavior};
use tracing::info;
use utoipa::ToSchema;
use uuid::Uuid;

pub const ADMIN_DESK_USERNAME: &str = "admin-desk";
const MIN_BOT_INTERVAL_MS: u64 = 100;

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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct AdminBotState {
    pub bot_id: String,
    pub display_name: String,
    pub trader_id: Uuid,
    pub trader_username: String,
    pub market_id: String,
    pub order_type: OrderType,
    pub side_mode: BotSideMode,
    pub status: BotStatus,
    pub min_quantity: u64,
    pub max_quantity: u64,
    pub interval_ms: u64,
    pub max_open_orders: usize,
    pub price_offset_ticks: u64,
    pub walk_step_ticks: u64,
    pub fallback_price: Option<u64>,
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
    pub order_type: OrderType,
    pub side_mode: BotSideMode,
    pub min_quantity: u64,
    pub max_quantity: u64,
    pub interval_ms: u64,
    pub max_open_orders: usize,
    pub price_offset_ticks: u64,
    pub walk_step_ticks: u64,
    pub fallback_price: Option<u64>,
    #[serde(default)]
    pub start_immediately: bool,
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
    #[error("bot order interval must be at least {minimum_ms} ms")]
    IntervalTooLow { minimum_ms: u64 },
    #[error("minimum quantity must be greater than zero")]
    InvalidMinimumQuantity,
    #[error("maximum quantity must be at least the minimum quantity")]
    InvalidMaximumQuantity,
    #[error("max open orders must be greater than zero")]
    InvalidOpenOrderLimit,
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
            | Self::IntervalTooLow { .. }
            | Self::InvalidMinimumQuantity
            | Self::InvalidMaximumQuantity
            | Self::InvalidOpenOrderLimit => StatusCode::BAD_REQUEST,
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
        if request.interval_ms < MIN_BOT_INTERVAL_MS {
            return Err(BotControlError::IntervalTooLow {
                minimum_ms: MIN_BOT_INTERVAL_MS,
            });
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
        if state.storage.get_market(&market_id).is_none() {
            return Err(BotControlError::MarketNotFound);
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
                order_type: request.order_type,
                side_mode: request.side_mode,
                status: BotStatus::Paused,
                min_quantity: request.min_quantity,
                max_quantity: request.max_quantity,
                interval_ms: request.interval_ms,
                max_open_orders: request.max_open_orders,
                price_offset_ticks: request.price_offset_ticks,
                walk_step_ticks: request.walk_step_ticks,
                fallback_price: request.fallback_price,
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
                "bot_id={} market={} side_mode={:?} order_type={:?} interval_ms={} quantity={}..{}",
                next_state.bot_id,
                next_state.market_id,
                next_state.side_mode,
                next_state.order_type,
                next_state.interval_ms,
                next_state.min_quantity,
                next_state.max_quantity
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
    let mut ticker = time::interval(time::Duration::from_millis(config.interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut side_toggle = false;
    let mut rng = BotRng::seeded(&config.bot_id, config.trader_id);
    let mut anchor_price = initial_anchor_price(&state, &config).await;

    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                break;
            }
            _ = ticker.tick() => {
                let Some(market) = state.storage.get_market(&config.market_id) else {
                    manager.record_error(&bot_id, "market is not configured");
                    continue;
                };
                if market.status == MarketStatus::Settled {
                    manager.record_error(&bot_id, "market has already been settled");
                    continue;
                }
                let open_orders = state.storage.list_open_orders(config.trader_id, Some(&config.market_id));
                if open_orders.len() >= config.max_open_orders {
                    continue;
                }

                let current_anchor = market_anchor_price(&state, &config, anchor_price, market.reference_price).await;
                anchor_price = walk_anchor_price(current_anchor, market.tick_size, config.walk_step_ticks, &mut rng);
                let side = select_side(config.side_mode, &mut side_toggle);
                let quantity = rng.range_u64(config.min_quantity, config.max_quantity);
                let offset = market.tick_size.saturating_mul(config.price_offset_ticks);
                let price = match config.order_type {
                    OrderType::Market => 0,
                    OrderType::Limit => match side {
                        Side::Buy => anchor_price.saturating_sub(offset).max(market.tick_size),
                        Side::Sell => anchor_price.saturating_add(offset).max(market.tick_size),
                    },
                };

                match TradingService::submit_order(
                    &state,
                    config.trader_id,
                    SubmitOrderRequest {
                        market: config.market_id.clone(),
                        side,
                        order_type: config.order_type,
                        price,
                        quantity,
                    },
                ).await {
                    Ok(_) => manager.record_submission(&bot_id),
                    Err(error) => manager.record_error(&bot_id, error.to_string()),
                }
            }
        }
    }

    manager.mark_stopped(&bot_id);
}

async fn initial_anchor_price(state: &AppState, config: &AdminBotState) -> u64 {
    market_anchor_price(
        state,
        config,
        config.fallback_price.unwrap_or(1),
        config.fallback_price,
    )
    .await
}

async fn market_anchor_price(
    state: &AppState,
    config: &AdminBotState,
    previous_anchor: u64,
    market_reference_price: Option<u64>,
) -> u64 {
    let (best_bid, best_ask) = state.market_best_prices(&config.market_id).await;
    match (best_bid, best_ask) {
        (Some(bid), Some(ask)) => bid.saturating_add(ask) / 2,
        (Some(bid), None) => bid,
        (None, Some(ask)) => ask,
        (None, None) => config
            .fallback_price
            .or(market_reference_price)
            .unwrap_or(previous_anchor.max(1)),
    }
}

fn walk_anchor_price(
    anchor_price: u64,
    tick_size: u64,
    walk_step_ticks: u64,
    rng: &mut BotRng,
) -> u64 {
    if walk_step_ticks == 0 {
        return anchor_price.max(tick_size);
    }

    let max_delta = i64::try_from(walk_step_ticks).unwrap_or(i64::MAX);
    let delta_ticks = rng.range_i64(-max_delta, max_delta);
    let tick_size_i64 = i64::try_from(tick_size).unwrap_or(i64::MAX);
    let price_i64 = i64::try_from(anchor_price).unwrap_or(i64::MAX);
    let next = price_i64.saturating_add(delta_ticks.saturating_mul(tick_size_i64));
    u64::try_from(next.max(tick_size_i64)).unwrap_or(tick_size)
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
            role: Some(UserRole::Trader),
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

    fn range_i64(&mut self, min: i64, max: i64) -> i64 {
        if min >= max {
            return min;
        }
        let span = u64::try_from(max.saturating_sub(min).saturating_add(1)).unwrap_or(u64::MAX);
        min.saturating_add(i64::try_from(self.next_u64() % span).unwrap_or(0))
    }
}
