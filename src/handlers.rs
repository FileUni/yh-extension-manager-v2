use crate::installer::{self, InstallPluginOptions};
use crate::manager::{PluginRuntimeStatusSnapshot, get_plugin_runtime_manager, get_runtime_status_snapshot};
use crate::market;
use crate::permissions::{self, PluginPermissionGrantItem};
use crate::registry::{self, RegistryStats};
use crate::runtime::RuntimeHandle;
use axum::{Json, extract::{Path, Request, State}, response::IntoResponse};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use utoipa::ToSchema;
use yh_config_infra::RequestContext;
use yh_system::config::get_system_config;
use yh_response::{AppError, Resp, error::ErrorCode};

#[derive(Clone)]
pub struct PluginAdminState {
    pub db: Arc<DatabaseConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginAdminStatus {
    pub runtime: PluginRuntimeStatusSnapshot,
    pub stats: RegistryStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct UpdatePluginPermissionGrantsRequest {
    pub grants: Vec<PluginPermissionGrantItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginRuntimeActionResponse {
    pub handle: RuntimeHandle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginRuntimeListResponse {
    pub runtimes: Vec<RuntimeHandle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct MarketInstallFromUrlRequest {
    pub download_url: String,
}

fn sanitize_fs_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' { ch } else { '_' })
        .collect()
}

fn map_db_error(ctx: &RequestContext, message: &str, error: sea_orm::DbErr) -> AppError {
    yh_console_log::yhlog("error", &format!("{}: {}", message, error));
    AppError::new(
        ErrorCode::InternalError,
        message.to_string(),
        ctx.request_id.to_owned(),
        ctx.client_ip.to_owned(),
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/plugins/status",
    responses((status = 200, description = "Get plugin system v2 runtime status", body = PluginAdminStatus)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn get_status(
    State(state): State<PluginAdminState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let runtime = get_runtime_status_snapshot().await.map_err(|e| {
        AppError::new(
            ErrorCode::ConfigReadFailed,
            e,
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let stats = registry::collect_stats(state.db.as_ref())
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to query plugin registry statistics", e))?;
    let data = serde_json::to_value(PluginAdminStatus { runtime, stats }).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/plugins/registry",
    responses((status = 200, description = "List installed plugin registry records", body = Vec<crate::entities::plugin_registry::Model>)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn list_registry(
    State(state): State<PluginAdminState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let rows = registry::list_registry(state.db.as_ref())
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to list plugin registry", e))?;
    let data = serde_json::to_value(rows).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/plugins/registry/{plugin_id}/versions",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "List plugin versions", body = Vec<crate::entities::plugin_version::Model>)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn list_versions(
    State(state): State<PluginAdminState>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let rows = registry::list_versions_by_plugin(state.db.as_ref(), &plugin_id)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to list plugin versions", e))?;
    let data = serde_json::to_value(rows).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/plugins/audit",
    responses((status = 200, description = "List plugin audit logs", body = Vec<crate::entities::plugin_audit_log::Model>)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn list_audit_logs(
    State(state): State<PluginAdminState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let rows = registry::list_audit_logs(state.db.as_ref(), 100)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to list plugin audit logs", e))?;
    let data = serde_json::to_value(rows).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

pub async fn install_plugin_zip(
    State(state): State<PluginAdminState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    request: Request,
) -> Result<Json<Resp>, AppError> {
    let runtime = get_runtime_status_snapshot().await.map_err(|e| {
        AppError::new(
            ErrorCode::ConfigReadFailed,
            e,
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::BadRequest,
                format!("Failed to read plugin zip request body: {}", e),
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            )
        })?;
    if body.is_empty() {
        return Err(AppError::new(
            ErrorCode::BadRequest,
            "Request body must contain plugin zip bytes",
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        ));
    }

    let zip_bytes = body.to_vec();
    let packages_root = PathBuf::from(runtime.layout.packages_dir);
    let data = installer::install_plugin_from_zip_bytes(
        state.db.as_ref(),
        &packages_root,
        &zip_bytes,
        InstallPluginOptions {
            source_kind: "sideload".to_string(),
            market_origin: None,
            actor_user_id: ctx.user_id.as_ref().map(|v| v.to_string()),
        },
    )
    .await
    .map_err(|e| {
        AppError::new(
            ErrorCode::BadRequest,
            e,
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })
    .and_then(|result| {
        serde_json::to_value(result).map_err(|e| {
            AppError::new(
                ErrorCode::SerializationFailed,
                e.to_string(),
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            )
        })
    })?;

    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/plugins/{plugin_id}/permissions",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "List plugin permission grants", body = Vec<PluginPermissionGrantItem>)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn list_permission_grants(
    State(state): State<PluginAdminState>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let grants = permissions::list_plugin_permission_grants(state.db.as_ref(), &plugin_id)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to list plugin permission grants", e))?
        .into_iter()
        .map(|grant| PluginPermissionGrantItem {
            permission_key: grant.permission_key,
            granted: grant.granted,
        })
        .collect::<Vec<_>>();
    let data = serde_json::to_value(grants).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/plugins/{plugin_id}/permissions",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    request_body = UpdatePluginPermissionGrantsRequest,
    responses((status = 200, description = "Replace plugin permission grants", body = Vec<PluginPermissionGrantItem>)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn update_permission_grants(
    State(state): State<PluginAdminState>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Json(payload): Json<UpdatePluginPermissionGrantsRequest>,
) -> Result<impl IntoResponse, AppError> {
    permissions::replace_plugin_permission_grants(state.db.as_ref(), &plugin_id, &payload.grants)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to update plugin permission grants", e))?;
    let data = serde_json::to_value(payload.grants).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/plugins/{plugin_id}/start",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "Start plugin runtime", body = PluginRuntimeActionResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn start_plugin_runtime(
    State(state): State<PluginAdminState>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let manager = get_plugin_runtime_manager().ok_or_else(|| {
        AppError::internal(
            "plugin runtime manager is not initialized",
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let plugin = registry::get_registry_by_id(state.db.as_ref(), &plugin_id)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to load plugin registry", e))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                format!("Plugin '{}' not found", plugin_id),
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            )
        })?;
    let version = plugin.current_version.as_ref().ok_or_else(|| {
        AppError::new(
            ErrorCode::BadRequest,
            format!("Plugin '{}' has no installed version", plugin_id),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let version = registry::get_version_by_plugin_and_version(state.db.as_ref(), &plugin_id, version)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to load plugin version", e))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                format!("Plugin '{}' current version record not found", plugin_id),
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            )
        })?;
    let install_root = PathBuf::from(&version.package_path);
    let manifest = installer::read_manifest_from_package_dir(&install_root)
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCode::BadRequest,
                e,
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            )
        })?;
    let handle = match &manifest.runtime {
        crate::manifest::PluginRuntimeManifest::Process(runtime_manifest) => {
            crate::runtime::process::start_process_runtime(
                &plugin_id,
                &install_root,
                runtime_manifest,
                &manager.status_snapshot().host_api_base_url,
                &manager.host_api_secret_base64(),
            )
            .await
            .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?
        }
        crate::manifest::PluginRuntimeManifest::Docker(runtime_manifest) => {
            crate::runtime::docker::start_docker_runtime(
                &plugin_id,
                &install_root,
                runtime_manifest,
                manager.docker_engine_command(),
                &manager.status_snapshot().host_api_base_url,
                &manager.host_api_secret_base64(),
            )
            .await
            .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?
        }
        crate::manifest::PluginRuntimeManifest::WasmComponent(runtime_manifest) => {
            let _ = crate::runtime::wasm::prepare_wasm_runtime(
                &plugin_id,
                &install_root,
                runtime_manifest,
                "wasm-component",
            )
            .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?;
            return Err(AppError::new(
                ErrorCode::BadRequest,
                "wasm runtime executor is not implemented yet; installation and validation are available, but start is not supported yet",
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            ));
        }
        crate::manifest::PluginRuntimeManifest::WasmModule(runtime_manifest) => {
            let _ = crate::runtime::wasm::prepare_wasm_runtime(
                &plugin_id,
                &install_root,
                runtime_manifest,
                "wasm-module",
            )
            .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?;
            return Err(AppError::new(
                ErrorCode::BadRequest,
                "wasm runtime executor is not implemented yet; installation and validation are available, but start is not supported yet",
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            ));
        }
    };
    manager.set_runtime_handle(&plugin_id, handle.clone());
    let _ = registry::update_plugin_runtime_state(state.db.as_ref(), &plugin_id, true, "running").await;
    let _ = registry::append_audit_log(
        state.db.as_ref(),
        &plugin_id,
        "start",
        format!("Started plugin {}", plugin_id),
        ctx.user_id.as_ref().map(|v| v.to_string()),
    )
    .await;
    let data = serde_json::to_value(PluginRuntimeActionResponse { handle }).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/plugins/runtimes",
    responses((status = 200, description = "List active plugin runtimes", body = PluginRuntimeListResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn list_active_runtimes(
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let manager = get_plugin_runtime_manager().ok_or_else(|| {
        AppError::internal(
            "plugin runtime manager is not initialized",
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let data = serde_json::to_value(PluginRuntimeListResponse {
        runtimes: manager.list_runtime_handles(),
    })
    .map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/plugins/{plugin_id}/runtime",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "Get active plugin runtime handle", body = PluginRuntimeActionResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn get_plugin_runtime_handle(
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let manager = get_plugin_runtime_manager().ok_or_else(|| {
        AppError::internal(
            "plugin runtime manager is not initialized",
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let handle = manager.get_runtime_handle(&plugin_id).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            format!("Plugin '{}' runtime is not active", plugin_id),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let data = serde_json::to_value(PluginRuntimeActionResponse { handle }).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/plugins/{plugin_id}/stop",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "Stop plugin runtime", body = PluginRuntimeActionResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn stop_plugin_runtime(
    State(state): State<PluginAdminState>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let manager = get_plugin_runtime_manager().ok_or_else(|| {
        AppError::internal(
            "plugin runtime manager is not initialized",
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let handle = manager.remove_runtime_handle(&plugin_id).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            format!("Plugin '{}' runtime is not running", plugin_id),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    match handle.runtime_kind.as_str() {
        "process" => crate::runtime::process::stop_process_runtime(&handle)
            .await
            .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?,
        "docker" => crate::runtime::docker::stop_docker_runtime(&handle, manager.docker_engine_command())
            .await
            .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?,
        _ => {}
    }
    let _ = registry::update_plugin_runtime_state(state.db.as_ref(), &plugin_id, false, "installed").await;
    let _ = registry::append_audit_log(
        state.db.as_ref(),
        &plugin_id,
        "stop",
        format!("Stopped plugin {}", plugin_id),
        ctx.user_id.as_ref().map(|v| v.to_string()),
    )
    .await;
    let data = serde_json::to_value(PluginRuntimeActionResponse { handle }).map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/plugins/{plugin_id}/uninstall",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "Uninstall plugin", body = PluginRuntimeActionResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn uninstall_plugin(
    State(state): State<PluginAdminState>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(manager) = get_plugin_runtime_manager()
        && let Some(handle) = manager.remove_runtime_handle(&plugin_id)
    {
        match handle.runtime_kind.as_str() {
            "process" => crate::runtime::process::stop_process_runtime(&handle)
                .await
                .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?,
            "docker" => crate::runtime::docker::stop_docker_runtime(&handle, manager.docker_engine_command())
                .await
                .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?,
            _ => {}
        }
    }

    if let Some(system_cfg) = get_system_config() {
        let temp_dir = system_cfg.read().await.system.get_temp_dir().to_string();
        let plugin_component = sanitize_fs_component(&plugin_id);
        let sqlite_root = PathBuf::from(temp_dir).join("extension").join("sqlite").join(&plugin_component);
        if tokio::fs::try_exists(&sqlite_root).await.unwrap_or(false) {
            let _ = tokio::fs::remove_dir_all(&sqlite_root).await;
        }
    }

    let plugin = registry::get_registry_by_id(state.db.as_ref(), &plugin_id)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to load plugin registry", e))?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                format!("Plugin '{}' not found", plugin_id),
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            )
        })?;

    if let Some(version) = plugin.current_version.as_ref()
        && let Some(version_row) = registry::get_version_by_plugin_and_version(
            state.db.as_ref(),
            &plugin_id,
            version,
        )
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to load plugin version", e))?
    {
        let package_dir = PathBuf::from(version_row.package_path.clone());
        if tokio::fs::try_exists(&package_dir).await.unwrap_or(false) {
            let _ = tokio::fs::remove_dir_all(&package_dir).await;
        }
    }

    let _ = permissions::delete_plugin_permission_grants(state.db.as_ref(), &plugin_id).await;
    let _ = registry::delete_plugin_versions(state.db.as_ref(), &plugin_id).await;
    let _ = registry::mark_plugin_uninstalled(state.db.as_ref(), &plugin_id).await;
    let _ = registry::append_audit_log(
        state.db.as_ref(),
        &plugin_id,
        "uninstall",
        format!("Uninstalled plugin {}", plugin_id),
        ctx.user_id.as_ref().map(|v| v.to_string()),
    )
    .await;

    let data = serde_json::to_value(PluginRuntimeActionResponse {
        handle: RuntimeHandle {
            plugin_id,
            runtime_kind: plugin.runtime_kind,
            status: crate::runtime::RuntimeStatus::Stopped,
            detail: String::new(),
            pid: None,
            instance_ref: None,
            route_base_url: None,
        },
    })
    .map_err(|e| {
        AppError::new(
            ErrorCode::SerializationFailed,
            e.to_string(),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, data)))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/plugins/market/catalog",
    responses((status = 200, description = "Fetch public plugin market catalog", body = serde_json::Value)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn get_market_catalog(
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let catalog = market::fetch_catalog().await.map_err(|e| {
        AppError::new(
            ErrorCode::InternalError,
            e,
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, catalog)))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/plugins/market/install",
    request_body = MarketInstallFromUrlRequest,
    responses((status = 200, description = "Install plugin from market download URL", body = serde_json::Value)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn install_from_market_url(
    State(state): State<PluginAdminState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Json(payload): Json<MarketInstallFromUrlRequest>,
) -> Result<impl IntoResponse, AppError> {
    let runtime = get_runtime_status_snapshot().await.map_err(|e| {
        AppError::new(
            ErrorCode::ConfigReadFailed,
            e,
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let packages_root = PathBuf::from(runtime.layout.packages_dir);
    let result = market::install_from_download_url(
        state.db.as_ref(),
        &packages_root,
        market::MarketInstallFromUrlRequest {
            download_url: payload.download_url,
            actor_user_id: ctx.user_id.as_ref().map(|v| v.to_string()),
        },
    )
    .await
    .map_err(|e| AppError::new(
        ErrorCode::BadRequest,
        e,
        ctx.request_id.to_owned(),
        ctx.client_ip.to_owned(),
    ))?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, result)))
}
