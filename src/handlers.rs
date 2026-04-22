use crate::installer::{self, InstallPluginOptions};
use crate::manager::{
    PluginRuntimeStatusSnapshot, get_plugin_runtime_manager, get_runtime_status_snapshot,
};
use crate::market;
use crate::permissions::{self, PluginPermissionGrantItem};
use crate::registry::{self, RegistryStats};
use crate::runtime::RuntimeHandle;
use axum::{
    Json,
    extract::{Path, Request, State},
    response::IntoResponse,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use utoipa::ToSchema;
use yh_config_infra::RequestContext;
use yh_response::{AppError, Resp, error::ErrorCode};
use yh_system::config::get_system_config;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct PluginTaskListResponse {
    pub tasks: Vec<crate::entities::plugin_task::Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct PluginNavItemListResponse {
    pub items: Vec<crate::entities::plugin_nav_item::Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct MarketInstallFromUrlRequest {
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginConfigResponse {
    pub plugin_id: String,
    pub config_dir: String,
    pub config_file: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct UpdatePluginConfigRequest {
    pub content: String,
}

fn sanitize_fs_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

async fn stop_lingering_runtime_processes(runtime_dir: &std::path::Path) {
    let runtime_dir_str = runtime_dir.to_string_lossy().to_string();
    let status = tokio::process::Command::new("pkill")
        .arg("-f")
        .arg(&runtime_dir_str)
        .status()
        .await;
    match status {
        Ok(exit_status) if exit_status.success() => {}
        Ok(exit_status) if exit_status.code() == Some(1) => {}
        Ok(exit_status) => {
            yh_console_log::yhlog(
                "warn",
                &format!(
                    "failed to stop lingering plugin runtime processes under '{}': {}",
                    runtime_dir.display(),
                    exit_status
                ),
            );
        }
        Err(error) => {
            yh_console_log::yhlog(
                "warn",
                &format!(
                    "failed to invoke pkill for lingering plugin runtime processes under '{}': {}",
                    runtime_dir.display(),
                    error
                ),
            );
        }
    }
}

fn default_nav_icon(plugin_id: &str) -> &'static str {
    if plugin_id.contains("chat") {
        "MessageSquare"
    } else if plugin_id.contains("email") {
        "Mail"
    } else if plugin_id.contains("todo") {
        "ListTodo"
    } else {
        "PlugZap"
    }
}

fn default_nav_position(plugin_id: &str) -> &'static str {
    let _ = plugin_id;
    "sidebar"
}

async fn ensure_default_nav_item(
    db: &DatabaseConnection,
    plugin_id: &str,
    manifest: &crate::manifest::PluginManifest,
) {
    let Some(ui) = &manifest.ui else {
        return;
    };
    let route = ui
        .mount_path
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/".to_string());
    let _ = registry::upsert_plugin_nav_item(
        db,
        registry::UpsertNavItemInput {
            plugin_id: plugin_id.to_string(),
            item_key: "main".to_string(),
            label: manifest.name.clone(),
            route,
            icon: default_nav_icon(plugin_id).to_string(),
            visibility: "user".to_string(),
            group_key: Some("plugins".to_string()),
            position: Some(default_nav_position(plugin_id).to_string()),
            required_permission: None,
            sort_order: 100,
        },
    )
    .await;
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
                format!("Failed to read plugin package request body: {}", e),
                ctx.request_id.to_owned(),
                ctx.client_ip.to_owned(),
            )
        })?;
    if body.is_empty() {
        return Err(AppError::new(
            ErrorCode::BadRequest,
            "Request body must contain plugin package bytes",
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
    get,
    path = "/api/v1/admin/plugins/{plugin_id}/tasks",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "List plugin task governance records", body = PluginTaskListResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn list_plugin_tasks(
    State(state): State<PluginAdminState>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let tasks = registry::list_plugin_tasks(state.db.as_ref(), &plugin_id)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to list plugin tasks", e))?;
    let data = serde_json::to_value(PluginTaskListResponse { tasks }).map_err(|e| {
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
    path = "/api/v1/admin/plugins/{plugin_id}/nav-items",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "List plugin navigation items", body = PluginNavItemListResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn list_plugin_nav_items(
    State(state): State<PluginAdminState>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    let items = registry::list_plugin_nav_items(state.db.as_ref(), &plugin_id)
        .await
        .map_err(|e| map_db_error(&ctx, "Failed to list plugin nav items", e))?;
    let data = serde_json::to_value(PluginNavItemListResponse { items }).map_err(|e| {
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
    path = "/api/v1/admin/plugins/{plugin_id}/config",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    responses((status = 200, description = "Get plugin config file", body = PluginConfigResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn get_plugin_config(
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
    let (config_dir, config_file) = manager
        .ensure_plugin_config_paths(&plugin_id)
        .await
        .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?;
    let content = tokio::fs::read_to_string(&config_file)
        .await
        .unwrap_or_else(|_| String::new());
    let data = serde_json::to_value(PluginConfigResponse {
        plugin_id,
        config_dir,
        config_file,
        content,
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
    post,
    path = "/api/v1/admin/plugins/{plugin_id}/config",
    params(("plugin_id" = String, Path, description = "Plugin ID")),
    request_body = UpdatePluginConfigRequest,
    responses((status = 200, description = "Update plugin config file", body = PluginConfigResponse)),
    security(("jwt" = [])),
    tag = "Plugins V2"
)]
pub async fn update_plugin_config(
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Json(payload): Json<UpdatePluginConfigRequest>,
) -> Result<impl IntoResponse, AppError> {
    let manager = get_plugin_runtime_manager().ok_or_else(|| {
        AppError::internal(
            "plugin runtime manager is not initialized",
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let (config_dir, config_file) = manager
        .ensure_plugin_config_paths(&plugin_id)
        .await
        .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?;
    tokio::fs::write(&config_file, payload.content.as_bytes())
        .await
        .map_err(|e| AppError::internal(format!("failed to write plugin config: {}", e), ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?;
    let data = serde_json::to_value(PluginConfigResponse {
        plugin_id,
        config_dir,
        config_file,
        content: payload.content,
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
    let version =
        registry::get_version_by_plugin_and_version(state.db.as_ref(), &plugin_id, version)
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
    let (plugin_config_dir, plugin_config_file) = manager
        .ensure_plugin_config_paths(&plugin_id)
        .await
        .map_err(|e| AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned()))?;
    let handle = match &manifest.runtime {
        crate::manifest::PluginRuntimeManifest::Process(runtime_manifest) => {
            crate::runtime::process::start_process_runtime(
                &plugin_id,
                &install_root,
                runtime_manifest,
                &manager.status_snapshot().host_api_base_url,
                &manager.host_api_secret_base64(),
                &plugin_config_dir,
                &plugin_config_file,
            )
            .await
            .map_err(|e| {
                AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
            })?
        }
        crate::manifest::PluginRuntimeManifest::Docker(runtime_manifest) => {
            crate::runtime::docker::start_docker_runtime(
                &plugin_id,
                &install_root,
                runtime_manifest,
                &crate::runtime::docker::DockerRuntimeLaunchContext {
                    docker_engine_command: manager.docker_engine_command(),
                    host_api_base_url: &manager.status_snapshot().host_api_base_url,
                    host_api_token: &manager.host_api_secret_base64(),
                    plugin_config_dir: &plugin_config_dir,
                    plugin_config_file: &plugin_config_file,
                },
            )
            .await
            .map_err(|e| {
                AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
            })?
        }
        crate::manifest::PluginRuntimeManifest::WasmComponent(runtime_manifest) => {
            crate::runtime::wasm::start_wasm_component_runtime(
                &plugin_id,
                &install_root,
                runtime_manifest,
                &manager.status_snapshot().host_api_base_url,
                &manager.host_api_secret_base64(),
                &plugin_config_dir,
                &plugin_config_file,
            )
            .await
            .map_err(|e| {
                AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
            })?
        }
        crate::manifest::PluginRuntimeManifest::WasmModule(runtime_manifest) => {
            crate::runtime::wasm::start_wasm_module_runtime(
                &plugin_id,
                &install_root,
                runtime_manifest,
                &manager.status_snapshot().host_api_base_url,
                &manager.host_api_secret_base64(),
                &plugin_config_dir,
                &plugin_config_file,
            )
            .await
            .map_err(|e| {
                AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
            })?
        }
    };
    manager.set_runtime_handle(&plugin_id, handle.clone());
    ensure_default_nav_item(state.db.as_ref(), &plugin_id, &manifest).await;
    let _ =
        registry::update_plugin_runtime_state(state.db.as_ref(), &plugin_id, true, "running").await;
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
            .map_err(|e| {
                AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
            })?,
        "docker" => {
            crate::runtime::docker::stop_docker_runtime(&handle, manager.docker_engine_command())
                .await
                .map_err(|e| {
                    AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
                })?
        }
        "wasm-component" => crate::runtime::wasm::stop_wasm_runtime(&handle)
            .await
            .map_err(|e| {
                AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
            })?,
        "wasm-module" => crate::runtime::wasm::stop_wasm_runtime(&handle)
            .await
            .map_err(|e| {
                AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
            })?,
        _ => {}
    }
    let _ =
        registry::update_plugin_runtime_state(state.db.as_ref(), &plugin_id, false, "installed")
            .await;
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
                .map_err(|e| {
                    AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
                })?,
            "docker" => crate::runtime::docker::stop_docker_runtime(
                &handle,
                manager.docker_engine_command(),
            )
            .await
            .map_err(|e| {
                AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
            })?,
            "wasm-component" => crate::runtime::wasm::stop_wasm_runtime(&handle)
                .await
                .map_err(|e| {
                    AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
                })?,
            "wasm-module" => crate::runtime::wasm::stop_wasm_runtime(&handle)
                .await
                .map_err(|e| {
                    AppError::internal(e, ctx.request_id.to_owned(), ctx.client_ip.to_owned())
                })?,
            _ => {}
        }
    }

    if let Some(system_cfg) = get_system_config() {
        let temp_dir = system_cfg.read().await.system.get_temp_dir().to_string();
        let plugin_component = sanitize_fs_component(&plugin_id);
        let sqlite_root = PathBuf::from(temp_dir)
            .join("extension")
            .join("sqlite")
            .join(&plugin_component);
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
        && let Some(version_row) =
            registry::get_version_by_plugin_and_version(state.db.as_ref(), &plugin_id, version)
                .await
                .map_err(|e| map_db_error(&ctx, "Failed to load plugin version", e))?
    {
        let package_dir = PathBuf::from(version_row.package_path.clone());
        stop_lingering_runtime_processes(&package_dir.join("runtime")).await;
        if tokio::fs::try_exists(&package_dir).await.unwrap_or(false) {
            let _ = tokio::fs::remove_dir_all(&package_dir).await;
        }
    }

    if let Some(manager) = get_plugin_runtime_manager() {
        let layout = manager.status_snapshot().layout;
        let plugin_component = sanitize_fs_component(&plugin_id);
        for dir in [
            PathBuf::from(layout.packages_dir).join(&plugin_component),
            PathBuf::from(layout.config_dir).join(&plugin_component),
            PathBuf::from(layout.state_dir).join(&plugin_component),
            PathBuf::from(layout.logs_dir).join(&plugin_component),
            PathBuf::from(layout.runtime_dir).join(&plugin_component),
            PathBuf::from(layout.shared_dir).join(&plugin_component),
        ] {
            if tokio::fs::try_exists(&dir).await.unwrap_or(false) {
                let _ = tokio::fs::remove_dir_all(&dir).await;
            }
        }
    }

    let _ = registry::delete_plugin_permission_grants_all(state.db.as_ref(), &plugin_id).await;
    let _ = registry::delete_plugin_versions(state.db.as_ref(), &plugin_id).await;
    let _ = registry::delete_plugin_tasks(state.db.as_ref(), &plugin_id).await;
    let _ = registry::delete_plugin_nav_items(state.db.as_ref(), &plugin_id).await;
    let _ = registry::delete_plugin_shared_records(state.db.as_ref(), &plugin_id).await;
    let _ = registry::delete_plugin_migration_states(state.db.as_ref(), &plugin_id).await;
    let _ = registry::delete_plugin_registry(state.db.as_ref(), &plugin_id).await;
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
    .map_err(|e| {
        AppError::new(
            ErrorCode::BadRequest,
            e,
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(Json(Resp::ok(ctx.request_id, ctx.client_ip, result)))
}
