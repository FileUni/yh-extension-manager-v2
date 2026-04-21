use crate::handlers::{self, PluginAdminState};
use axum::{Router, middleware::from_fn, routing::get};
use sea_orm::DatabaseConnection;
use std::sync::Arc;

pub fn create_router(db: Arc<DatabaseConnection>) -> Router {
    Router::new()
        .route("/status", get(handlers::get_status))
        .route("/registry", get(handlers::list_registry))
        .route("/runtimes", get(handlers::list_active_runtimes))
        .route(
            "/registry/{plugin_id}/versions",
            get(handlers::list_versions),
        )
        .route("/audit", get(handlers::list_audit_logs))
        .route(
            "/install",
            axum::routing::post(handlers::install_plugin_zip),
        )
        .route(
            "/{plugin_id}/permissions",
            get(handlers::list_permission_grants),
        )
        .route(
            "/{plugin_id}/permissions",
            axum::routing::post(handlers::update_permission_grants),
        )
        .route("/{plugin_id}/tasks", get(handlers::list_plugin_tasks))
        .route(
            "/{plugin_id}/nav-items",
            get(handlers::list_plugin_nav_items),
        )
        .route(
            "/{plugin_id}/runtime",
            get(handlers::get_plugin_runtime_handle),
        )
        .route(
            "/{plugin_id}/start",
            axum::routing::post(handlers::start_plugin_runtime),
        )
        .route(
            "/{plugin_id}/stop",
            axum::routing::post(handlers::stop_plugin_runtime),
        )
        .route(
            "/{plugin_id}/uninstall",
            axum::routing::post(handlers::uninstall_plugin),
        )
        .route("/market/catalog", get(handlers::get_market_catalog))
        .route(
            "/market/install",
            axum::routing::post(handlers::install_from_market_url),
        )
        .with_state(PluginAdminState { db })
        .layer(from_fn(
            yh_api_middlewares::jwt_auth::admin_permission_middleware,
        ))
        .layer(from_fn(yh_api_middlewares::jwt_auth::jwt_auth_middleware))
}

pub fn create_host_router(db: Arc<DatabaseConnection>) -> Router {
    crate::host_api::create_host_api_router(db)
        .layer(from_fn(yh_api_middlewares::jwt_auth::jwt_auth_middleware))
}
