pub mod docker;
pub mod process;
pub mod wasm;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub enum RuntimeStatus {
    Prepared,
    Running,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct RuntimeHandle {
    pub plugin_id: String,
    pub runtime_kind: String,
    pub status: RuntimeStatus,
    pub detail: String,
    pub pid: Option<u32>,
    pub instance_ref: Option<String>,
    pub route_base_url: Option<String>,
}
