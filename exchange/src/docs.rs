use crate::accounts::UserProfile;
use crate::auth::{ProvisionUserRequest, ProvisionUserResponse};
use crate::orderbook::{Fill, Order, Side};
use crate::rest::{ApiError, HealthResponse};
use crate::state::{Balance, PortfolioSnapshot};
use crate::trading::{
    AmendOrderRequest, AmendOrderResponse, CancelOrderResponse, SubmitOrderRequest,
    SubmitOrderResponse,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::rest::health,
        crate::rest::provision_user,
        crate::rest::get_user,
        crate::rest::get_balance,
        crate::rest::get_portfolio,
        crate::rest::get_open_orders,
        crate::rest::get_fills,
        crate::rest::submit_order,
        crate::rest::cancel_order,
        crate::rest::amend_order
    ),
    components(
        schemas(
            HealthResponse,
            ApiError,
            UserProfile,
            Side,
            Order,
            Fill,
            Balance,
            PortfolioSnapshot,
            ProvisionUserRequest,
            ProvisionUserResponse,
            SubmitOrderRequest,
            SubmitOrderResponse,
            CancelOrderResponse,
            AmendOrderRequest,
            AmendOrderResponse
        )
    ),
    tags(
        (name = "system", description = "Service and health endpoints"),
        (name = "admin", description = "Operator-only administrative endpoints"),
        (name = "account", description = "Trader account data endpoints"),
        (name = "trading", description = "Order entry and order management")
    )
)]
pub struct ApiDoc;
