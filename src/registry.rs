use crate::entities::{plugin_audit_log, plugin_registry, plugin_version};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct RegistryStats {
    pub plugin_count: u64,
    pub version_count: u64,
    pub audit_log_count: u64,
}

pub async fn list_registry(
    db: &DatabaseConnection,
) -> Result<Vec<plugin_registry::Model>, sea_orm::DbErr> {
    plugin_registry::Entity::find()
        .order_by_desc(plugin_registry::Column::UpdatedAt)
        .all(db)
        .await
}

pub async fn list_versions_by_plugin(
    db: &DatabaseConnection,
    plugin_id: &str,
) -> Result<Vec<plugin_version::Model>, sea_orm::DbErr> {
    plugin_version::Entity::find()
        .filter(plugin_version::Column::PluginId.eq(plugin_id))
        .order_by_desc(plugin_version::Column::CreatedAt)
        .all(db)
        .await
}

pub async fn list_audit_logs(
    db: &DatabaseConnection,
    limit: u64,
) -> Result<Vec<plugin_audit_log::Model>, sea_orm::DbErr> {
    plugin_audit_log::Entity::find()
        .order_by_desc(plugin_audit_log::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await
}

pub async fn collect_stats(db: &DatabaseConnection) -> Result<RegistryStats, sea_orm::DbErr> {
    Ok(RegistryStats {
        plugin_count: plugin_registry::Entity::find().count(db).await?,
        version_count: plugin_version::Entity::find().count(db).await?,
        audit_log_count: plugin_audit_log::Entity::find().count(db).await?,
    })
}

pub async fn get_registry_by_id(
    db: &DatabaseConnection,
    plugin_id: &str,
) -> Result<Option<plugin_registry::Model>, sea_orm::DbErr> {
    plugin_registry::Entity::find_by_id(plugin_id.to_string())
        .one(db)
        .await
}

pub async fn get_version_by_plugin_and_version(
    db: &DatabaseConnection,
    plugin_id: &str,
    version: &str,
) -> Result<Option<plugin_version::Model>, sea_orm::DbErr> {
    plugin_version::Entity::find()
        .filter(plugin_version::Column::PluginId.eq(plugin_id))
        .filter(plugin_version::Column::Version.eq(version))
        .one(db)
        .await
}

pub async fn mark_plugin_uninstalled(
    db: &DatabaseConnection,
    plugin_id: &str,
) -> Result<Option<plugin_registry::Model>, sea_orm::DbErr> {
    let Some(existing) = plugin_registry::Entity::find_by_id(plugin_id.to_string())
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let mut active: plugin_registry::ActiveModel = existing.into();
    active.current_version = Set(None);
    active.install_status = Set("uninstalled".to_string());
    active.enabled = Set(false);
    active.updated_at = Set(chrono::Utc::now().into());
    active.update(db).await.map(Some)
}

pub async fn update_plugin_runtime_state(
    db: &DatabaseConnection,
    plugin_id: &str,
    enabled: bool,
    install_status: &str,
) -> Result<Option<plugin_registry::Model>, sea_orm::DbErr> {
    let Some(existing) = plugin_registry::Entity::find_by_id(plugin_id.to_string())
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let mut active: plugin_registry::ActiveModel = existing.into();
    active.enabled = Set(enabled);
    active.install_status = Set(install_status.to_string());
    active.updated_at = Set(chrono::Utc::now().into());
    active.update(db).await.map(Some)
}

pub async fn delete_plugin_versions(
    db: &DatabaseConnection,
    plugin_id: &str,
) -> Result<u64, sea_orm::DbErr> {
    Ok(plugin_version::Entity::delete_many()
        .filter(plugin_version::Column::PluginId.eq(plugin_id))
        .exec(db)
        .await?
        .rows_affected)
}

pub async fn append_audit_log(
    db: &DatabaseConnection,
    plugin_id: &str,
    action: &str,
    message: String,
    actor_user_id: Option<String>,
) -> Result<plugin_audit_log::Model, sea_orm::DbErr> {
    plugin_audit_log::ActiveModel {
        id: Set(uuid::Uuid::now_v7().to_string()),
        plugin_id: Set(plugin_id.to_string()),
        action: Set(action.to_string()),
        message: Set(message),
        actor_user_id: Set(actor_user_id),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(db)
    .await
}
