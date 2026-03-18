pub mod accounts;
pub mod admin;
pub mod auth;
pub mod config;
pub mod docs;
pub mod marketdata;
pub mod matching;
pub mod orderbook;
pub mod rate_limit;
pub mod rest;
pub mod settlement;
pub mod state;
pub mod storage;
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
        .with_state(app_state.clone());

    let admin_routes = Router::new()
        .route("/api/v1/admin/users", post(rest::provision_user))
        .with_state(app_state.clone());

    let protected_routes = Router::new()
        .route("/api/v1/user", get(rest::get_user))
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
