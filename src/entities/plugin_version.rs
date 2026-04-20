use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, ToSchema)]
#[sea_orm(table_name = "yh_plg_versions")]
pub struct Model {
    #[sea_orm(primary_key, column_type = "Text", auto_increment = false)]
    pub id: String,
    #[sea_orm(column_type = "Text")]
    pub plugin_id: String,
    #[sea_orm(column_type = "Text")]
    pub version: String,
    #[sea_orm(column_type = "Text")]
    pub package_path: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub checksum_sha256: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub install_status: String,
    #[schema(value_type = String, format = DateTime)]
    pub installed_at: Option<DateTimeUtc>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
