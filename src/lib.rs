pub mod config;
pub mod entities;
pub mod handlers;
pub mod host_api;
pub mod installer;
pub mod manager;
pub mod manifest;
pub mod market;
pub mod openapi;
pub mod permissions;
pub mod public;
pub mod registry;
pub mod router;
pub mod runtime;

pub use config::{
    ExtensionManagerV2AppConfig, ExtensionManagerV2Config, ExtensionManagerV2ConfigManager,
    get_extension_manager_v2_config, init_extension_manager_v2_config,
};
pub use manager::{
    PluginRuntimeManagerV2, PluginRuntimeStatusSnapshot, get_plugin_runtime_manager,
    get_runtime_status_snapshot, init_plugin_runtime_manager,
};

use sea_orm::{ConnectionTrait, DatabaseConnection, Schema, Statement};

pub async fn init_db(db: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    let schema = Schema::new(db.get_database_backend());
    let builder = db.get_database_backend();
    let stmt = builder.build(
        schema
            .create_table_from_entity(entities::plugin_registry::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    let stmt = builder.build(
        schema
            .create_table_from_entity(entities::plugin_version::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    let stmt = builder.build(
        schema
            .create_table_from_entity(entities::plugin_audit_log::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    let stmt = builder.build(
        schema
            .create_table_from_entity(entities::plugin_permission_grant::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    let stmt = builder.build(
        schema
            .create_table_from_entity(entities::plugin_shared_record::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    let stmt = builder.build(
        schema
            .create_table_from_entity(entities::plugin_migration_state::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    let stmt = builder.build(
        schema
            .create_table_from_entity(entities::plugin_task::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    let stmt = builder.build(
        schema
            .create_table_from_entity(entities::plugin_nav_item::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    ensure_nav_item_columns(db).await?;

    Ok(())
}

async fn ensure_nav_item_columns(db: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    let backend = db.get_database_backend();
    if !matches!(backend, sea_orm::DatabaseBackend::Sqlite) {
        return Ok(());
    }

    let table_info = db
        .query_all(Statement::from_string(
            backend,
            "PRAGMA table_info(yh_plg_nav_items)".to_string(),
        ))
        .await?;
    let mut existing = std::collections::BTreeSet::new();
    for row in table_info {
        if let Ok(name) = row.try_get::<String>("", "name") {
            existing.insert(name);
        }
    }

    let alter_statements = [
        (
            "group_key",
            "ALTER TABLE yh_plg_nav_items ADD COLUMN group_key TEXT NULL",
        ),
        (
            "position",
            "ALTER TABLE yh_plg_nav_items ADD COLUMN position TEXT NULL",
        ),
        (
            "required_permission",
            "ALTER TABLE yh_plg_nav_items ADD COLUMN required_permission TEXT NULL",
        ),
    ];

    for (column, sql) in alter_statements {
        if !existing.contains(column) {
            db.execute(Statement::from_string(backend, sql.to_string()))
                .await?;
        }
    }

    Ok(())
}
