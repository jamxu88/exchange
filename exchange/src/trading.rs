use crate::admin::{MarketDefinition, MarketStatus};
use crate::marketdata::{MarketEvent, MarketEventRemoveReason, OrderStateStatus, ServerMessage};
use crate::matching::{MatchExecution, MatchingEngine};
use crate::orderbook::{BookLevel, Fill, Order, OrderBook, Side};
use crate::settlement::{SettlementError, apply_fill_to_position, should_persist_position};
use crate::state::{AccountBarrierTelemetry, AppState, BarrierKind, Position};
use axum::http::StatusCode;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use utoipa::ToSchema;
use uuid::Uuid;

const MARKET_ENGINE_COMMAND_BUFFER: usize = 4_096;
const MAX_PERSISTABLE_U64: u64 = i64::MAX as u64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    #[default]
    Limit,
    Market,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubmitOrderRequest {
    pub market: String,
    pub side: Side,
    #[serde(default)]
    pub order_type: OrderType,
    pub price: u64,
    pub quantity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AmendOrderRequest {
    pub remaining: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubmitOrderResponse {
    pub order: Order,
    pub fills: Vec<Fill>,
    pub resting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CancelOrderResponse {
    pub order: Order,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AmendOrderResponse {
    pub order: Order,
}

#[derive(Debug, Clone, Default)]
pub struct MarketBookSnapshot {
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
}

#[derive(Debug, Clone, Copy, Default)]
struct TraderMarketExposure {
    net_quantity: i64,
    average_entry_price: Option<u64>,
    realized_pnl: i64,
    pending_buy_quantity: i64,
    pending_sell_quantity: i64,
}

#[derive(Default)]
struct MarketExposureBook {
    traders: HashMap<Uuid, TraderMarketExposure>,
}

impl MarketExposureBook {
    fn recover(state: &AppState, market: &str, orderbook: &OrderBook) -> Self {
        let mut book = Self::default();
        for (trader_id, positions) in state.storage.list_all_positions() {
            if let Some(position) = positions
                .into_iter()
                .find(|position| position.market == market)
            {
                let exposure = book.exposure_mut(trader_id);
                exposure.net_quantity = position.net_quantity;
                exposure.average_entry_price = position.average_entry_price;
                exposure.realized_pnl = position.realized_pnl;
            }
        }
        for order in orderbook.orders_for_side(Side::Buy) {
            book.add_pending(order.trader_id, order.side, order.remaining);
        }
        for order in orderbook.orders_for_side(Side::Sell) {
            book.add_pending(order.trader_id, order.side, order.remaining);
        }
        book
    }

    fn ensure_submit_within_limit(
        &self,
        state: &AppState,
        trader_id: Uuid,
        market: &str,
        side: Side,
        quantity: u64,
    ) -> Result<(), SettlementError> {
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

        let exposure = self.traders.get(&trader_id).copied().unwrap_or_default();
        let requested = i64::try_from(quantity).map_err(|_| SettlementError::Overflow)?;
        let mut pending_buy_quantity = exposure.pending_buy_quantity;
        let mut pending_sell_quantity = exposure.pending_sell_quantity;
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

        let max_long = exposure
            .net_quantity
            .checked_add(pending_buy_quantity)
            .ok_or(SettlementError::Overflow)?;
        if max_long > crate::state::NET_POSITION_LIMIT {
            return Err(SettlementError::PositionLimitExceeded {
                market: market.to_string(),
                projected: max_long,
                limit: crate::state::NET_POSITION_LIMIT,
            });
        }

        let max_short = exposure
            .net_quantity
            .checked_sub(pending_sell_quantity)
            .ok_or(SettlementError::Overflow)?;
        if max_short < -crate::state::NET_POSITION_LIMIT {
            return Err(SettlementError::PositionLimitExceeded {
                market: market.to_string(),
                projected: max_short,
                limit: crate::state::NET_POSITION_LIMIT,
            });
        }

        Ok(())
    }

    fn apply_fill(
        &mut self,
        trader_id: Uuid,
        market: &str,
        side: Side,
        fill_price: u64,
        quantity: u64,
    ) -> Result<Option<Position>, SettlementError> {
        let exposure = self.exposure_mut(trader_id);
        let mut position = Position {
            market: market.to_string(),
            net_quantity: exposure.net_quantity,
            average_entry_price: exposure.average_entry_price,
            realized_pnl: exposure.realized_pnl,
            updated_at: Utc::now(),
        };
        apply_fill_to_position(&mut position, side, fill_price, quantity)?;
        position.updated_at = Utc::now();
        exposure.net_quantity = position.net_quantity;
        exposure.average_entry_price = position.average_entry_price;
        exposure.realized_pnl = position.realized_pnl;
        self.prune_if_empty(trader_id);
        Ok(should_persist_position(&position).then_some(position))
    }

    fn add_pending(&mut self, trader_id: Uuid, side: Side, quantity: u64) {
        if quantity == 0 {
            return;
        }
        let delta = i64::try_from(quantity).expect("pending quantity should fit in i64");
        let exposure = self.exposure_mut(trader_id);
        match side {
            Side::Buy => {
                exposure.pending_buy_quantity = exposure
                    .pending_buy_quantity
                    .checked_add(delta)
                    .expect("pending buy quantity overflow");
            }
            Side::Sell => {
                exposure.pending_sell_quantity = exposure
                    .pending_sell_quantity
                    .checked_add(delta)
                    .expect("pending sell quantity overflow");
            }
        }
    }

    fn remove_pending(
        &mut self,
        trader_id: Uuid,
        side: Side,
        quantity: u64,
    ) -> Result<(), SettlementError> {
        if quantity == 0 {
            return Ok(());
        }
        let delta = i64::try_from(quantity).map_err(|_| SettlementError::Overflow)?;
        let exposure = self.exposure_mut(trader_id);
        match side {
            Side::Buy => {
                exposure.pending_buy_quantity = exposure
                    .pending_buy_quantity
                    .checked_sub(delta)
                    .ok_or(SettlementError::Overflow)?;
            }
            Side::Sell => {
                exposure.pending_sell_quantity = exposure
                    .pending_sell_quantity
                    .checked_sub(delta)
                    .ok_or(SettlementError::Overflow)?;
            }
        }
        self.prune_if_empty(trader_id);
        Ok(())
    }

    fn exposure_mut(&mut self, trader_id: Uuid) -> &mut TraderMarketExposure {
        self.traders.entry(trader_id).or_default()
    }

    fn prune_if_empty(&mut self, trader_id: Uuid) {
        if self.traders.get(&trader_id).is_some_and(|exposure| {
            exposure.net_quantity == 0
                && exposure.average_entry_price.is_none()
                && exposure.realized_pnl == 0
                && exposure.pending_buy_quantity == 0
                && exposure.pending_sell_quantity == 0
        }) {
            self.traders.remove(&trader_id);
        }
    }
}

#[derive(Clone)]
pub struct MarketEngineHandle {
    tx: mpsc::Sender<MarketCommand>,
    barrier_telemetry: AccountBarrierTelemetry,
}

struct EngineCommandResult<T> {
    result: Result<T, TradingError>,
    account_barrier: Option<oneshot::Receiver<()>>,
}

impl MarketEngineHandle {
    pub fn spawn(state: AppState, market: String, orderbook: OrderBook) -> Self {
        let exposure_book = MarketExposureBook::recover(&state, &market, &orderbook);
        let barrier_telemetry = state.account_barrier_telemetry();
        let (tx, rx) = mpsc::channel(MARKET_ENGINE_COMMAND_BUFFER);
        let thread_name = format!("market-engine-{market}");
        thread::Builder::new()
            .name(thread_name)
            .spawn(move || market_engine_loop(state, market, orderbook, exposure_book, rx))
            .unwrap_or_else(|error| panic!("failed to spawn market engine thread: {error}"));
        Self {
            tx,
            barrier_telemetry,
        }
    }

    pub async fn submit_order(
        &self,
        trader_id: Uuid,
        request: SubmitOrderRequest,
    ) -> Result<SubmitOrderResponse, TradingError> {
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(MarketCommand::Submit {
                trader_id,
                request,
                respond_to,
            })
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        let response = response
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        wait_for_account_barrier(response, &self.barrier_telemetry, BarrierKind::Submit).await
    }

    pub async fn submit_limit_order(
        &self,
        trader_id: Uuid,
        request: SubmitOrderRequest,
    ) -> Result<SubmitOrderResponse, TradingError> {
        self.submit_order(trader_id, request).await
    }

    pub async fn cancel_order(
        &self,
        trader_id: Uuid,
        order_id: Uuid,
    ) -> Result<CancelOrderResponse, TradingError> {
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(MarketCommand::Cancel {
                trader_id,
                order_id,
                respond_to,
            })
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        let response = response
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        wait_for_account_barrier(response, &self.barrier_telemetry, BarrierKind::Cancel).await
    }

    pub async fn amend_order(
        &self,
        trader_id: Uuid,
        order_id: Uuid,
        request: AmendOrderRequest,
    ) -> Result<AmendOrderResponse, TradingError> {
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(MarketCommand::Amend {
                trader_id,
                order_id,
                request,
                respond_to,
            })
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        let response = response
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        wait_for_account_barrier(response, &self.barrier_telemetry, BarrierKind::Amend).await
    }

    pub async fn snapshot(&self) -> Result<MarketBookSnapshot, TradingError> {
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(MarketCommand::Snapshot { respond_to })
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        response.await.map_err(|_| TradingError::EngineUnavailable)
    }

    pub async fn best_prices(&self) -> Result<(Option<u64>, Option<u64>), TradingError> {
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(MarketCommand::BestPrices { respond_to })
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        response.await.map_err(|_| TradingError::EngineUnavailable)
    }
}

enum MarketCommand {
    Submit {
        trader_id: Uuid,
        request: SubmitOrderRequest,
        respond_to: oneshot::Sender<EngineCommandResult<SubmitOrderResponse>>,
    },
    Cancel {
        trader_id: Uuid,
        order_id: Uuid,
        respond_to: oneshot::Sender<EngineCommandResult<CancelOrderResponse>>,
    },
    Amend {
        trader_id: Uuid,
        order_id: Uuid,
        request: AmendOrderRequest,
        respond_to: oneshot::Sender<EngineCommandResult<AmendOrderResponse>>,
    },
    Snapshot {
        respond_to: oneshot::Sender<MarketBookSnapshot>,
    },
    BestPrices {
        respond_to: oneshot::Sender<(Option<u64>, Option<u64>)>,
    },
}

async fn wait_for_account_barrier<T>(
    response: EngineCommandResult<T>,
    telemetry: &AccountBarrierTelemetry,
    barrier: BarrierKind,
) -> Result<T, TradingError> {
    if let Some(account_barrier) = response.account_barrier {
        let waited_at = Instant::now();
        account_barrier
            .await
            .map_err(|_| TradingError::EngineUnavailable)?;
        telemetry.record(barrier, waited_at.elapsed());
    }
    response.result
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TradingError {
    #[error("trading is currently disabled")]
    TradingDisabled,
    #[error("invalid market symbol")]
    InvalidMarket,
    #[error("market is not configured")]
    MarketNotConfigured,
    #[error("market is disabled")]
    MarketDisabled,
    #[error("market has already been settled")]
    MarketSettled,
    #[error("price must be greater than zero")]
    InvalidPrice,
    #[error("price must be at most {maximum}")]
    PriceTooLarge { maximum: u64 },
    #[error("price must align to tick size {tick_size}")]
    TickSizeViolation { tick_size: u64 },
    #[error("price must be at least {minimum}")]
    PriceBelowMinimum { minimum: u64 },
    #[error("price must be at most {maximum}")]
    PriceAboveMaximum { maximum: u64 },
    #[error("market order could not be filled because no opposite-side liquidity is available")]
    NoLiquidity,
    #[error("quantity must be greater than zero")]
    InvalidQuantity,
    #[error("quantity must be at most {maximum}")]
    QuantityTooLarge { maximum: u64 },
    #[error("quantity must be at least {minimum}")]
    QuantityBelowMinimum { minimum: u64 },
    #[error("remaining quantity must be greater than zero")]
    InvalidRemaining,
    #[error("cannot increase remaining quantity")]
    InvalidAmend,
    #[error("order not found")]
    OrderNotFound,
    #[error("order does not belong to trader")]
    OrderNotOwned,
    #[error("market engine unavailable")]
    EngineUnavailable,
    #[error("projected net position for {market} would be {projected}; limit is +/-{limit}")]
    PositionLimitExceeded {
        market: String,
        projected: i64,
        limit: i64,
    },
    #[error("numeric overflow")]
    Overflow,
}

impl TradingError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            TradingError::TradingDisabled => StatusCode::CONFLICT,
            TradingError::InvalidMarket
            | TradingError::TickSizeViolation { .. }
            | TradingError::PriceBelowMinimum { .. }
            | TradingError::PriceAboveMaximum { .. }
            | TradingError::QuantityBelowMinimum { .. }
            | TradingError::NoLiquidity
            | TradingError::InvalidPrice
            | TradingError::PriceTooLarge { .. }
            | TradingError::InvalidQuantity
            | TradingError::QuantityTooLarge { .. }
            | TradingError::InvalidRemaining
            | TradingError::InvalidAmend => StatusCode::BAD_REQUEST,
            TradingError::MarketNotConfigured | TradingError::OrderNotFound => {
                StatusCode::NOT_FOUND
            }
            TradingError::OrderNotOwned => StatusCode::FORBIDDEN,
            TradingError::MarketDisabled
            | TradingError::MarketSettled
            | TradingError::PositionLimitExceeded { .. } => StatusCode::CONFLICT,
            TradingError::EngineUnavailable | TradingError::Overflow => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

impl From<SettlementError> for TradingError {
    fn from(value: SettlementError) -> Self {
        match value {
            SettlementError::InvalidMarket => Self::InvalidMarket,
            SettlementError::InvalidSettlementPrice => Self::InvalidPrice,
            SettlementError::PositionLimitExceeded {
                market,
                projected,
                limit,
            } => Self::PositionLimitExceeded {
                market,
                projected,
                limit,
            },
            SettlementError::Overflow => Self::Overflow,
        }
    }
}

pub struct TradingService;

impl TradingService {
    pub async fn submit_order(
        state: &AppState,
        trader_id: Uuid,
        request: SubmitOrderRequest,
    ) -> Result<SubmitOrderResponse, TradingError> {
        state.operator_telemetry().record_submit_attempt();
        let result = match validate_market_symbol(&request.market) {
            Ok(()) => match state.storage.get_market(&request.market) {
                Some(market) => {
                    let engine = state.ensure_market_engine(&market.market_id);
                    engine.submit_order(trader_id, request).await
                }
                None => Err(TradingError::MarketNotConfigured),
            },
            Err(error) => Err(error),
        };
        match &result {
            Ok(_) => state.operator_telemetry().record_submit_accept(),
            Err(_) => state.operator_telemetry().record_submit_reject(),
        }
        result
    }

    pub async fn submit_limit_order(
        state: &AppState,
        trader_id: Uuid,
        request: SubmitOrderRequest,
    ) -> Result<SubmitOrderResponse, TradingError> {
        Self::submit_order(state, trader_id, request).await
    }

    pub async fn cancel_order(
        state: &AppState,
        trader_id: Uuid,
        order_id: Uuid,
    ) -> Result<CancelOrderResponse, TradingError> {
        state.operator_telemetry().record_cancel_attempt();
        let result = match find_order_market(state, trader_id, order_id) {
            Ok(market) => {
                let engine = state.ensure_market_engine(&market);
                engine.cancel_order(trader_id, order_id).await
            }
            Err(error) => Err(error),
        };
        match &result {
            Ok(_) => state.operator_telemetry().record_cancel_accept(),
            Err(_) => state.operator_telemetry().record_cancel_reject(),
        }
        result
    }

    pub async fn amend_order(
        state: &AppState,
        trader_id: Uuid,
        order_id: Uuid,
        request: AmendOrderRequest,
    ) -> Result<AmendOrderResponse, TradingError> {
        state.operator_telemetry().record_amend_attempt();
        if request.remaining == 0 {
            state.operator_telemetry().record_amend_reject();
            return Err(TradingError::InvalidRemaining);
        }

        let result = match find_order_market(state, trader_id, order_id) {
            Ok(market) => {
                let engine = state.ensure_market_engine(&market);
                engine.amend_order(trader_id, order_id, request).await
            }
            Err(error) => Err(error),
        };
        match &result {
            Ok(_) => state.operator_telemetry().record_amend_accept(),
            Err(_) => state.operator_telemetry().record_amend_reject(),
        }
        result
    }
}

fn validate_submit_market(
    state: &AppState,
    request: &SubmitOrderRequest,
) -> Result<MarketDefinition, TradingError> {
    let market = ensure_market_allows_entry(state, &request.market)?;
    if request.order_type == OrderType::Limit {
        if request.price > MAX_PERSISTABLE_U64 {
            return Err(TradingError::PriceTooLarge {
                maximum: MAX_PERSISTABLE_U64,
            });
        }
        if request.price % market.tick_size != 0 {
            return Err(TradingError::TickSizeViolation {
                tick_size: market.tick_size,
            });
        }
        if let Some(minimum) = market.min_price {
            if request.price < minimum {
                return Err(TradingError::PriceBelowMinimum { minimum });
            }
        }
        if let Some(maximum) = market.max_price {
            if request.price > maximum {
                return Err(TradingError::PriceAboveMaximum { maximum });
            }
        }
    }
    if request.quantity == 0 {
        return Err(TradingError::InvalidQuantity);
    }
    if request.quantity > MAX_PERSISTABLE_U64 {
        return Err(TradingError::QuantityTooLarge {
            maximum: MAX_PERSISTABLE_U64,
        });
    }
    if request.quantity < market.min_order_quantity {
        return Err(TradingError::QuantityBelowMinimum {
            minimum: market.min_order_quantity,
        });
    }
    Ok(market)
}

fn ensure_market_allows_entry(
    state: &AppState,
    market: &str,
) -> Result<MarketDefinition, TradingError> {
    validate_market_symbol(market)?;
    if !state.storage.get_exchange_controls().trading_enabled {
        return Err(TradingError::TradingDisabled);
    }
    let market = state
        .storage
        .get_market(market)
        .ok_or(TradingError::MarketNotConfigured)?;
    match market.status {
        MarketStatus::Enabled => Ok(market),
        MarketStatus::Disabled => Err(TradingError::MarketDisabled),
        MarketStatus::Settled => Err(TradingError::MarketSettled),
    }
}

fn validate_market_symbol(market: &str) -> Result<(), TradingError> {
    let Some((base, quote)) = market.split_once('-') else {
        return Err(TradingError::InvalidMarket);
    };
    if base.is_empty() || quote.is_empty() {
        return Err(TradingError::InvalidMarket);
    }
    Ok(())
}

fn find_order_market(
    state: &AppState,
    trader_id: Uuid,
    order_id: Uuid,
) -> Result<String, TradingError> {
    state
        .storage
        .get_open_order(trader_id, order_id)
        .as_ref()
        .map(|order| order.market.clone())
        .ok_or(TradingError::OrderNotFound)
}

fn sync_open_order(
    state: &AppState,
    trader_id: Uuid,
    order_id: Uuid,
    order: Option<Order>,
    closed_remaining: Option<u64>,
) {
    if let Some(incoming) = order {
        state.queue_upsert_open_order(trader_id, incoming);
        return;
    }

    if let Some(remaining) = closed_remaining {
        state.close_order_ledger(trader_id, order_id, remaining);
    }
    state.queue_delete_open_order(trader_id, order_id);
}

fn sync_position(state: &AppState, trader_id: Uuid, market: &str, position: Option<Position>) {
    if let Some(position) = position {
        state.queue_upsert_position(trader_id, position);
    } else {
        state.queue_delete_position(trader_id, market.to_string());
    }
}

fn publish_market_event(state: &AppState, market: &str, event: MarketEvent) {
    state.dispatch_market_event(market, event);
}

fn publish_user_event(state: &AppState, trader_id: Uuid, message: ServerMessage) {
    state.dispatch_user_event(trader_id, message);
}

fn publish_order_added(state: &AppState, order: &Order) {
    publish_market_event(
        state,
        &order.market,
        MarketEvent::OrderAdded {
            order_id: order.id,
            side: order.side,
            price: order.price,
            remaining: order.remaining,
            created_at: order.created_at,
        },
    );
}

fn publish_order_updated(state: &AppState, order: &Order) {
    publish_market_event(
        state,
        &order.market,
        MarketEvent::OrderUpdated {
            order_id: order.id,
            side: order.side,
            price: order.price,
            remaining: order.remaining,
        },
    );
}

fn publish_order_removed(
    state: &AppState,
    market: &str,
    order_id: Uuid,
    side: Side,
    price: u64,
    reason: MarketEventRemoveReason,
) {
    publish_market_event(
        state,
        market,
        MarketEvent::OrderRemoved {
            order_id,
            side,
            price,
            reason,
        },
    );
}

fn publish_trade_event(state: &AppState, fill: &Fill, taker_side: Side) {
    publish_market_event(
        state,
        &fill.market,
        MarketEvent::Trade {
            maker_order_id: fill.maker_order_id,
            taker_order_id: fill.taker_order_id,
            taker_side,
            price: fill.price,
            quantity: fill.quantity,
        },
    );
}

fn publish_user_order_state(
    state: &AppState,
    trader_id: Uuid,
    order: Order,
    status: OrderStateStatus,
) {
    publish_user_event(
        state,
        trader_id,
        ServerMessage::OrderState { order, status },
    );
}

fn maker_order_state_from_execution(market: &str, execution: &MatchExecution) -> Order {
    Order {
        id: execution.maker_order_id,
        trader_id: execution.maker_trader_id,
        market: market.to_string(),
        side: execution.maker_side,
        price: execution.maker_limit_price,
        quantity: execution.maker_order_quantity,
        remaining: 0,
        created_at: execution.maker_created_at,
    }
}

fn market_engine_loop(
    state: AppState,
    _market: String,
    mut orderbook: OrderBook,
    mut exposure_book: MarketExposureBook,
    mut rx: mpsc::Receiver<MarketCommand>,
) {
    while let Some(command) = rx.blocking_recv() {
        match command {
            MarketCommand::Submit {
                trader_id,
                request,
                respond_to,
            } => {
                let _ = respond_to.send(process_submit_order(
                    &state,
                    &mut orderbook,
                    &mut exposure_book,
                    trader_id,
                    request,
                ));
            }
            MarketCommand::Cancel {
                trader_id,
                order_id,
                respond_to,
            } => {
                let _ = respond_to.send(process_cancel_order(
                    &state,
                    &mut orderbook,
                    &mut exposure_book,
                    trader_id,
                    order_id,
                ));
            }
            MarketCommand::Amend {
                trader_id,
                order_id,
                request,
                respond_to,
            } => {
                let _ = respond_to.send(process_amend_order(
                    &state,
                    &mut orderbook,
                    &mut exposure_book,
                    trader_id,
                    order_id,
                    request,
                ));
            }
            MarketCommand::Snapshot { respond_to } => {
                let _ = respond_to.send(MarketBookSnapshot {
                    bids: orderbook.levels_for_side(Side::Buy),
                    asks: orderbook.levels_for_side(Side::Sell),
                });
            }
            MarketCommand::BestPrices { respond_to } => {
                let _ = respond_to.send((orderbook.best_bid_price(), orderbook.best_ask_price()));
            }
        }
    }
}

fn process_submit_order(
    state: &AppState,
    orderbook: &mut OrderBook,
    exposure_book: &mut MarketExposureBook,
    trader_id: Uuid,
    request: SubmitOrderRequest,
) -> EngineCommandResult<SubmitOrderResponse> {
    let result = (|| -> Result<SubmitOrderResponse, TradingError> {
        let market = validate_submit_market(state, &request)?;
        if request.order_type == OrderType::Limit && request.price == 0 {
            return Err(TradingError::InvalidPrice);
        }
        if request.quantity == 0 {
            return Err(TradingError::InvalidQuantity);
        }

        let order = Order {
            id: Uuid::new_v4(),
            trader_id,
            market: market.market_id.clone(),
            side: request.side,
            price: request.price,
            quantity: request.quantity,
            remaining: request.quantity,
            created_at: Utc::now(),
        };

        exposure_book.ensure_submit_within_limit(
            state,
            trader_id,
            &order.market,
            order.side,
            order.quantity,
        )?;
        state.persist_order_ledger(order.clone());

        let executions = match request.order_type {
            OrderType::Limit => {
                MatchingEngine::process_limit_order_executions(orderbook, order.clone())
            }
            OrderType::Market => {
                let executions = MatchingEngine::process_market_order_executions(
                    orderbook,
                    order.side,
                    order.quantity,
                );
                if executions.is_empty() {
                    return Err(TradingError::NoLiquidity);
                }
                executions
            }
        };
        let maker_ids: HashSet<Uuid> = executions
            .iter()
            .map(|execution| execution.maker_order_id)
            .collect();
        let maker_orders = maker_ids
            .into_iter()
            .map(|order_id| (order_id, orderbook.get_order(order_id).cloned()))
            .collect::<HashMap<_, _>>();
        let resting_order = if request.order_type == OrderType::Limit {
            orderbook.get_order(order.id).cloned()
        } else {
            None
        };

        let occurred_at = Utc::now();
        let fills = executions
            .iter()
            .map(|execution| Fill {
                fill_id: Uuid::new_v4(),
                market: order.market.clone(),
                maker_order_id: execution.maker_order_id,
                taker_order_id: order.id,
                price: execution.price,
                quantity: execution.quantity,
                occurred_at,
            })
            .collect::<Vec<_>>();
        for fill in &fills {
            state.operator_telemetry().record_fill(fill.quantity);
        }

        let mut latest_positions = HashMap::new();
        for execution in &executions {
            if execution.maker_trader_id == trader_id {
                // Self-trade: consuming your own resting liquidity should not change inventory or PnL.
                // We still remove pending maker exposure below so position limit accounting remains consistent.
                exposure_book.remove_pending(
                    execution.maker_trader_id,
                    execution.maker_side,
                    execution.quantity,
                )?;
                continue;
            }
            let taker_position = exposure_book.apply_fill(
                trader_id,
                &order.market,
                order.side,
                execution.price,
                execution.quantity,
            )?;
            latest_positions.insert(trader_id, taker_position);

            let maker_position = exposure_book.apply_fill(
                execution.maker_trader_id,
                &order.market,
                execution.maker_side,
                execution.price,
                execution.quantity,
            )?;
            latest_positions.insert(execution.maker_trader_id, maker_position);
            exposure_book.remove_pending(
                execution.maker_trader_id,
                execution.maker_side,
                execution.quantity,
            )?;
        }

        for (trader_id, position) in latest_positions {
            sync_position(state, trader_id, &order.market, position);
        }

        for (execution, fill) in executions.iter().zip(fills.iter()) {
            state.persist_fill(fill.clone());
            state.queue_append_fill(trader_id, fill.clone());
            if execution.maker_trader_id != trader_id {
                state.queue_append_fill(execution.maker_trader_id, fill.clone());
            }
            publish_user_event(state, trader_id, ServerMessage::Fill { fill: fill.clone() });
            if execution.maker_trader_id != trader_id {
                publish_user_event(
                    state,
                    execution.maker_trader_id,
                    ServerMessage::Fill { fill: fill.clone() },
                );
            }

            let maker_resting_state = maker_orders
                .get(&execution.maker_order_id)
                .cloned()
                .flatten();
            if let Some(maker_resting_state) = maker_resting_state.as_ref() {
                publish_order_updated(state, maker_resting_state);
            } else {
                publish_order_removed(
                    state,
                    &order.market,
                    execution.maker_order_id,
                    execution.maker_side,
                    execution.maker_limit_price,
                    MarketEventRemoveReason::Filled,
                );
            }
            publish_trade_event(state, fill, order.side);
        }

        for execution in &executions {
            let maker_resting_state = maker_orders
                .get(&execution.maker_order_id)
                .cloned()
                .flatten();
            let maker_order_state = maker_resting_state
                .clone()
                .unwrap_or_else(|| maker_order_state_from_execution(&order.market, execution));
            sync_open_order(
                state,
                execution.maker_trader_id,
                execution.maker_order_id,
                maker_resting_state.clone(),
                Some(0),
            );
            publish_user_order_state(
                state,
                execution.maker_trader_id,
                maker_order_state,
                if maker_resting_state.is_some() {
                    OrderStateStatus::Open
                } else {
                    OrderStateStatus::Filled
                },
            );
        }

        sync_open_order(state, trader_id, order.id, resting_order.clone(), Some(0));
        if let Some(resting_order) = resting_order.as_ref() {
            exposure_book.add_pending(
                resting_order.trader_id,
                resting_order.side,
                resting_order.remaining,
            );
            publish_order_added(state, resting_order);
        }
        let completed_order = resting_order.clone().unwrap_or(Order {
            remaining: 0,
            price: resolved_order_price(request.order_type, order.price, &fills),
            ..order.clone()
        });
        publish_user_order_state(
            state,
            trader_id,
            completed_order.clone(),
            if resting_order.is_some() {
                OrderStateStatus::Open
            } else {
                OrderStateStatus::Filled
            },
        );

        let resting = resting_order.is_some();
        Ok(SubmitOrderResponse {
            order: resting_order.unwrap_or(completed_order),
            fills,
            resting,
        })
    })();

    EngineCommandResult {
        account_barrier: result
            .as_ref()
            .ok()
            .map(|_| state.account_dispatch_barrier()),
        result,
    }
}

fn resolved_order_price(order_type: OrderType, requested_price: u64, fills: &[Fill]) -> u64 {
    if order_type == OrderType::Limit {
        return requested_price;
    }

    weighted_fill_price(fills).unwrap_or(requested_price)
}

fn weighted_fill_price(fills: &[Fill]) -> Option<u64> {
    let total_quantity = fills.iter().map(|fill| fill.quantity as u128).sum::<u128>();
    if total_quantity == 0 {
        return None;
    }

    let weighted_sum = fills
        .iter()
        .map(|fill| fill.price as u128 * fill.quantity as u128)
        .sum::<u128>();
    let rounded = (weighted_sum + total_quantity / 2) / total_quantity;
    u64::try_from(rounded).ok()
}

fn process_cancel_order(
    state: &AppState,
    orderbook: &mut OrderBook,
    exposure_book: &mut MarketExposureBook,
    trader_id: Uuid,
    order_id: Uuid,
) -> EngineCommandResult<CancelOrderResponse> {
    let result = (|| -> Result<CancelOrderResponse, TradingError> {
        let Some(order) = orderbook.get_order(order_id).cloned() else {
            return Err(TradingError::OrderNotFound);
        };
        if order.trader_id != trader_id {
            return Err(TradingError::OrderNotOwned);
        }
        let removed = orderbook
            .cancel_order(order_id)
            .expect("book order should still cancel after lookup");

        exposure_book.remove_pending(trader_id, removed.side, removed.remaining)?;
        sync_open_order(state, trader_id, order_id, None, Some(removed.remaining));
        publish_user_order_state(
            state,
            trader_id,
            removed.clone(),
            OrderStateStatus::Canceled,
        );
        publish_order_removed(
            state,
            &removed.market,
            removed.id,
            removed.side,
            removed.price,
            MarketEventRemoveReason::Canceled,
        );

        Ok(CancelOrderResponse { order: removed })
    })();

    EngineCommandResult {
        account_barrier: result
            .as_ref()
            .ok()
            .map(|_| state.account_dispatch_barrier()),
        result,
    }
}

fn process_amend_order(
    state: &AppState,
    orderbook: &mut OrderBook,
    exposure_book: &mut MarketExposureBook,
    trader_id: Uuid,
    order_id: Uuid,
    request: AmendOrderRequest,
) -> EngineCommandResult<AmendOrderResponse> {
    let result = (|| -> Result<AmendOrderResponse, TradingError> {
        if request.remaining == 0 {
            return Err(TradingError::InvalidRemaining);
        }

        let market = find_order_market(state, trader_id, order_id)?;
        ensure_market_allows_entry(state, &market)?;
        let Some(before) = orderbook.get_order(order_id).cloned() else {
            return Err(TradingError::OrderNotFound);
        };
        if before.trader_id != trader_id {
            return Err(TradingError::OrderNotOwned);
        }
        if request.remaining > before.remaining {
            return Err(TradingError::InvalidAmend);
        }
        orderbook
            .amend_order_remaining(order_id, request.remaining)
            .expect("amend should succeed after validation");
        let after = orderbook
            .get_order(order_id)
            .cloned()
            .expect("order should remain after non-zero amend");

        exposure_book.remove_pending(trader_id, before.side, before.remaining - after.remaining)?;
        sync_open_order(state, trader_id, order_id, Some(after.clone()), None);
        publish_user_order_state(state, trader_id, after.clone(), OrderStateStatus::Open);
        publish_order_updated(state, &after);

        Ok(AmendOrderResponse { order: after })
    })();

    EngineCommandResult {
        account_barrier: result
            .as_ref()
            .ok()
            .map(|_| state.account_dispatch_barrier()),
        result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{MarketDefinition, MarketStatus};
    use crate::config::Config;
    use crate::marketdata::{BookDelta, MarketEvent, MarketEventRemoveReason};
    use crate::state::Position;
    use chrono::Utc;

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
        seed_market(&state, "BTC-USD", "BTC", "USD");
        seed_market(&state, "ETH-USD", "ETH", "USD");
        state
    }

    fn seed_market(state: &AppState, market_id: &str, base_asset: &str, quote_asset: &str) {
        let now = Utc::now();
        state.storage.upsert_market(MarketDefinition {
            market_id: market_id.to_string(),
            display_name: market_id.to_string(),
            base_asset: base_asset.to_string(),
            quote_asset: quote_asset.to_string(),
            tick_size: 1,
            min_order_quantity: 1,
            min_price: None,
            max_price: None,
            reference_price: None,
            settlement_price: None,
            status: MarketStatus::Enabled,
            created_at: now,
            updated_at: now,
        });
    }

    fn position_for(state: &AppState, trader_id: Uuid, market: &str) -> PositionSnapshot {
        let position = state
            .storage
            .list_positions(trader_id)
            .into_iter()
            .find(|position| position.market == market)
            .unwrap_or(Position {
                market: market.to_string(),
                net_quantity: 0,
                average_entry_price: None,
                realized_pnl: 0,
                updated_at: Utc::now(),
            });
        PositionSnapshot {
            net_quantity: position.net_quantity,
            average_entry_price: position.average_entry_price,
            realized_pnl: position.realized_pnl,
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct PositionSnapshot {
        net_quantity: i64,
        average_entry_price: Option<u64>,
        realized_pnl: i64,
    }

    #[tokio::test]
    async fn submit_resting_buy_order_tracks_open_order_without_changing_position() {
        let state = test_state();
        let trader_id = Uuid::new_v4();

        let response = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 3,
            },
        )
        .await
        .expect("submit should succeed");

        assert!(response.resting);
        assert!(response.fills.is_empty());
        assert_eq!(state.storage.list_positions(trader_id).len(), 0);
        let orders = state.storage.list_open_orders(trader_id, None);
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].remaining, 3);
    }

    #[tokio::test]
    async fn submit_crossing_order_updates_positions_and_records_fills() {
        let state = test_state();
        let maker_id = Uuid::new_v4();
        let taker_id = Uuid::new_v4();

        TradingService::submit_limit_order(
            &state,
            maker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 2,
            },
        )
        .await
        .expect("maker order should rest");

        let response = TradingService::submit_limit_order(
            &state,
            taker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 105,
                quantity: 2,
            },
        )
        .await
        .expect("taker order should match");

        assert!(!response.resting);
        assert_eq!(response.fills.len(), 1);
        assert_eq!(response.fills[0].price, 100);
        assert_eq!(
            position_for(&state, maker_id, "BTC-USD"),
            PositionSnapshot {
                net_quantity: -2,
                average_entry_price: Some(100),
                realized_pnl: 0,
            }
        );
        assert_eq!(
            position_for(&state, taker_id, "BTC-USD"),
            PositionSnapshot {
                net_quantity: 2,
                average_entry_price: Some(100),
                realized_pnl: 0,
            }
        );
        assert_eq!(state.storage.list_open_orders(maker_id, None).len(), 0);
        assert_eq!(state.storage.list_fills(maker_id, None).len(), 1);
        assert_eq!(state.storage.list_fills(taker_id, None).len(), 1);
    }

    #[tokio::test]
    async fn self_cross_does_not_change_position_or_double_record_fills() {
        let state = test_state();
        let trader_id = Uuid::new_v4();

        TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 2,
            },
        )
        .await
        .expect("maker order should rest");

        let before = position_for(&state, trader_id, "BTC-USD");
        let response = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 105,
                quantity: 2,
            },
        )
        .await
        .expect("self taker order should match");
        let after = position_for(&state, trader_id, "BTC-USD");

        assert!(!response.resting);
        assert_eq!(response.fills.len(), 1);
        assert_eq!(after, before);
        assert_eq!(state.storage.list_open_orders(trader_id, None).len(), 0);
        assert_eq!(state.storage.list_fills(trader_id, None).len(), 1);
    }

    #[tokio::test]
    async fn submit_market_order_matches_without_resting() {
        let state = test_state();
        let maker_id = Uuid::new_v4();
        let taker_id = Uuid::new_v4();

        TradingService::submit_limit_order(
            &state,
            maker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 2,
            },
        )
        .await
        .expect("maker order should rest");

        let response = TradingService::submit_order(
            &state,
            taker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Market,
                price: 0,
                quantity: 2,
            },
        )
        .await
        .expect("market order should match");

        assert!(!response.resting);
        assert_eq!(response.order.remaining, 0);
        assert_eq!(response.order.price, 100);
        assert_eq!(response.fills.len(), 1);
        assert_eq!(response.fills[0].price, 100);
        assert_eq!(state.storage.list_open_orders(taker_id, None).len(), 0);
    }

    #[tokio::test]
    async fn submit_market_order_requires_opposite_side_liquidity() {
        let state = test_state();
        let trader_id = Uuid::new_v4();

        let error = TradingService::submit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Market,
                price: 0,
                quantity: 2,
            },
        )
        .await
        .expect_err("market order should fail without liquidity");

        assert_eq!(error, TradingError::NoLiquidity);
    }

    #[tokio::test]
    async fn submit_limit_order_rejects_prices_outside_market_bounds() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        let now = Utc::now();
        state.storage.upsert_market(MarketDefinition {
            market_id: "BOUNDED-USD".to_string(),
            display_name: "Bounded".to_string(),
            base_asset: "BOUNDED".to_string(),
            quote_asset: "USD".to_string(),
            tick_size: 5,
            min_order_quantity: 1,
            min_price: Some(50),
            max_price: Some(150),
            reference_price: Some(100),
            settlement_price: None,
            status: MarketStatus::Enabled,
            created_at: now,
            updated_at: now,
        });

        let below_minimum = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BOUNDED-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 45,
                quantity: 1,
            },
        )
        .await
        .expect_err("order below minimum price should fail");
        assert_eq!(
            below_minimum,
            TradingError::PriceBelowMinimum { minimum: 50 }
        );

        let above_maximum = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BOUNDED-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 155,
                quantity: 1,
            },
        )
        .await
        .expect_err("order above maximum price should fail");
        assert_eq!(
            above_maximum,
            TradingError::PriceAboveMaximum { maximum: 150 }
        );
    }

    #[tokio::test]
    async fn recovered_open_orders_participate_in_matching_after_restart() {
        let storage = crate::storage::StorageRepository::new_in_memory();
        let maker_id = Uuid::new_v4();
        let taker_id = Uuid::new_v4();
        let maker_order = Order {
            id: Uuid::new_v4(),
            trader_id: maker_id,
            market: "BTC-USD".to_string(),
            side: Side::Sell,
            price: 100,
            quantity: 2,
            remaining: 2,
            created_at: Utc::now(),
        };

        storage.upsert_order_ledger(maker_order.clone());
        storage.upsert_open_order(maker_id, maker_order.clone());
        let now = Utc::now();
        storage.upsert_market(MarketDefinition {
            market_id: "BTC-USD".to_string(),
            display_name: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USD".to_string(),
            tick_size: 1,
            min_order_quantity: 1,
            min_price: None,
            max_price: None,
            reference_price: None,
            settlement_price: None,
            status: MarketStatus::Enabled,
            created_at: now,
            updated_at: now,
        });

        let state = AppState::with_storage(
            Config {
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
            },
            storage,
        );

        let response = TradingService::submit_limit_order(
            &state,
            taker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 2,
            },
        )
        .await
        .expect("submit should succeed");

        assert_eq!(response.fills.len(), 1);
        assert!(!response.resting);
        assert_eq!(state.storage.list_open_orders(maker_id, None).len(), 0);
        assert_eq!(
            position_for(&state, maker_id, "BTC-USD"),
            PositionSnapshot {
                net_quantity: -2,
                average_entry_price: Some(100),
                realized_pnl: 0,
            }
        );
        assert_eq!(
            position_for(&state, taker_id, "BTC-USD"),
            PositionSnapshot {
                net_quantity: 2,
                average_entry_price: Some(100),
                realized_pnl: 0,
            }
        );
    }

    #[tokio::test]
    async fn cancel_clears_resting_order_without_touching_positions() {
        let state = test_state();
        let trader_id = Uuid::new_v4();

        let response = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 3,
            },
        )
        .await
        .expect("submit should succeed");

        TradingService::cancel_order(&state, trader_id, response.order.id)
            .await
            .expect("cancel should succeed");

        assert!(state.storage.list_positions(trader_id).is_empty());
        assert_eq!(state.storage.list_open_orders(trader_id, None).len(), 0);
    }

    #[tokio::test]
    async fn amend_down_reduces_resting_quantity() {
        let state = test_state();
        let trader_id = Uuid::new_v4();

        let response = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 5,
            },
        )
        .await
        .expect("submit should succeed");

        let amended = TradingService::amend_order(
            &state,
            trader_id,
            response.order.id,
            AmendOrderRequest { remaining: 2 },
        )
        .await
        .expect("amend should succeed");

        assert_eq!(amended.order.remaining, 2);
        let open_orders = state.storage.list_open_orders(trader_id, Some("BTC-USD"));
        assert_eq!(open_orders.len(), 1);
        assert_eq!(open_orders[0].remaining, 2);
    }

    #[tokio::test]
    async fn exposure_tracking_stays_correct_after_fill_and_cancel() {
        let state = test_state();
        let buyer_id = Uuid::new_v4();
        let seller_id = Uuid::new_v4();

        crate::settlement::SettlementEngine::seed_position(
            &state,
            buyer_id,
            "BTC-USD",
            900,
            Some(100),
            0,
        );

        let resting_buy = TradingService::submit_limit_order(
            &state,
            buyer_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 100,
            },
        )
        .await
        .expect("buy order should rest at the limit");

        let partial_sell = TradingService::submit_limit_order(
            &state,
            seller_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 40,
            },
        )
        .await
        .expect("partial fill should succeed");
        assert!(!partial_sell.resting);

        let error = TradingService::submit_limit_order(
            &state,
            buyer_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 99,
                quantity: 1,
            },
        )
        .await
        .expect_err("remaining headroom should be exhausted");
        assert!(matches!(
            error,
            TradingError::PositionLimitExceeded {
                projected: 1_001,
                ..
            }
        ));

        TradingService::cancel_order(&state, buyer_id, resting_buy.order.id)
            .await
            .expect("cancel should release pending exposure");

        let replacement = TradingService::submit_limit_order(
            &state,
            buyer_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 99,
                quantity: 60,
            },
        )
        .await
        .expect("replacement order should fit exactly after cancel");

        assert!(replacement.resting);
        assert_eq!(replacement.order.remaining, 60);
        assert_eq!(
            position_for(&state, buyer_id, "BTC-USD"),
            PositionSnapshot {
                net_quantity: 940,
                average_entry_price: Some(100),
                realized_pnl: 0,
            }
        );
    }

    #[tokio::test]
    async fn submit_amend_cancel_publish_snapshot_delta_events_in_sequence() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        let mut rx = state.events_tx.subscribe();

        let submitted = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 5,
            },
        )
        .await
        .expect("submit should succeed");
        let add_event = rx.recv().await.expect("add event");
        assert_eq!(add_event.market, "BTC-USD");
        assert_eq!(add_event.start_sequence, 1);
        assert_eq!(add_event.sequence, 1);
        assert_eq!(add_event.events.len(), 1);
        match &add_event.events[0] {
            BookDelta::LevelUpdated {
                side,
                price,
                quantity,
            } => {
                assert_eq!(*side, Side::Buy);
                assert_eq!(*price, 100);
                assert_eq!(*quantity, 5);
            }
            other => panic!("unexpected add event: {other:?}"),
        }

        TradingService::amend_order(
            &state,
            trader_id,
            submitted.order.id,
            AmendOrderRequest { remaining: 2 },
        )
        .await
        .expect("amend should succeed");
        let update_event = rx.recv().await.expect("update event");
        assert_eq!(update_event.start_sequence, 2);
        assert_eq!(update_event.sequence, 2);
        assert_eq!(update_event.events.len(), 1);
        match &update_event.events[0] {
            BookDelta::LevelUpdated {
                side,
                price,
                quantity,
            } => {
                assert_eq!(*side, Side::Buy);
                assert_eq!(*price, 100);
                assert_eq!(*quantity, 2);
            }
            other => panic!("unexpected update event: {other:?}"),
        }

        TradingService::cancel_order(&state, trader_id, submitted.order.id)
            .await
            .expect("cancel should succeed");
        let remove_event = rx.recv().await.expect("remove event");
        assert_eq!(remove_event.start_sequence, 3);
        assert_eq!(remove_event.sequence, 3);
        assert_eq!(remove_event.events.len(), 1);
        match &remove_event.events[0] {
            BookDelta::LevelUpdated {
                side,
                price,
                quantity,
            } => {
                assert_eq!(*side, Side::Buy);
                assert_eq!(*price, 100);
                assert_eq!(*quantity, 0);
            }
            other => panic!("unexpected remove event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn submit_amend_cancel_publish_canonical_market_events_in_sequence() {
        let state = test_state();
        let trader_id = Uuid::new_v4();
        let mut rx = state.market_event_tx.subscribe();

        let submitted = TradingService::submit_limit_order(
            &state,
            trader_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 5,
            },
        )
        .await
        .expect("submit should succeed");

        let add_event = rx.recv().await.expect("add event");
        assert_eq!(add_event.market, "BTC-USD");
        assert_eq!(add_event.sequence, 1);
        match add_event.event {
            MarketEvent::OrderAdded {
                order_id,
                side,
                price,
                remaining,
                ..
            } => {
                assert_eq!(order_id, submitted.order.id);
                assert_eq!(side, Side::Buy);
                assert_eq!(price, 100);
                assert_eq!(remaining, 5);
            }
            other => panic!("unexpected add market event: {other:?}"),
        }

        TradingService::amend_order(
            &state,
            trader_id,
            submitted.order.id,
            AmendOrderRequest { remaining: 2 },
        )
        .await
        .expect("amend should succeed");

        let update_event = rx.recv().await.expect("update event");
        assert_eq!(update_event.sequence, 2);
        match update_event.event {
            MarketEvent::OrderUpdated {
                order_id,
                side,
                price,
                remaining,
            } => {
                assert_eq!(order_id, submitted.order.id);
                assert_eq!(side, Side::Buy);
                assert_eq!(price, 100);
                assert_eq!(remaining, 2);
            }
            other => panic!("unexpected update market event: {other:?}"),
        }

        TradingService::cancel_order(&state, trader_id, submitted.order.id)
            .await
            .expect("cancel should succeed");

        let remove_event = rx.recv().await.expect("remove event");
        assert_eq!(remove_event.sequence, 3);
        match remove_event.event {
            MarketEvent::OrderRemoved {
                order_id,
                side,
                price,
                reason,
            } => {
                assert_eq!(order_id, submitted.order.id);
                assert_eq!(side, Side::Buy);
                assert_eq!(price, 100);
                assert_eq!(reason, MarketEventRemoveReason::Canceled);
            }
            other => panic!("unexpected remove market event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn partial_match_publishes_order_update_then_trade_market_events() {
        let state = test_state();
        let maker_id = Uuid::new_v4();
        let taker_id = Uuid::new_v4();
        let mut rx = state.market_event_tx.subscribe();

        let maker_submit = TradingService::submit_limit_order(
            &state,
            maker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 5,
            },
        )
        .await
        .expect("maker submit should succeed");

        let maker_add_event = rx.recv().await.expect("maker add event");
        assert_eq!(maker_add_event.sequence, 1);
        match maker_add_event.event {
            MarketEvent::OrderAdded {
                order_id,
                side,
                price,
                remaining,
                ..
            } => {
                assert_eq!(order_id, maker_submit.order.id);
                assert_eq!(side, Side::Sell);
                assert_eq!(price, 100);
                assert_eq!(remaining, 5);
            }
            other => panic!("unexpected maker add market event: {other:?}"),
        }

        let taker_submit = TradingService::submit_limit_order(
            &state,
            taker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 101,
                quantity: 2,
            },
        )
        .await
        .expect("taker submit should succeed");
        assert!(!taker_submit.resting);

        let maker_update_event = rx.recv().await.expect("maker update event");
        assert_eq!(maker_update_event.sequence, 2);
        match maker_update_event.event {
            MarketEvent::OrderUpdated {
                order_id,
                side,
                price,
                remaining,
            } => {
                assert_eq!(order_id, maker_submit.order.id);
                assert_eq!(side, Side::Sell);
                assert_eq!(price, 100);
                assert_eq!(remaining, 3);
            }
            other => panic!("unexpected maker update market event: {other:?}"),
        }

        let trade_event = rx.recv().await.expect("trade event");
        assert_eq!(trade_event.sequence, 3);
        match trade_event.event {
            MarketEvent::Trade {
                maker_order_id,
                taker_order_id,
                taker_side,
                price,
                quantity,
            } => {
                assert_eq!(maker_order_id, maker_submit.order.id);
                assert_eq!(taker_order_id, taker_submit.order.id);
                assert_eq!(taker_side, Side::Buy);
                assert_eq!(price, 100);
                assert_eq!(quantity, 2);
            }
            other => panic!("unexpected trade market event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn full_match_publishes_order_removed_then_trade_delta() {
        let state = test_state();
        let maker_id = Uuid::new_v4();
        let taker_id = Uuid::new_v4();
        let mut rx = state.events_tx.subscribe();

        let _maker_submit = TradingService::submit_limit_order(
            &state,
            maker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: 100,
                quantity: 2,
            },
        )
        .await
        .expect("maker submit should succeed");
        let _ = rx.recv().await.expect("maker add event");

        let taker_submit = TradingService::submit_limit_order(
            &state,
            taker_id,
            SubmitOrderRequest {
                market: "BTC-USD".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: 101,
                quantity: 2,
            },
        )
        .await
        .expect("taker submit should succeed");
        assert!(!taker_submit.resting);

        let remove_event = rx.recv().await.expect("remove event");
        assert_eq!(remove_event.start_sequence, 2);
        assert_eq!(remove_event.sequence, 3);
        assert_eq!(remove_event.events.len(), 2);
        match &remove_event.events[0] {
            BookDelta::LevelUpdated {
                side,
                price,
                quantity,
            } => {
                assert_eq!(*side, Side::Sell);
                assert_eq!(*price, 100);
                assert_eq!(*quantity, 0);
            }
            other => panic!("unexpected remove event: {other:?}"),
        }
        match &remove_event.events[1] {
            BookDelta::Trade { price, quantity } => {
                assert_eq!(*price, 100);
                assert_eq!(*quantity, 2);
            }
            other => panic!("unexpected trade event: {other:?}"),
        }
    }
}
