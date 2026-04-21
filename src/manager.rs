use crate::config::get_extension_manager_v2_config;
use base64::Engine;
use dashmap::DashMap;
use rand::RngCore;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::OnceCell;
use utoipa::ToSchema;

#[derive(Debug, Clone)]
struct ExtensionManagerV2ConfigSnapshot {
    enabled: bool,
    root_dir: String,
    temp_dir: String,
    market_request_timeout_sec: u64,
    allow_sideload: bool,
    enable_wasm_runtime: bool,
    enable_process_runtime: bool,
    enable_docker_runtime: bool,
    docker_engine_command: String,
    host_api_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct RuntimeDirectoryLayout {
    pub root_dir: String,
    pub cache_dir: String,
    pub packages_dir: String,
    pub state_dir: String,
    pub config_dir: String,
    pub logs_dir: String,
    pub runtime_dir: String,
    pub shared_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginRuntimeStatusSnapshot {
    pub initialized: bool,
    pub enabled: bool,
    pub temp_dir: String,
    pub market_request_timeout_sec: u64,
    pub allow_sideload: bool,
    pub enable_wasm_runtime: bool,
    pub enable_process_runtime: bool,
    pub enable_docker_runtime: bool,
    pub docker_engine_command: String,
    pub host_api_base_url: String,
    pub layout: RuntimeDirectoryLayout,
}

#[derive(Debug)]
pub struct PluginRuntimeManagerV2 {
    snapshot: PluginRuntimeStatusSnapshot,
    db: Option<Arc<DatabaseConnection>>,
    host_api_secret: Arc<[u8]>,
    active_runtimes: DashMap<String, crate::runtime::RuntimeHandle>,
}

static PLUGIN_RUNTIME_MANAGER: OnceCell<Arc<PluginRuntimeManagerV2>> = OnceCell::const_new();

fn layout_from_root(root_dir: &str) -> RuntimeDirectoryLayout {
    fn path_to_string(path: PathBuf) -> String {
        path.to_string_lossy().to_string()
    }

    let root = Path::new(root_dir);
    RuntimeDirectoryLayout {
        root_dir: root_dir.to_string(),
        packages_dir: path_to_string(root.join("packages")),
        state_dir: path_to_string(root.join("state")),
        config_dir: path_to_string(root.join("config")),
        logs_dir: path_to_string(root.join("logs")),
        runtime_dir: path_to_string(root.join("runtime")),
        shared_dir: path_to_string(root.join("shared")),
        cache_dir: path_to_string(root.join("cache")),
    }
}

async fn read_config_snapshot() -> Result<ExtensionManagerV2ConfigSnapshot, String> {
    let config_arc = get_extension_manager_v2_config()
        .ok_or_else(|| "extension_manager_v2 config is not initialized".to_string())?;
    let guard = config_arc.read().await;
    let config = &guard.extension_manager_v2;
    let host_api_base_url =
        if let Some(core_cfg) = yh_config_infra::core_crate_config::get_core_config() {
            let core_guard = core_cfg.read().await;
            let server = &core_guard.server;
            let host = if server.get_main_ip() == "0.0.0.0" {
                "127.0.0.1"
            } else {
                server.get_main_ip()
            };
            format!(
                "http://{}:{}/api/v1/plugin-host",
                host,
                server.get_main_port()
            )
        } else {
            "http://127.0.0.1:19000/api/v1/plugin-host".to_string()
        };
    Ok(ExtensionManagerV2ConfigSnapshot {
        enabled: config.is_enabled(),
        root_dir: config.get_root_dir(),
        temp_dir: config.get_temp_dir(),
        market_request_timeout_sec: config.get_market_request_timeout_sec(),
        allow_sideload: config.is_allow_sideload(),
        enable_wasm_runtime: config.is_enable_wasm_runtime(),
        enable_process_runtime: config.is_enable_process_runtime(),
        enable_docker_runtime: config.is_enable_docker_runtime(),
        docker_engine_command: config.get_docker_engine_command(),
        host_api_base_url,
    })
}

impl PluginRuntimeManagerV2 {
    fn new(snapshot: PluginRuntimeStatusSnapshot) -> Self {
        let mut host_api_secret = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut host_api_secret);
        Self {
            snapshot,
            db: None,
            host_api_secret: Arc::from(host_api_secret.to_vec()),
            active_runtimes: DashMap::new(),
        }
    }

    pub fn status_snapshot(&self) -> PluginRuntimeStatusSnapshot {
        self.snapshot.clone()
    }

    pub fn docker_engine_command(&self) -> &str {
        &self.snapshot.docker_engine_command
    }

    pub fn db(&self) -> Option<Arc<DatabaseConnection>> {
        self.db.as_ref().map(Arc::clone)
    }

    pub fn set_runtime_handle(&self, plugin_id: &str, handle: crate::runtime::RuntimeHandle) {
        self.active_runtimes.insert(plugin_id.to_string(), handle);
    }

    pub fn get_runtime_handle(&self, plugin_id: &str) -> Option<crate::runtime::RuntimeHandle> {
        self.active_runtimes
            .get(plugin_id)
            .map(|entry| entry.clone())
    }

    pub fn list_runtime_handles(&self) -> Vec<crate::runtime::RuntimeHandle> {
        self.active_runtimes
            .iter()
            .map(|entry| entry.clone())
            .collect()
    }

    pub fn active_runtime_count(&self) -> usize {
        self.active_runtimes.len()
    }

    pub fn remove_runtime_handle(&self, plugin_id: &str) -> Option<crate::runtime::RuntimeHandle> {
        self.active_runtimes
            .remove(plugin_id)
            .map(|(_, handle)| handle)
    }

    pub fn host_api_secret_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.host_api_secret.as_ref())
    }

    pub async fn ensure_plugin_config_paths(
        &self,
        plugin_id: &str,
    ) -> Result<(String, String), String> {
        let plugin_component = plugin_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let config_dir = PathBuf::from(&self.snapshot.layout.config_dir).join(&plugin_component);
        tokio::fs::create_dir_all(&config_dir).await.map_err(|e| {
            format!(
                "failed to create plugin config dir '{}': {}",
                config_dir.display(),
                e
            )
        })?;
        let config_file = config_dir.join("config.toml");
        if !tokio::fs::try_exists(&config_file).await.map_err(|e| {
            format!(
                "failed to check plugin config file '{}': {}",
                config_file.display(),
                e
            )
        })? {
            tokio::fs::write(&config_file, b"# plugin config\n")
                .await
                .map_err(|e| {
                    format!(
                        "failed to create plugin config file '{}': {}",
                        config_file.display(),
                        e
                    )
                })?;
        }
        Ok((
            config_dir.to_string_lossy().to_string(),
            config_file.to_string_lossy().to_string(),
        ))
    }
}

pub async fn init_plugin_runtime_manager(
    _config_path: &str,
    db: Arc<DatabaseConnection>,
) -> Result<Arc<PluginRuntimeManagerV2>, String> {
    let manager_ref = PLUGIN_RUNTIME_MANAGER
        .get_or_try_init(|| async {
            let config = read_config_snapshot().await?;
            let layout = layout_from_root(&config.root_dir);
            let temp_root = Path::new(&config.temp_dir);
            if config.enabled {
                for path in [
                    &layout.root_dir,
                    &layout.packages_dir,
                    &layout.state_dir,
                    &layout.config_dir,
                    &layout.logs_dir,
                    &layout.runtime_dir,
                    &layout.shared_dir,
                    &config.temp_dir,
                ] {
                    tokio::fs::create_dir_all(path).await.map_err(|e| {
                        format!("failed to create plugin runtime dir '{}': {}", path, e)
                    })?;
                }
                tokio::fs::create_dir_all(temp_root.join("cache"))
                    .await
                    .map_err(|e| format!("failed to create plugin temp cache dir: {}", e))?;
            }
            let snapshot = PluginRuntimeStatusSnapshot {
                initialized: true,
                enabled: config.enabled,
                temp_dir: config.temp_dir,
                market_request_timeout_sec: config.market_request_timeout_sec,
                allow_sideload: config.allow_sideload,
                enable_wasm_runtime: config.enable_wasm_runtime,
                enable_process_runtime: config.enable_process_runtime,
                enable_docker_runtime: config.enable_docker_runtime,
                docker_engine_command: config.docker_engine_command,
                host_api_base_url: config.host_api_base_url,
                layout,
            };
            let mut manager = PluginRuntimeManagerV2::new(snapshot);
            manager.db = Some(db);
            Ok::<Arc<PluginRuntimeManagerV2>, String>(Arc::new(manager))
        })
        .await?;
    Ok(Arc::clone(manager_ref))
}

pub fn get_plugin_runtime_manager() -> Option<Arc<PluginRuntimeManagerV2>> {
    PLUGIN_RUNTIME_MANAGER.get().map(Arc::clone)
}

pub async fn get_runtime_status_snapshot() -> Result<PluginRuntimeStatusSnapshot, String> {
    if let Some(manager) = get_plugin_runtime_manager() {
        let _ = manager.active_runtime_count();
        let _ = manager.host_api_secret_base64();
        return Ok(manager.status_snapshot());
    }

    let config = read_config_snapshot().await?;
    Ok(PluginRuntimeStatusSnapshot {
        initialized: false,
        enabled: config.enabled,
        temp_dir: config.temp_dir,
        market_request_timeout_sec: config.market_request_timeout_sec,
        allow_sideload: config.allow_sideload,
        enable_wasm_runtime: config.enable_wasm_runtime,
        enable_process_runtime: config.enable_process_runtime,
        enable_docker_runtime: config.enable_docker_runtime,
        docker_engine_command: config.docker_engine_command,
        host_api_base_url: config.host_api_base_url,
        layout: layout_from_root(&config.root_dir),
    })
}
