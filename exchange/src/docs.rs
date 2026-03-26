use crate::accounts::{UserProfile, UserRole};
use crate::admin::{ProvisionedUserCredential, ProvisionedUsersQuery, ProvisionedUsersResponse};
use crate::auth::{ProvisionUserRequest, ProvisionUserResponse};
use crate::orderbook::{Fill, Order, Side};
use crate::rest::{ApiError, HealthResponse};
use crate::state::{PortfolioSnapshot, Position};
use crate::trading::{
    AmendOrderRequest, AmendOrderResponse, CancelOrderResponse, SubmitOrderRequest,
    SubmitOrderResponse,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::rest::health,
        crate::rest::list_provisioned_users,
        crate::rest::provision_user,
        crate::rest::export_provisioned_users_csv,
        crate::rest::get_user,
        crate::rest::get_positions,
        crate::rest::get_portfolio,
        crate::rest::get_open_orders,
        crate::rest::get_fills,
        crate::rest::submit_order,
        crate::rest::cancel_order,
        crate::rest::amend_order,
        crate::rest::reset_all_users
    ),
    components(
        schemas(
            HealthResponse,
            ApiError,
            UserProfile,
            UserRole,
            Side,
            Order,
            Fill,
            Position,
            PortfolioSnapshot,
            ProvisionedUsersQuery,
            ProvisionedUserCredential,
            ProvisionedUsersResponse,
            ProvisionUserRequest,
            ProvisionUserResponse,
            SubmitOrderRequest,
            SubmitOrderResponse,
            CancelOrderResponse,
            AmendOrderRequest,
            AmendOrderResponse,
            crate::admin::ResetUsersResponse
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
