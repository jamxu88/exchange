use crate::accounts::UserProfile;
use crate::auth::{
    AuthError, AuthService, AuthenticatedAdmin, AuthenticatedUser, ProvisionUserRequest,
    ProvisionUserResponse,
};
use crate::state::{AppState, Balance, PortfolioSnapshot};
use crate::storage::{PersistenceMode, PersistenceStatus};
use crate::trading::{
    AmendOrderRequest, AmendOrderResponse, CancelOrderResponse, SubmitOrderRequest,
    SubmitOrderResponse, TradingError, TradingService,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
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
            TradingError::InvalidMarket
            | TradingError::InvalidPrice
            | TradingError::InvalidQuantity
            | TradingError::InvalidRemaining
            | TradingError::InvalidAmend => StatusCode::BAD_REQUEST,
            TradingError::OrderNotFound => StatusCode::NOT_FOUND,
            TradingError::OrderNotOwned => StatusCode::FORBIDDEN,
            TradingError::InsufficientBalance { .. } => StatusCode::CONFLICT,
            TradingError::Overflow => StatusCode::INTERNAL_SERVER_ERROR,
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
    let status = match persistence.mode {
        PersistenceMode::Retrying | PersistenceMode::Backpressured | PersistenceMode::Stopped => {
            "degraded"
        }
        PersistenceMode::Disabled | PersistenceMode::Ok => "ok",
    };

    Json(HealthResponse {
        status: status.to_string(),
        service: "exchange".to_string(),
        now: Utc::now().to_rfc3339(),
        persistence,
    })
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
    path = "/api/v1/balance",
    tag = "account",
    responses(
        (status = 200, description = "Balances", body = [Balance])
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
        balances: state.storage.list_balances(auth.trader_id),
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
        (status = 409, description = "Insufficient balance", body = ApiError)
    )
)]
pub async fn submit_order(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Json(request): Json<SubmitOrderRequest>,
) -> Result<(StatusCode, Json<SubmitOrderResponse>), TradingError> {
    let response = TradingService::submit_limit_order(&state, auth.trader_id, request).await?;
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
