use crate::accounts::PublicUserProfile;
use crate::admin::{
    AdminService, AdminStateResponse, AdminTelemetryResponse, CompetitionLeaderboardSnapshot,
    CompetitionSnapshotQuery, DeleteMarketResponse, FinalizeCompetitionRequest,
    FinalizeCompetitionResponse, LeaderboardRow, ListQuery, LoadExchangeConfigRequest,
    LoadExchangeConfigResponse, MarketDefinition, ProvisionedUsersQuery, ProvisionedUsersResponse,
    SendAdminMessageRequest, SettleMarketRequest, SettleMarketResponse, TradingControlResponse,
    UpdateMarketRequest, UpsertMarketRequest,
};
use crate::auth::{
    AuthError, AuthService, AuthenticatedAdmin, AuthenticatedUser, ProvisionUserRequest,
    ProvisionUserResponse,
};
use crate::bots::{
    AdminBotState, AdminDeskOrderRequest, AdminDeskOrderResponse, AdminDeskSummary,
    BotControlError, UpsertAdminBotRequest,
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

impl IntoResponse for BotControlError {
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
    let persistence = state.persistence_status();
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
    security(("admin_bearer" = [])),
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
    security(("admin_bearer" = [])),
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
    security(("admin_bearer" = [])),
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
    security(("competitor_api_key" = [])),
    responses(
        (status = 200, description = "Authenticated user profile", body = PublicUserProfile)
    )
)]
pub async fn get_user(State(state): State<AppState>, auth: AuthenticatedUser) -> impl IntoResponse {
    let profile = state
        .storage
        .get_user(auth.trader_id)
        .map(|user| user.profile)
        .expect("authenticated user should exist");
    Json(PublicUserProfile::from(&profile))
}

#[utoipa::path(
    get,
    path = "/api/v1/positions",
    tag = "account",
    security(("competitor_api_key" = [])),
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

#[utoipa::path(
    get,
    path = "/api/v1/balance",
    tag = "account",
    security(("competitor_api_key" = [])),
    responses(
        (status = 200, description = "Per-asset balances", body = [crate::state::Balance])
    )
)]
pub async fn get_balance(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
) -> impl IntoResponse {
    Json(state.storage.list_balances(auth.trader_id))
}

#[utoipa::path(
    get,
    path = "/api/v1/portfolio",
    tag = "account",
    security(("competitor_api_key" = [])),
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
    security(("competitor_api_key" = [])),
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
    security(("competitor_api_key" = [])),
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
    security(("competitor_api_key" = [])),
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
    security(("competitor_api_key" = [])),
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
    security(("competitor_api_key" = [])),
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

#[utoipa::path(
    get,
    path = "/api/v1/markets",
    tag = "system",
    responses(
        (status = 200, description = "Active markets", body = [MarketDefinition])
    )
)]
pub async fn get_markets(State(state): State<AppState>) -> impl IntoResponse {
    Json(AdminService::list_markets(&state))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/state",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "Operator snapshot of controls, markets, desk, bots, and messages", body = AdminStateResponse),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn get_admin_state(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::get_state(&state, 50))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/desk/ensure",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "Admin desk user ready", body = AdminDeskSummary),
        (status = 400, description = "Invalid request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn ensure_admin_desk(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> Result<Json<AdminDeskSummary>, (StatusCode, Json<ApiError>)> {
    AdminService::ensure_admin_desk(&state, &admin)
        .map(Json)
        .map_err(|error| (error.status_code(), Json(ApiError::from(error))))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/telemetry",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "Live operator telemetry and health snapshot", body = AdminTelemetryResponse),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn get_admin_telemetry(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::get_telemetry(&state))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/desk/orders",
    tag = "admin",
    security(("admin_bearer" = [])),
    request_body = AdminDeskOrderRequest,
    responses(
        (status = 201, description = "Desk order submitted", body = AdminDeskOrderResponse),
        (status = 400, description = "Invalid order or market", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Desk or market not found", body = ApiError),
        (status = 409, description = "Trading or market state blocks order", body = ApiError)
    )
)]
pub async fn submit_admin_desk_order(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Json(request): Json<AdminDeskOrderRequest>,
) -> Result<(StatusCode, Json<AdminDeskOrderResponse>), (StatusCode, Json<ApiError>)> {
    AdminService::submit_admin_desk_order(&state, &admin, request)
        .await
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| {
            let status = error.status_code();
            (
                status,
                Json(ApiError {
                    error: error.to_string(),
                }),
            )
        })
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/bots",
    tag = "admin",
    security(("admin_bearer" = [])),
    request_body = UpsertAdminBotRequest,
    responses(
        (status = 201, description = "Bot created or updated", body = AdminBotState),
        (status = 400, description = "Invalid bot configuration", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Market not found", body = ApiError)
    )
)]
pub async fn upsert_admin_bot(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Json(request): Json<UpsertAdminBotRequest>,
) -> Result<(StatusCode, Json<AdminBotState>), BotControlError> {
    AdminService::upsert_bot(&state, &admin, request)
        .await
        .map(|response| (StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/bots/{bot_id}/start",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("bot_id" = String, Path, description = "Bot identifier")
    ),
    responses(
        (status = 200, description = "Bot running", body = AdminBotState),
        (status = 400, description = "Invalid request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Bot or market not found", body = ApiError)
    )
)]
pub async fn start_admin_bot(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Path(bot_id): Path<String>,
) -> Result<Json<AdminBotState>, BotControlError> {
    AdminService::start_bot(&state, &admin, &bot_id)
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/bots/start",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "All bots running", body = [AdminBotState]),
        (status = 400, description = "Invalid request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Bot or market not found", body = ApiError)
    )
)]
pub async fn start_all_admin_bots(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> Result<Json<Vec<AdminBotState>>, BotControlError> {
    AdminService::start_all_bots(&state, &admin).await.map(Json)
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/bots/{bot_id}/pause",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("bot_id" = String, Path, description = "Bot identifier")
    ),
    responses(
        (status = 200, description = "Bot paused", body = AdminBotState),
        (status = 400, description = "Invalid request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Bot not found", body = ApiError)
    )
)]
pub async fn pause_admin_bot(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Path(bot_id): Path<String>,
) -> Result<Json<AdminBotState>, BotControlError> {
    AdminService::pause_bot(&state, &admin, &bot_id)
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/bots/pause",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "All bots paused", body = [AdminBotState]),
        (status = 400, description = "Invalid request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Bot not found", body = ApiError)
    )
)]
pub async fn pause_all_admin_bots(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> Result<Json<Vec<AdminBotState>>, BotControlError> {
    AdminService::pause_all_bots(&state, &admin).await.map(Json)
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/bots/{bot_id}",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("bot_id" = String, Path, description = "Bot identifier")
    ),
    responses(
        (status = 200, description = "Bot removed", body = AdminBotState),
        (status = 400, description = "Invalid request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Bot not found", body = ApiError)
    )
)]
pub async fn delete_admin_bot(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Path(bot_id): Path<String>,
) -> Result<Json<AdminBotState>, BotControlError> {
    AdminService::delete_bot(&state, &admin, &bot_id)
        .await
        .map(Json)
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/bots",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "All bots removed", body = [AdminBotState]),
        (status = 400, description = "Invalid request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Bot not found", body = ApiError)
    )
)]
pub async fn delete_all_admin_bots(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> Result<Json<Vec<AdminBotState>>, BotControlError> {
    AdminService::delete_all_bots(&state, &admin)
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/trading/start",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "Trading enabled", body = TradingControlResponse),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn start_trading(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::set_trading_enabled(&state, &admin, true))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/trading/stop",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "Trading disabled", body = TradingControlResponse),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn stop_trading(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::set_trading_enabled(&state, &admin, false))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/markets",
    tag = "admin",
    security(("admin_bearer" = [])),
    responses(
        (status = 200, description = "All markets", body = [MarketDefinition]),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn list_admin_markets(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
) -> impl IntoResponse {
    Json(AdminService::list_markets(&state))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/markets",
    tag = "admin",
    security(("admin_bearer" = [])),
    request_body = UpsertMarketRequest,
    responses(
        (status = 200, description = "Market upserted", body = MarketDefinition),
        (status = 400, description = "Invalid market definition", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Referenced market missing", body = ApiError),
        (status = 409, description = "Market state conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
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

#[utoipa::path(
    patch,
    path = "/api/v1/admin/markets/{market_id}",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("market_id" = String, Path, description = "Market symbol")
    ),
    request_body = UpdateMarketRequest,
    responses(
        (status = 200, description = "Market updated", body = MarketDefinition),
        (status = 400, description = "Invalid update", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Market not found", body = ApiError),
        (status = 409, description = "Market state conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
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

#[utoipa::path(
    delete,
    path = "/api/v1/admin/markets/{market_id}",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("market_id" = String, Path, description = "Market symbol")
    ),
    responses(
        (status = 200, description = "Market deleted", body = DeleteMarketResponse),
        (status = 400, description = "Invalid request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Market not found", body = ApiError),
        (status = 409, description = "Market has open orders or is settled", body = ApiError)
    )
)]
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

#[utoipa::path(
    post,
    path = "/api/v1/admin/config/load",
    tag = "admin",
    security(("admin_bearer" = [])),
    request_body = LoadExchangeConfigRequest,
    responses(
        (status = 200, description = "Config applied", body = LoadExchangeConfigResponse),
        (status = 400, description = "Invalid config payload", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Missing market or user reference", body = ApiError),
        (status = 409, description = "State conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub async fn load_exchange_config(
    State(state): State<AppState>,
    admin: AuthenticatedAdmin,
    Json(request): Json<LoadExchangeConfigRequest>,
) -> Result<Json<LoadExchangeConfigResponse>, (StatusCode, Json<ApiError>)> {
    AdminService::load_exchange_config(&state, &admin, request)
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

#[utoipa::path(
    post,
    path = "/api/v1/admin/messages",
    tag = "admin",
    security(("admin_bearer" = [])),
    request_body = SendAdminMessageRequest,
    responses(
        (status = 200, description = "Message recorded and broadcast", body = crate::admin::AdminMessageEntry),
        (status = 400, description = "Invalid message", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Target user not found", body = ApiError)
    )
)]
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/messages",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("limit" = Option<usize>, Query, description = "Maximum messages to return (default 50)")
    ),
    responses(
        (status = 200, description = "Recent admin messages", body = [crate::admin::AdminMessageEntry]),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
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

#[utoipa::path(
    post,
    path = "/api/v1/admin/markets/{market_id}/settle",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("market_id" = String, Path, description = "Market symbol")
    ),
    request_body = SettleMarketRequest,
    responses(
        (status = 200, description = "Market settled", body = SettleMarketResponse),
        (status = 400, description = "Invalid settlement", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Market not found", body = ApiError),
        (status = 409, description = "Market already settled or has blocking state", body = ApiError),
        (status = 500, description = "Settlement failed", body = ApiError)
    )
)]
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

#[utoipa::path(
    post,
    path = "/api/v1/admin/competition/finalize",
    tag = "admin",
    security(("admin_bearer" = [])),
    request_body = FinalizeCompetitionRequest,
    responses(
        (status = 200, description = "Competition finalized", body = FinalizeCompetitionResponse),
        (status = 400, description = "Invalid finalize request", body = ApiError),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "User or snapshot reference not found", body = ApiError),
        (status = 409, description = "Eligibility or settlement conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/competition/snapshots/{snapshot_id}",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("snapshot_id" = Uuid, Path, description = "Snapshot UUID")
    ),
    responses(
        (status = 200, description = "Leaderboard snapshot", body = CompetitionLeaderboardSnapshot),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Snapshot not found", body = ApiError)
    )
)]
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/competition/snapshots/latest",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("competition_id" = String, Query, description = "Competition identifier")
    ),
    responses(
        (status = 200, description = "Latest snapshot for competition", body = CompetitionLeaderboardSnapshot),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "No snapshot for competition", body = ApiError)
    )
)]
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/competition/snapshots/{snapshot_id}/export.csv",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("snapshot_id" = Uuid, Path, description = "Snapshot UUID")
    ),
    responses(
        (status = 200, description = "CSV attachment", body = String, content_type = "text/csv"),
        (status = 401, description = "Invalid admin token", body = ApiError),
        (status = 404, description = "Snapshot not found", body = ApiError)
    )
)]
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
    security(("admin_bearer" = [])),
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/leaderboard",
    tag = "admin",
    security(("admin_bearer" = [])),
    params(
        ("limit" = Option<usize>, Query, description = "Optional maximum number of rows to return")
    ),
    responses(
        (status = 200, description = "Live leaderboard (operator view)", body = [LeaderboardRow]),
        (status = 401, description = "Invalid admin token", body = ApiError)
    )
)]
pub async fn get_admin_leaderboard(
    State(state): State<AppState>,
    _admin: AuthenticatedAdmin,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    Json(AdminService::leaderboard(&state, query.limit).await)
}
