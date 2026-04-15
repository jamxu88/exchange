use crate::accounts::{PublicUserProfile, UserRole};
use crate::admin::MarketDefinition;
use crate::orderbook::{Fill, Order, Side};
use crate::rest::{ApiError, HealthResponse};
use crate::state::{
    AccountBarrierStatus, Balance, BarrierWaitStatus, DispatchQueueMode, DispatchQueueStatus,
    PortfolioSnapshot, Position,
};
use crate::storage::{PersistenceMode, PersistenceStatus, StorageBackendKind};
use crate::telemetry::{
    ActionTelemetrySnapshot, CounterTelemetrySnapshot, FillTelemetrySnapshot,
    OperatorTelemetrySnapshot, ResyncTelemetrySnapshot, WebSocketTelemetrySnapshot,
};
use crate::trading::{
    AmendOrderRequest, AmendOrderResponse, CancelOrderResponse, OrderType, SubmitOrderRequest,
    SubmitOrderResponse,
};
use utoipa::Modify;
use utoipa::OpenApi;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};

/// Registers `competitor_api_key` (`x-api-key`) for Swagger UI. Admin HTTP routes are not listed here.
struct ExchangeSecuritySchemes;

impl Modify for ExchangeSecuritySchemes {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let Some(components) = openapi.components.as_mut() else {
            return;
        };
        components.add_security_scheme(
            "competitor_api_key",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                "x-api-key",
                "Assigned competitor API key.",
            ))),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    modifiers(&ExchangeSecuritySchemes),
    paths(
        crate::rest::health,
        crate::rest::get_markets,
        crate::rest::get_user,
        crate::rest::get_positions,
        crate::rest::get_balance,
        crate::rest::get_portfolio,
        crate::rest::get_open_orders,
        crate::rest::get_fills,
        crate::rest::submit_order,
        crate::rest::cancel_order,
        crate::rest::amend_order,
    ),
    components(
        schemas(
            HealthResponse,
            ApiError,
            PublicUserProfile,
            UserRole,
            Side,
            Order,
            Fill,
            Position,
            Balance,
            PortfolioSnapshot,
            OrderType,
            MarketDefinition,
            crate::admin::MarketStatus,
            SubmitOrderRequest,
            SubmitOrderResponse,
            CancelOrderResponse,
            AmendOrderRequest,
            AmendOrderResponse,
            PersistenceStatus,
            PersistenceMode,
            StorageBackendKind,
            DispatchQueueStatus,
            DispatchQueueMode,
            AccountBarrierStatus,
            BarrierWaitStatus,
            ActionTelemetrySnapshot,
            CounterTelemetrySnapshot,
            FillTelemetrySnapshot,
            OperatorTelemetrySnapshot,
            ResyncTelemetrySnapshot,
            WebSocketTelemetrySnapshot
        )
    ),
    tags(
        (name = "system", description = "Service health and public market metadata"),
        (name = "account", description = "Trader account data (x-api-key)"),
        (name = "trading", description = "Order entry and order management (x-api-key)")
    )
)]
pub struct ApiDoc;
