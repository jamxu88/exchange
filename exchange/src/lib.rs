pub mod accounts;
pub mod admin;
pub mod auth;
pub mod bots;
mod checkpoint;
pub mod config;
mod derived_marketdata;
pub mod docs;
pub mod marketdata;
mod marketdata_bridge;
pub mod marketdata_ipc;
pub mod marketdata_service;
pub mod matching;
pub mod orderbook;
pub mod rate_limit;
pub mod rest;
pub mod settlement;
pub mod state;
pub mod storage;
pub mod telemetry;
pub mod trading;
pub mod ws;

use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
};
use docs::ApiDoc;
use state::AppState;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub fn build_app(app_state: AppState) -> Router {
    let public_routes = Router::new()
        .route("/health", get(rest::health))
        .route("/ws", get(ws::ws_handler))
        .route("/api/v1/markets", get(rest::get_markets))
        .with_state(app_state.clone());

    let admin_routes = Router::new()
        .route(
            "/api/v1/admin/users",
            get(rest::list_provisioned_users).post(rest::provision_user),
        )
        .route(
            "/api/v1/admin/users/export.csv",
            get(rest::export_provisioned_users_csv),
        )
        .route("/api/v1/admin/state", get(rest::get_admin_state))
        .route("/api/v1/admin/trading/start", post(rest::start_trading))
        .route("/api/v1/admin/trading/stop", post(rest::stop_trading))
        .route(
            "/api/v1/admin/bots",
            post(rest::upsert_admin_bot).delete(rest::delete_all_admin_bots),
        )
        .route("/api/v1/admin/bots/start", post(rest::start_all_admin_bots))
        .route("/api/v1/admin/bots/pause", post(rest::pause_all_admin_bots))
        .route(
            "/api/v1/admin/bots/:bot_id/start",
            post(rest::start_admin_bot),
        )
        .route(
            "/api/v1/admin/bots/:bot_id/pause",
            post(rest::pause_admin_bot),
        )
        .route("/api/v1/admin/bots/:bot_id", delete(rest::delete_admin_bot))
        .route("/api/v1/admin/desk/ensure", post(rest::ensure_admin_desk))
        .route(
            "/api/v1/admin/desk/orders",
            post(rest::submit_admin_desk_order),
        )
        .route(
            "/api/v1/admin/markets",
            get(rest::list_admin_markets).post(rest::create_or_update_market),
        )
        .route(
            "/api/v1/admin/markets/:market_id",
            delete(rest::delete_market).patch(rest::patch_market),
        )
        .route(
            "/api/v1/admin/markets/:market_id/settle",
            post(rest::settle_market),
        )
        .route(
            "/api/v1/admin/competition/finalize",
            post(rest::finalize_competition),
        )
        .route(
            "/api/v1/admin/competition/snapshots/latest",
            get(rest::get_latest_competition_snapshot),
        )
        .route(
            "/api/v1/admin/competition/snapshots/:snapshot_id",
            get(rest::get_competition_snapshot),
        )
        .route(
            "/api/v1/admin/competition/snapshots/:snapshot_id/export.csv",
            get(rest::export_competition_snapshot_csv),
        )
        .route(
            "/api/v1/admin/config/load",
            post(rest::load_exchange_config),
        )
        .route(
            "/api/v1/admin/messages",
            get(rest::list_admin_messages).post(rest::send_admin_message),
        )
        .route("/api/v1/admin/telemetry", get(rest::get_admin_telemetry))
        .route("/api/v1/admin/users/reset", post(rest::reset_all_users))
        .route(
            "/api/v1/admin/leaderboard",
            get(rest::get_admin_leaderboard),
        )
        .with_state(app_state.clone());

    let protected_routes = Router::new()
        .route("/api/v1/user", get(rest::get_user))
        .route("/api/v1/positions", get(rest::get_positions))
        .route("/api/v1/balance", get(rest::get_balance))
        .route("/api/v1/portfolio", get(rest::get_portfolio))
        .route("/api/v1/open-orders", get(rest::get_open_orders))
        .route("/api/v1/fills", get(rest::get_fills))
        .route("/api/v1/orders", post(rest::submit_order))
        .route(
            "/api/v1/orders/:order_id",
            delete(rest::cancel_order).patch(rest::amend_order),
        )
        .with_state(app_state.clone())
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            rate_limit::authenticated_user_rate_limit,
        ));

    Router::new()
        .merge(public_routes)
        .merge(admin_routes)
        .merge(protected_routes)
        .merge(SwaggerUi::new("/docs").url("/api-doc/openapi.json", ApiDoc::openapi()))
        .fallback(|| async { (StatusCode::NOT_FOUND, "not found") })
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}
