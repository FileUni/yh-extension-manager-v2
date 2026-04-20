use crate::installer::{InstallPluginOptions, install_plugin_from_zip_bytes};
use crate::manager::get_runtime_status_snapshot;
use crate::registry::{self, RegistryStats};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use utoipa::ToSchema;

pub const DEFAULT_PLUGIN_MARKET_BASE_URL: &str = "https://www.fileuni.com/api/plugins";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct MarketInstallFromUrlRequest {
    pub download_url: String,
    pub actor_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct MarketInstallFromUrlResponse {
    pub plugin_id: String,
    pub version: String,
    pub package_dir: String,
    pub checksum_sha256: String,
    pub prepared_runtime_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct MarketCatalogResponse {
    pub schema_version: u64,
    pub source: String,
    pub generated_at: String,
    pub plugin_count: usize,
    pub stats: RegistryStats,
}

pub async fn fetch_catalog() -> Result<serde_json::Value, String> {
    let response = reqwest::Client::new()
        .get(DEFAULT_PLUGIN_MARKET_BASE_URL)
        .send()
        .await
        .map_err(|e| format!("failed to fetch plugin market catalog: {}", e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("plugin market catalog request failed with status {}", status));
    }
    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("failed to parse plugin market catalog: {}", e))
}

pub async fn install_from_download_url(
    db: &DatabaseConnection,
    packages_root: &std::path::Path,
    request: MarketInstallFromUrlRequest,
) -> Result<serde_json::Value, String> {
    let response = reqwest::Client::new()
        .get(&request.download_url)
        .send()
        .await
        .map_err(|e| format!("failed to download plugin zip: {}", e))?;
    if !response.status().is_success() {
        return Err(format!(
            "plugin zip download failed with status {}",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read plugin zip response body: {}", e))?;
    let result = install_plugin_from_zip_bytes(
        db,
        packages_root,
        &bytes,
        InstallPluginOptions {
            source_kind: "market".to_string(),
            market_origin: Some(request.download_url.clone()),
            actor_user_id: request.actor_user_id,
        },
    )
    .await?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

pub async fn market_catalog_snapshot(
    db: &DatabaseConnection,
) -> Result<MarketCatalogResponse, sea_orm::DbErr> {
    let stats = registry::collect_stats(db).await?;
    let runtime = get_runtime_status_snapshot().await.map_err(|e| {
        sea_orm::DbErr::Custom(format!("plugin runtime status snapshot failed: {}", e))
    })?;
    Ok(MarketCatalogResponse {
        schema_version: 1,
        source: DEFAULT_PLUGIN_MARKET_BASE_URL.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        plugin_count: stats.plugin_count as usize,
        stats,
    })
}
