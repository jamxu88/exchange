use crate::accounts::UserProfile;
use crate::admin::{
    AdminService, CompetitionLeaderboardSnapshot, CompetitionSnapshotQuery, DeleteMarketResponse,
    FinalizeCompetitionRequest, FinalizeCompetitionResponse, ListQuery, LoadExchangeConfigRequest,
    LoadExchangeConfigResponse, MarketDefinition, ProvisionedUsersQuery, ProvisionedUsersResponse,
    SendAdminMessageRequest, SettleMarketRequest, SettleMarketResponse, UpdateMarketRequest,
    UpsertMarketRequest,
};
use crate::auth::{
    AuthError, AuthService, AuthenticatedAdmin, AuthenticatedUser, ProvisionUserRequest,
    ProvisionUserResponse,
};
use crate::settlement::SettlementEngine;
use crate::state::{
    AccountBarrierStatus, AppState, DispatchQueueMode, DispatchQueueStatus, PortfolioSnapshot,
    Position,
};
use crate::storage::{PersistenceMode, PersistenceStatus};
use crate::trading::{
    AmendOrderRequest, AmendOrderResponse, CancelOrderResponse, SubmitOrderRequest,
    SubmitOrderResponse, TradingError, TradingService,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub now: String,
    pub persistence: PersistenceStatus,
    pub runtime_dispatch: DispatchQueueStatus,
    pub account_dispatch: DispatchQueueStatus,
    pub persistence_dispatch: DispatchQueueStatus,
    pub account_barrier: AccountBarrierStatus,
}

#[derive(Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct MarketFilter {
    pub market: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    pub error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

impl From<AuthError> for ApiError {
    fn from(value: AuthError) -> Self {
        Self {
            error: value.to_string(),
        }
    }
}

impl TradingError {
    fn status_code(&self) -> StatusCode {
        match self {
            TradingError::TradingDisabled => StatusCode::CONFLICT,
            TradingError::InvalidMarket
            | TradingError::TickSizeViolation { .. }
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

impl IntoResponse for TradingError {
    fn into_response(self) -> Response {
        (
            self.status_code(),
            Json(ApiError {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    responses(
        (status = 200, description = "Service health", body = HealthResponse)
    )
)]
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let persistence = state.storage.persistence_status();
    let runtime_dispatch = state.runtime_dispatch_status();
    let account_dispatch = state.account_dispatch_status();
    let persistence_dispatch = state.persistence_dispatch_status();
    let account_barrier = state.account_barrier_status();
    let status = if matches!(
        persistence.mode,
        PersistenceMode::Retrying | PersistenceMode::Backpressured | PersistenceMode::Stopped
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

    Json(HealthResponse {
        status: status.to_string(),
        service: "exchange".to_string(),
        now: Utc::now().to_rfc3339(),
        persistence,
        runtime_dispatch,
        account_dispatch,
        persistence_dispatch,
        account_barrier,
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/users",
    tag = "admin",
    params(
        ("username_prefix" = Option<String>, Query, description = "Optional username prefix filter"),
        ("role" = Option<crate::accounts::UserRole>, Query, description = "Optional role filter"),
        ("limit" = Option<usize>, Query, description = "Optional maximum number of rows to return")
    ),
    responses(
        (status = 200, description = "Provisioned user roster with API keys", body = ProvisionedUsersResponse),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn list_provisioned_users(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Query(query): Query<ProvisionedUsersQuery>,
) -> impl IntoResponse {
    Json(AdminService::list_provisioned_users(&state, &admin, query))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users",
    tag = "admin",
    request_body = ProvisionUserRequest,
    responses(
        (status = 201, description = "Competition user provisioned", body = ProvisionUserResponse),
        (status = 400, description = "Invalid provisioning request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 409, description = "Username already exists", body = ApiError)
    )
)]
pub async fn provision_user(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Json(request): Json<ProvisionUserRequest>,
) -> Result<(StatusCode, Json<ProvisionUserResponse>), (StatusCode, Json<ApiError>)> {
    AuthService::provision_user_as_admin(&state, &admin, request)
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|err| (err.status_code(), Json(ApiError::from(err))))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/users/export.csv",
    tag = "admin",
    params(
        ("username_prefix" = Option<String>, Query, description = "Optional username prefix filter"),
        ("role" = Option<crate::accounts::UserRole>, Query, description = "Optional role filter"),
        ("limit" = Option<usize>, Query, description = "Optional maximum number of rows to export")
    ),
    responses(
        (status = 200, description = "Provisioned users exported as CSV", body = String),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn export_provisioned_users_csv(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Query(query): Query<ProvisionedUsersQuery>,
) -> Response {
    let csv = AdminService::export_provisioned_users_csv(&state, &admin, query);
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"provisioned-users.csv\"",
            ),
        ],
        csv,
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/user",
    tag = "account",
    responses(
        (status = 200, description = "Authenticated user profile", body = UserProfile)
    )
)]
pub async fn get_user(State(state): State<AppState>, auth: AuthenticatedUser) -> impl IntoResponse {
    let profile = state
        .storage
        .get_user(auth.trader_id)
        .map(|user| user.profile)
        .expect("authenticated user should exist");
    Json(profile)
}

#[utoipa::path(
    get,
    path = "/api/v1/positions",
    tag = "account",
    responses(
        (status = 200, description = "Positions", body = [Position])
    )
)]
pub async fn get_positions(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
) -> impl IntoResponse {
    Json(state.storage.list_positions(auth.trader_id))
}

pub async fn get_balance(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
) -> impl IntoResponse {
    Json(state.storage.list_positions(auth.trader_id))
}

#[utoipa::path(
    get,
    path = "/api/v1/portfolio",
    tag = "account",
    responses(
        (status = 200, description = "Portfolio", body = PortfolioSnapshot)
    )
)]
pub async fn get_portfolio(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
) -> impl IntoResponse {
    Json(PortfolioSnapshot {
        trader_id: auth.trader_id,
        position_limit: SettlementEngine::position_limit_for_role(auth.role),
        positions: state.storage.list_positions(auth.trader_id),
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/open-orders",
    tag = "account",
    params(
        ("market" = Option<String>, Query, description = "Optional market filter")
    ),
    responses(
        (status = 200, description = "Open orders", body = [crate::orderbook::Order])
    )
)]
pub async fn get_open_orders(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Query(filter): Query<MarketFilter>,
) -> impl IntoResponse {
    Json(
        state
            .storage
            .list_open_orders(auth.trader_id, filter.market.as_deref()),
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/fills",
    tag = "account",
    params(
        ("market" = Option<String>, Query, description = "Optional market filter")
    ),
    responses(
        (status = 200, description = "Fills", body = [crate::orderbook::Fill])
    )
)]
pub async fn get_fills(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Query(filter): Query<MarketFilter>,
) -> impl IntoResponse {
    Json(
        state
            .storage
            .list_fills(auth.trader_id, filter.market.as_deref()),
    )
}

#[utoipa::path(
    post,
    path = "/api/v1/orders",
    tag = "trading",
    request_body = SubmitOrderRequest,
    responses(
        (status = 201, description = "Limit order accepted", body = SubmitOrderResponse),
        (status = 400, description = "Invalid order", body = ApiError),
        (status = 409, description = "Projected position limit breach", body = ApiError)
    )
)]
pub async fn submit_order(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Json(request): Json<SubmitOrderRequest>,
) -> Result<(StatusCode, Json<SubmitOrderResponse>), TradingError> {
    let response = TradingService::submit_order(&state, auth.trader_id, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/orders/{order_id}",
    tag = "trading",
    params(
        ("order_id" = Uuid, Path, description = "Order id")
    ),
    responses(
        (status = 200, description = "Order canceled", body = CancelOrderResponse),
        (status = 404, description = "Order not found", body = ApiError)
    )
)]
pub async fn cancel_order(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Path(order_id): Path<Uuid>,
) -> Result<Json<CancelOrderResponse>, TradingError> {
    let response = TradingService::cancel_order(&state, auth.trader_id, order_id).await?;
    Ok(Json(response))
}

#[utoipa::path(
    patch,
    path = "/api/v1/orders/{order_id}",
    tag = "trading",
    params(
        ("order_id" = Uuid, Path, description = "Order id")
    ),
    request_body = AmendOrderRequest,
    responses(
        (status = 200, description = "Order amended", body = AmendOrderResponse),
        (status = 400, description = "Invalid amend", body = ApiError),
        (status = 404, description = "Order not found", body = ApiError)
    )
)]
pub async fn amend_order(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Path(order_id): Path<Uuid>,
    Json(request): Json<AmendOrderRequest>,
) -> Result<Json<AmendOrderResponse>, TradingError> {
    let response = TradingService::amend_order(&state, auth.trader_id, order_id, request).await?;
    Ok(Json(response))
}

pub async fn get_markets(State(state): State<AppState>) -> impl IntoResponse {
    Json(AdminService::list_markets(&state))
}

pub async fn get_leaderboard(
    State(state): State<AppState>,
    _auth: AuthenticatedUser,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    Json(AdminService::leaderboard(&state, query.limit).await)
}

pub async fn get_admin_state(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::get_state(&state, 50))
}

pub async fn start_trading(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::set_trading_enabled(&state, &admin, true))
}

pub async fn stop_trading(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::set_trading_enabled(&state, &admin, false))
}

pub async fn list_admin_markets(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::list_markets(&state))
}

pub async fn create_or_update_market(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Json(request): Json<UpsertMarketRequest>,
) -> Result<Json<MarketDefinition>, (StatusCode, Json<ApiError>)> {
    AdminService::upsert_market(&state, &admin, request)
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn patch_market(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Path(market_id): Path<String>,
    Json(request): Json<UpdateMarketRequest>,
) -> Result<Json<MarketDefinition>, (StatusCode, Json<ApiError>)> {
    AdminService::update_market(&state, &admin, &market_id, request)
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn delete_market(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Path(market_id): Path<String>,
) -> Result<Json<DeleteMarketResponse>, (StatusCode, Json<ApiError>)> {
    AdminService::delete_market(&state, &admin, &market_id)
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn load_exchange_config(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Json(request): Json<LoadExchangeConfigRequest>,
) -> Result<Json<LoadExchangeConfigResponse>, (StatusCode, Json<ApiError>)> {
    AdminService::load_exchange_config(&state, &admin, request)
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn send_admin_message(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Json(request): Json<SendAdminMessageRequest>,
) -> Result<Json<crate::admin::AdminMessageEntry>, (StatusCode, Json<ApiError>)> {
    AdminService::send_message(&state, &admin, request)
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn list_admin_messages(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    Json(AdminService::list_admin_messages(
        &state,
        query.limit.unwrap_or(50),
    ))
}

pub async fn settle_market(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Path(market_id): Path<String>,
    Json(request): Json<SettleMarketRequest>,
) -> Result<Json<SettleMarketResponse>, (StatusCode, Json<ApiError>)> {
    AdminService::settle_market(&state, &admin, &market_id, request)
        .await
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn finalize_competition(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Json(request): Json<FinalizeCompetitionRequest>,
) -> Result<Json<FinalizeCompetitionResponse>, (StatusCode, Json<ApiError>)> {
    AdminService::finalize_competition(&state, &admin, request)
        .await
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn get_competition_snapshot(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
    Path(snapshot_id): Path<Uuid>,
) -> Result<Json<CompetitionLeaderboardSnapshot>, (StatusCode, Json<ApiError>)> {
    AdminService::get_competition_snapshot(&state, snapshot_id)
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn get_latest_competition_snapshot(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
    Query(query): Query<CompetitionSnapshotQuery>,
) -> Result<Json<CompetitionLeaderboardSnapshot>, (StatusCode, Json<ApiError>)> {
    AdminService::latest_competition_snapshot(&state, &query.competition_id)
        .map(Json)
        .map_err(|err| {
            (
                err.status_code(),
                Json(ApiError {
                    error: err.to_string(),
                }),
            )
        })
}

pub async fn export_competition_snapshot_csv(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
    Path(snapshot_id): Path<Uuid>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let snapshot = AdminService::get_competition_snapshot(&state, snapshot_id).map_err(|err| {
        (
            err.status_code(),
            Json(ApiError {
                error: err.to_string(),
            }),
        )
    })?;
    let filename = format!(
        "{}-{}.csv",
        snapshot.competition_id.replace(' ', "-"),
        snapshot.snapshot_id
    );
    let csv = AdminService::export_competition_snapshot_csv(&snapshot);
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{filename}\""),
            ),
        ],
        csv,
    )
        .into_response())
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/reset",
    tag = "admin",
    responses(
        (status = 200, description = "All user trading state reset", body = crate::admin::ResetUsersResponse),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn reset_all_users(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::reset_all_users(&state, &admin))
}

pub async fn get_admin_leaderboard(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    Json(AdminService::leaderboard(&state, query.limit).await)
}
