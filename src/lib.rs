pub mod config;
pub mod entities;
pub mod host_api;
pub mod handlers;
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

use sea_orm::{ConnectionTrait, DatabaseConnection, Schema};

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

    Ok(())
}
