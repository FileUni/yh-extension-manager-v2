use axum::{Json, Router, extract::{Path, Query, State}, routing::{delete, get, post}};
use bytes::Bytes;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use utoipa::ToSchema;
use yh_config_infra::RequestContext;
use yh_system::config::get_system_config;
use yh_filemanager_vfs_storage_hub::vfs::traits::VfsStorage;
use yh_response::{AppError, Resp};

#[derive(Clone)]
pub struct HostApiState {
    pub db: Arc<DatabaseConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostIdentityResponse {
    pub user_id: String,
    pub username: Option<String>,
    pub role_id: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostPermissionCheckRequest {
    pub permission_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostPermissionCheckResponse {
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostKvSetRequest {
    pub key: String,
    pub value: String,
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostKvGetResponse {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostKvDeleteResponse {
    pub key: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostVfsWriteTextRequest {
    pub logical_path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostVfsReadTextQuery {
    pub logical_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostVfsReadTextResponse {
    pub logical_path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostUserLookupResponse {
    pub user_id: String,
    pub username: String,
    pub role_id: i16,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostDbInfoResponse {
    pub connection_available: bool,
    pub access_mode: String,
    pub recommended_table_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostSqliteEnsureRequest {
    pub plugin_id: String,
    pub database_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostSqliteEnsureResponse {
    pub plugin_id: String,
    pub database_name: String,
    pub logical_path: String,
    pub physical_path: String,
    pub dsn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostSharedRecordUpsertRequest {
    pub plugin_id: String,
    pub collection: String,
    pub record_key: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostSharedRecordResponse {
    pub plugin_id: String,
    pub collection: String,
    pub record_key: String,
    pub owner_user_id: Option<String>,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostSharedRecordQuery {
    pub plugin_id: String,
    pub collection: String,
    pub record_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostSharedRecordListQuery {
    pub plugin_id: String,
    pub collection: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostSharedRecordListResponse {
    pub records: Vec<HostSharedRecordResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct HostSharedRecordDeleteResponse {
    pub deleted: bool,
}

fn require_user(ctx: &RequestContext) -> Result<(&str, i16, Option<&str>), AppError> {
    let user = ctx.user_info.as_ref().ok_or_else(|| {
        AppError::unauthorized(
            "plugin host API requires authenticated user context",
            Arc::clone(&ctx.request_id),
            Arc::clone(&ctx.client_ip),
        )
    })?;
    Ok((user.user_id.as_ref(), user.role_id, user.username.as_deref()))
}

fn internal_error(ctx: &RequestContext, message: impl Into<String>) -> AppError {
    AppError::internal(message, Arc::clone(&ctx.request_id), Arc::clone(&ctx.client_ip))
}

fn sanitize_sqlite_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' { ch } else { '_' })
        .collect()
}

fn validate_host_plugin_identifier(value: &str, field_name: &str, ctx: &RequestContext) -> Result<String, AppError> {
    let sanitized = sanitize_sqlite_component(value);
    if sanitized.trim().is_empty() {
        return Err(AppError::new(
            yh_response::error::ErrorCode::BadRequest,
            format!("{} cannot be empty", field_name),
            Arc::clone(&ctx.request_id),
            Arc::clone(&ctx.client_ip),
        ));
    }
    Ok(sanitized)
}

async fn create_user_scoped_engine(
    state: &HostApiState,
    ctx: &RequestContext,
) -> Result<Arc<dyn VfsStorage>, AppError> {
    let (user_id, role_id, _) = require_user(ctx)?;
    let hub = yh_filemanager_vfs_storage_hub::vfs::hub::get_vfs_storage_hub()
        .cloned()
        .ok_or_else(|| internal_error(ctx, "vfs storage hub is not initialized"))?;
    hub.create_user_storage(Arc::clone(&state.db), user_id, &role_id.to_string(), None)
        .await
        .map_err(|e| internal_error(ctx, format!("failed to create scoped vfs storage: {}", e)))
}

#[utoipa::path(get, path = "/api/v1/plugin-host/identity", tag = "Plugins V2")]
pub async fn get_identity(
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<Json<Resp>, AppError> {
    let (user_id, role_id, username) = require_user(&ctx)?;
    let payload = HostIdentityResponse {
        user_id: user_id.to_string(),
        username: username.map(ToOwned::to_owned),
        role_id,
    };
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(payload).map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(get, path = "/api/v1/plugin-host/users/{user_id}", tag = "Plugins V2")]
pub async fn get_user_by_id(
    State(state): State<HostApiState>,
    Path(user_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<Json<Resp>, AppError> {
    let _ = require_user(&ctx)?;
    let parsed = uuid::Uuid::parse_str(&user_id).map_err(|e| {
        AppError::new(
            yh_response::error::ErrorCode::BadRequest,
            format!("invalid user id: {}", e),
            Arc::clone(&ctx.request_id),
            Arc::clone(&ctx.client_ip),
        )
    })?;
    let user = yh_user_center::services::user_service::UserService::get_user_by_id(
        state.db.as_ref(),
        parsed,
    )
    .await
    .map_err(|e| internal_error(&ctx, format!("failed to query user: {}", e)))?
    .ok_or_else(|| {
        AppError::new(
            yh_response::error::ErrorCode::NotFound,
            format!("user '{}' not found", user_id),
            Arc::clone(&ctx.request_id),
            Arc::clone(&ctx.client_ip),
        )
    })?;
    let payload = HostUserLookupResponse {
        user_id,
        username: user.username,
        role_id: user.role_id,
        status: user.status,
    };
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(payload).map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(post, path = "/api/v1/plugin-host/auth/has-permission", tag = "Plugins V2")]
pub async fn check_permission(
    State(state): State<HostApiState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Json(payload): Json<HostPermissionCheckRequest>,
) -> Result<Json<Resp>, AppError> {
    let (user_id, role_id, _) = require_user(&ctx)?;
    let allowed = yh_user_center::services::user_service::UserService::has_permission(
        state.db.as_ref(),
        user_id,
        role_id,
        &payload.permission_key,
    )
    .await
    .map_err(|e| internal_error(&ctx, format!("permission check failed: {}", e)))?;
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(HostPermissionCheckResponse { allowed })
            .map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(post, path = "/api/v1/plugin-host/kv/set", tag = "Plugins V2")]
pub async fn set_kv(
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Json(payload): Json<HostKvSetRequest>,
) -> Result<Json<Resp>, AppError> {
    let _ = require_user(&ctx)?;
    yh_fast_kv_storage_hub::api::helpers::set(
        &payload.key,
        Bytes::from(payload.value.into_bytes()),
        payload.ttl_secs,
    )
    .await
    .map_err(|e| internal_error(&ctx, format!("kv set failed: {}", e)))?;
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::json!({ "ok": true }),
    )))
}

#[utoipa::path(get, path = "/api/v1/plugin-host/kv/{key}", tag = "Plugins V2")]
pub async fn get_kv(
    axum::extract::Path(key): axum::extract::Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<Json<Resp>, AppError> {
    let _ = require_user(&ctx)?;
    let value = yh_fast_kv_storage_hub::api::helpers::get(&key)
        .await
        .map_err(|e| internal_error(&ctx, format!("kv get failed: {}", e)))?
        .map(|bytes| String::from_utf8_lossy(bytes.as_ref()).to_string());
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(HostKvGetResponse { key, value })
            .map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(delete, path = "/api/v1/plugin-host/kv/{key}", tag = "Plugins V2")]
pub async fn delete_kv(
    Path(key): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<Json<Resp>, AppError> {
    let _ = require_user(&ctx)?;
    let deleted = yh_fast_kv_storage_hub::api::helpers::del(&key)
        .await
        .map_err(|e| internal_error(&ctx, format!("kv delete failed: {}", e)))?;
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(HostKvDeleteResponse { key, deleted })
            .map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(get, path = "/api/v1/plugin-host/vfs/read-text", tag = "Plugins V2")]
pub async fn read_vfs_text(
    State(state): State<HostApiState>,
    Query(query): Query<HostVfsReadTextQuery>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<Json<Resp>, AppError> {
    let storage = create_user_scoped_engine(&state, &ctx).await?;
    let (bytes, _) = storage
        .read(&query.logical_path)
        .await
        .map_err(|e| internal_error(&ctx, format!("vfs read failed: {}", e)))?;
    let payload = HostVfsReadTextResponse {
        logical_path: query.logical_path,
        content: String::from_utf8_lossy(bytes.as_ref()).to_string(),
    };
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(payload).map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(post, path = "/api/v1/plugin-host/vfs/write-text", tag = "Plugins V2")]
pub async fn write_vfs_text(
    State(state): State<HostApiState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Json(payload): Json<HostVfsWriteTextRequest>,
) -> Result<Json<Resp>, AppError> {
    let storage = create_user_scoped_engine(&state, &ctx).await?;
    storage
        .write(&payload.logical_path, Bytes::from(payload.content.into_bytes()))
        .await
        .map_err(|e| internal_error(&ctx, format!("vfs write failed: {}", e)))?;
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::json!({ "ok": true, "path": payload.logical_path }),
    )))
}

#[utoipa::path(get, path = "/api/v1/plugin-host/db/info", tag = "Plugins V2")]
pub async fn get_db_info(
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<Json<Resp>, AppError> {
    let (user_id, _, _) = require_user(&ctx)?;
    let prefix_seed = user_id.chars().take(8).collect::<String>();
    let payload = HostDbInfoResponse {
        connection_available: true,
        access_mode: "shared-broker".to_string(),
        recommended_table_prefix: format!("yh_plg_{}", prefix_seed),
    };
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(payload).map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(post, path = "/api/v1/plugin-host/db/sqlite/ensure", tag = "Plugins V2")]
pub async fn ensure_sqlite_database(
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Json(payload): Json<HostSqliteEnsureRequest>,
) -> Result<Json<Resp>, AppError> {
    let (user_id, _, _) = require_user(&ctx)?;
    let system_cfg = get_system_config().ok_or_else(|| {
        internal_error(&ctx, "system config manager is not initialized")
    })?;
    let temp_dir = system_cfg.read().await.system.get_temp_dir().to_string();
    let plugin_dir = sanitize_sqlite_component(&payload.plugin_id);
    let db_name = sanitize_sqlite_component(&payload.database_name);
    let logical_path = format!("/.plugins/{}/sqlite/{}/{}.db", plugin_dir, user_id, db_name);
    let physical_dir = PathBuf::from(&temp_dir).join("extension").join("sqlite").join(&plugin_dir).join(user_id);
    tokio::fs::create_dir_all(&physical_dir)
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to create sqlite broker dir: {}", e)))?;
    let physical_path = physical_dir.join(format!("{}.db", db_name));
    if !tokio::fs::try_exists(&physical_path)
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to check sqlite broker file: {}", e)))?
    {
        let _ = tokio::fs::File::create(&physical_path)
            .await
            .map_err(|e| internal_error(&ctx, format!("failed to create sqlite broker file: {}", e)))?;
    }
    let dsn = format!("sqlite://{}?mode=rwc", physical_path.to_string_lossy());
    let payload = HostSqliteEnsureResponse {
        plugin_id: payload.plugin_id,
        database_name: payload.database_name,
        logical_path,
        physical_path: physical_path.to_string_lossy().to_string(),
        dsn,
    };
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(payload).map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(post, path = "/api/v1/plugin-host/db/shared/upsert", tag = "Plugins V2")]
pub async fn upsert_shared_record(
    State(state): State<HostApiState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Json(payload): Json<HostSharedRecordUpsertRequest>,
) -> Result<Json<Resp>, AppError> {
    let (user_id, _, _) = require_user(&ctx)?;
    let plugin_id = validate_host_plugin_identifier(&payload.plugin_id, "plugin_id", &ctx)?;
    let collection = validate_host_plugin_identifier(&payload.collection, "collection", &ctx)?;
    let record_key = validate_host_plugin_identifier(&payload.record_key, "record_key", &ctx)?;

    let existing = crate::entities::plugin_shared_record::Entity::find()
        .filter(crate::entities::plugin_shared_record::Column::PluginId.eq(plugin_id.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::Collection.eq(collection.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::RecordKey.eq(record_key.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::OwnerUserId.eq(user_id))
        .one(state.db.as_ref())
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to query shared record: {}", e)))?;

    let now = chrono::Utc::now();
    let model = if let Some(existing) = existing {
        let mut active: crate::entities::plugin_shared_record::ActiveModel = existing.into();
        active.payload_json = Set(payload.payload_json.clone());
        active.updated_at = Set(now.into());
        active.update(state.db.as_ref())
            .await
            .map_err(|e| internal_error(&ctx, format!("failed to update shared record: {}", e)))?
    } else {
        crate::entities::plugin_shared_record::ActiveModel {
            id: Set(uuid::Uuid::now_v7().to_string()),
            plugin_id: Set(plugin_id.clone()),
            collection: Set(collection.clone()),
            record_key: Set(record_key.clone()),
            owner_user_id: Set(Some(user_id.to_string())),
            payload_json: Set(payload.payload_json.clone()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        }
        .insert(state.db.as_ref())
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to insert shared record: {}", e)))?
    };

    let response = HostSharedRecordResponse {
        plugin_id: model.plugin_id,
        collection: model.collection,
        record_key: model.record_key,
        owner_user_id: model.owner_user_id,
        payload_json: model.payload_json,
    };
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(response).map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(get, path = "/api/v1/plugin-host/db/shared/get", tag = "Plugins V2")]
pub async fn get_shared_record(
    State(state): State<HostApiState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Query(query): Query<HostSharedRecordQuery>,
) -> Result<Json<Resp>, AppError> {
    let (user_id, _, _) = require_user(&ctx)?;
    let plugin_id = validate_host_plugin_identifier(&query.plugin_id, "plugin_id", &ctx)?;
    let collection = validate_host_plugin_identifier(&query.collection, "collection", &ctx)?;
    let record_key = validate_host_plugin_identifier(&query.record_key, "record_key", &ctx)?;
    let model = crate::entities::plugin_shared_record::Entity::find()
        .filter(crate::entities::plugin_shared_record::Column::PluginId.eq(plugin_id.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::Collection.eq(collection.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::RecordKey.eq(record_key.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::OwnerUserId.eq(user_id))
        .one(state.db.as_ref())
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to query shared record: {}", e)))?
        .ok_or_else(|| AppError::new(
            yh_response::error::ErrorCode::NotFound,
            "shared record not found",
            Arc::clone(&ctx.request_id),
            Arc::clone(&ctx.client_ip),
        ))?;
    let response = HostSharedRecordResponse {
        plugin_id: model.plugin_id,
        collection: model.collection,
        record_key: model.record_key,
        owner_user_id: model.owner_user_id,
        payload_json: model.payload_json,
    };
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(response).map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(get, path = "/api/v1/plugin-host/db/shared/list", tag = "Plugins V2")]
pub async fn list_shared_records(
    State(state): State<HostApiState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Query(query): Query<HostSharedRecordListQuery>,
) -> Result<Json<Resp>, AppError> {
    let (user_id, _, _) = require_user(&ctx)?;
    let plugin_id = validate_host_plugin_identifier(&query.plugin_id, "plugin_id", &ctx)?;
    let collection = validate_host_plugin_identifier(&query.collection, "collection", &ctx)?;
    let records = crate::entities::plugin_shared_record::Entity::find()
        .filter(crate::entities::plugin_shared_record::Column::PluginId.eq(plugin_id.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::Collection.eq(collection.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::OwnerUserId.eq(user_id))
        .all(state.db.as_ref())
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to list shared records: {}", e)))?
        .into_iter()
        .map(|record| HostSharedRecordResponse {
            plugin_id: record.plugin_id,
            collection: record.collection,
            record_key: record.record_key,
            owner_user_id: record.owner_user_id,
            payload_json: record.payload_json,
        })
        .collect::<Vec<_>>();
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(HostSharedRecordListResponse { records })
            .map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

#[utoipa::path(delete, path = "/api/v1/plugin-host/db/shared/delete", tag = "Plugins V2")]
pub async fn delete_shared_record(
    State(state): State<HostApiState>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    Query(query): Query<HostSharedRecordQuery>,
) -> Result<Json<Resp>, AppError> {
    let (user_id, _, _) = require_user(&ctx)?;
    let plugin_id = validate_host_plugin_identifier(&query.plugin_id, "plugin_id", &ctx)?;
    let collection = validate_host_plugin_identifier(&query.collection, "collection", &ctx)?;
    let record_key = validate_host_plugin_identifier(&query.record_key, "record_key", &ctx)?;
    let deleted = crate::entities::plugin_shared_record::Entity::delete_many()
        .filter(crate::entities::plugin_shared_record::Column::PluginId.eq(plugin_id.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::Collection.eq(collection.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::RecordKey.eq(record_key.as_str()))
        .filter(crate::entities::plugin_shared_record::Column::OwnerUserId.eq(user_id))
        .exec(state.db.as_ref())
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to delete shared record: {}", e)))?
        .rows_affected
        > 0;
    Ok(Json(Resp::ok(
        Arc::clone(&ctx.request_id),
        Arc::clone(&ctx.client_ip),
        serde_json::to_value(HostSharedRecordDeleteResponse { deleted })
            .map_err(|e| internal_error(&ctx, e.to_string()))?,
    )))
}

pub fn create_host_api_router(db: Arc<DatabaseConnection>) -> Router {
    Router::new()
        .route("/identity", get(get_identity))
        .route("/users/{user_id}", get(get_user_by_id))
        .route("/auth/has-permission", post(check_permission))
        .route("/kv/set", post(set_kv))
        .route("/kv/{key}", get(get_kv))
        .route("/kv/{key}", delete(delete_kv))
        .route("/vfs/read-text", get(read_vfs_text))
        .route("/vfs/write-text", post(write_vfs_text))
        .route("/db/info", get(get_db_info))
        .route("/db/sqlite/ensure", post(ensure_sqlite_database))
        .route("/db/shared/upsert", post(upsert_shared_record))
        .route("/db/shared/get", get(get_shared_record))
        .route("/db/shared/list", get(list_shared_records))
        .route("/db/shared/delete", delete(delete_shared_record))
        .with_state(HostApiState { db })
}
