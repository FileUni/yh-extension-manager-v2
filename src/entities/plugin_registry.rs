use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, ToSchema)]
#[sea_orm(table_name = "yh_plg_registry")]
pub struct Model {
    #[sea_orm(primary_key, column_type = "Text", auto_increment = false)]
    pub id: String,
    #[sea_orm(column_type = "Text")]
    pub display_name: String,
    #[sea_orm(column_type = "Text")]
    pub runtime_kind: String,
    #[sea_orm(column_type = "Text")]
    pub source_kind: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub current_version: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub install_status: String,
    pub enabled: bool,
    #[sea_orm(column_type = "Text", nullable)]
    pub market_origin: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
