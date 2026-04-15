use crate::accounts::{PublicUserProfile, UserProfile, UserRole};
use crate::admin::{
    AdminMessageEntry, AdminMessageLevel, AdminStateResponse, AdminTelemetryResponse,
    CompetitionLeaderboardSnapshot, CompetitionSettlementRequest, DeleteMarketResponse,
    ExchangeControls, FinalizeCompetitionRequest, FinalizeCompetitionResponse, LeaderboardRow,
    LoadExchangeConfigRequest, LoadExchangeConfigResponse, MarketDefinition, MarketStatus,
    ProvisionedUserCredential, ProvisionedUsersQuery, ProvisionedUsersResponse,
    SendAdminMessageRequest, SettleMarketRequest, SettleMarketResponse, TradingControlResponse,
    UpdateMarketRequest, UpsertMarketRequest,
};
use crate::auth::{ProvisionUserRequest, ProvisionUserResponse};
use crate::bots::{
    AdminBotState, AdminDeskOrderRequest, AdminDeskOrderResponse, AdminDeskSummary, BotSideMode,
    BotStatus, UpsertAdminBotRequest,
};
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
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};

/// Registers `admin_bearer` (`Authorization: Bearer`) and `competitor_api_key` (`x-api-key`) for Swagger UI.
struct ExchangeSecuritySchemes;

impl Modify for ExchangeSecuritySchemes {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let Some(components) = openapi.components.as_mut() else {
            return;
        };
        components.add_security_scheme(
            "admin_bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("ADMIN_API_TOKEN")
                    .description(Some(
                        "Same secret as the exchange `ADMIN_API_TOKEN` environment variable. Send as HTTP header `Authorization: Bearer <token>`.",
                    ))
                    .build(),
            ),
        );
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
        crate::rest::list_provisioned_users,
        crate::rest::provision_user,
        crate::rest::export_provisioned_users_csv,
        crate::rest::get_admin_state,
        crate::rest::get_admin_telemetry,
        crate::rest::ensure_admin_desk,
        crate::rest::submit_admin_desk_order,
        crate::rest::upsert_admin_bot,
        crate::rest::start_all_admin_bots,
        crate::rest::start_admin_bot,
        crate::rest::pause_all_admin_bots,
        crate::rest::pause_admin_bot,
        crate::rest::delete_all_admin_bots,
        crate::rest::delete_admin_bot,
        crate::rest::start_trading,
        crate::rest::stop_trading,
        crate::rest::list_admin_markets,
        crate::rest::create_or_update_market,
        crate::rest::patch_market,
        crate::rest::delete_market,
        crate::rest::load_exchange_config,
        crate::rest::send_admin_message,
        crate::rest::list_admin_messages,
        crate::rest::settle_market,
        crate::rest::finalize_competition,
        crate::rest::get_competition_snapshot,
        crate::rest::get_latest_competition_snapshot,
        crate::rest::export_competition_snapshot_csv,
        crate::rest::reset_all_users,
        crate::rest::get_admin_leaderboard,
    ),
    components(
        schemas(
            HealthResponse,
            ApiError,
            PublicUserProfile,
            UserProfile,
            UserRole,
            Side,
            Order,
            Fill,
            Position,
            Balance,
            PortfolioSnapshot,
            OrderType,
            ProvisionedUsersQuery,
            ProvisionedUserCredential,
            ProvisionedUsersResponse,
            ProvisionUserRequest,
            ProvisionUserResponse,
            MarketDefinition,
            MarketStatus,
            ExchangeControls,
            AdminStateResponse,
            AdminTelemetryResponse,
            AdminMessageEntry,
            AdminMessageLevel,
            TradingControlResponse,
            UpsertMarketRequest,
            UpdateMarketRequest,
            LoadExchangeConfigRequest,
            LoadExchangeConfigResponse,
            SendAdminMessageRequest,
            SettleMarketRequest,
            SettleMarketResponse,
            FinalizeCompetitionRequest,
            FinalizeCompetitionResponse,
            CompetitionSettlementRequest,
            CompetitionLeaderboardSnapshot,
            LeaderboardRow,
            DeleteMarketResponse,
            AdminBotState,
            BotSideMode,
            BotStatus,
            UpsertAdminBotRequest,
            AdminDeskSummary,
            AdminDeskOrderRequest,
            AdminDeskOrderResponse,
            SubmitOrderRequest,
            SubmitOrderResponse,
            CancelOrderResponse,
            AmendOrderRequest,
            AmendOrderResponse,
            crate::admin::ResetUsersResponse,
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
        (name = "admin", description = "Operator-only administrative endpoints (Authorization: Bearer)"),
        (name = "account", description = "Trader account data (x-api-key)"),
        (name = "trading", description = "Order entry and order management (x-api-key)")
    )
)]
pub struct ApiDoc;
