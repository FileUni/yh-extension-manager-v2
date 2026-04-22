use crate::manifest::PluginPermission;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginPermissionGrantItem {
    pub permission_key: String,
    pub granted: bool,
}

pub fn permission_keys_to_items(
    permissions: &[PluginPermission],
    granted_keys: &[String],
) -> Vec<PluginPermissionGrantItem> {
    permissions
        .iter()
        .map(|permission| {
            let key = permission.as_key().to_string();
            PluginPermissionGrantItem {
                permission_key: key.clone(),
                granted: granted_keys.iter().any(|value| value == &key),
            }
        })
        .collect()
}

pub async fn list_plugin_permission_grants(
    db: &DatabaseConnection,
    plugin_id: &str,
) -> Result<Vec<crate::entities::plugin_permission_grant::Model>, sea_orm::DbErr> {
    crate::entities::plugin_permission_grant::Entity::find()
        .filter(crate::entities::plugin_permission_grant::Column::PluginId.eq(plugin_id))
        .all(db)
        .await
}

pub async fn replace_plugin_permission_grants(
    db: &DatabaseConnection,
    plugin_id: &str,
    grants: &[PluginPermissionGrantItem],
) -> Result<(), sea_orm::DbErr> {
    let now = chrono::Utc::now();
    for grant in grants {
        let existing = crate::entities::plugin_permission_grant::Entity::find()
            .filter(crate::entities::plugin_permission_grant::Column::PluginId.eq(plugin_id))
            .filter(
                crate::entities::plugin_permission_grant::Column::PermissionKey
                    .eq(grant.permission_key.as_str()),
            )
            .one(db)
            .await?;

        if let Some(existing) = existing {
            let mut active: crate::entities::plugin_permission_grant::ActiveModel = existing.into();
            active.granted = sea_orm::Set(grant.granted);
            active.updated_at = sea_orm::Set(now);
            active.update(db).await?;
        } else {
            let model = crate::entities::plugin_permission_grant::ActiveModel {
                id: sea_orm::Set(uuid::Uuid::now_v7().to_string()),
                plugin_id: sea_orm::Set(plugin_id.to_string()),
                permission_key: sea_orm::Set(grant.permission_key.clone()),
                granted: sea_orm::Set(grant.granted),
                created_at: sea_orm::Set(now),
                updated_at: sea_orm::Set(now),
            };
            let _ = model.insert(db).await?;
        }
    }
    Ok(())
}

pub async fn granted_permission_keys(
    db: &DatabaseConnection,
    plugin_id: &str,
) -> Result<Vec<String>, sea_orm::DbErr> {
    Ok(list_plugin_permission_grants(db, plugin_id)
        .await?
        .into_iter()
        .filter(|grant| grant.granted)
        .map(|grant| grant.permission_key)
        .collect())
}

pub async fn delete_plugin_permission_grants(
    db: &DatabaseConnection,
    plugin_id: &str,
) -> Result<u64, sea_orm::DbErr> {
    Ok(
        crate::entities::plugin_permission_grant::Entity::delete_many()
            .filter(crate::entities::plugin_permission_grant::Column::PluginId.eq(plugin_id))
            .exec(db)
            .await?
            .rows_affected,
    )
}
