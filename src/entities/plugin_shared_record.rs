use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, ToSchema)]
#[sea_orm(table_name = "yh_plg_shared_records")]
pub struct Model {
    #[sea_orm(primary_key, column_type = "Text", auto_increment = false)]
    pub id: String,
    #[sea_orm(column_type = "Text")]
    pub plugin_id: String,
    #[sea_orm(column_type = "Text")]
    pub collection: String,
    #[sea_orm(column_type = "Text")]
    pub record_key: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub owner_user_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub payload_json: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
