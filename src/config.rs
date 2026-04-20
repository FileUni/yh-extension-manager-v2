use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
pub use yh_config_infra::{
    BaseConfigManager, ConfigApp, ConfigDoc, impl_config_manager_boilerplate,
};

#[derive(Debug, Serialize, Deserialize, PartialEq, ConfigDoc)]
pub struct ExtensionManagerV2Config {
    #[config(
        desc_zh = "是否启用插件系统 v2",
        desc_en = "Enable plugin system v2",
        example = "false"
    )]
    pub enabled: Option<bool>,
    #[config(
        desc_zh = "插件系统根目录",
        desc_en = "Plugin system root directory",
        example = "{RUNTIMEDIR}/plugins"
    )]
    pub root_dir: Option<String>,
    #[config(
        desc_zh = "插件系统临时目录",
        desc_en = "Plugin system temporary directory",
        example = "{TEMPDIR}/extension"
    )]
    pub temp_dir: Option<String>,
    #[config(
        desc_zh = "插件市场请求超时时间（秒）",
        desc_en = "Plugin market request timeout in seconds",
        example = "30"
    )]
    pub market_request_timeout_sec: Option<u64>,
    #[config(
        desc_zh = "是否允许本地 sideload 安装插件包",
        desc_en = "Allow local sideload plugin packages",
        example = "true"
    )]
    pub allow_sideload: Option<bool>,
    #[config(
        desc_zh = "是否启用 wasm 运行时",
        desc_en = "Enable wasm runtime",
        example = "true"
    )]
    pub enable_wasm_runtime: Option<bool>,
    #[config(
        desc_zh = "是否启用 process 运行时",
        desc_en = "Enable process runtime",
        example = "true"
    )]
    pub enable_process_runtime: Option<bool>,
    #[config(
        desc_zh = "是否启用 docker 运行时",
        desc_en = "Enable docker runtime",
        example = "false"
    )]
    pub enable_docker_runtime: Option<bool>,
    #[config(
        desc_zh = "docker 引擎命令",
        desc_en = "Docker engine command",
        example = "docker"
    )]
    pub docker_engine_command: Option<String>,
}

impl ExtensionManagerV2Config {
    pub fn validate(&self, section: &str, errors: &mut Vec<String>) {
        let s = section;
        yh_config_infra::config_collect_bool!(self.enabled, s, "enabled", errors);
        yh_config_infra::config_collect_not_empty!(self.root_dir, s, "root_dir", errors);
        yh_config_infra::config_collect_not_empty!(self.temp_dir, s, "temp_dir", errors);
        yh_config_infra::config_collect_gt_zero!(
            self.market_request_timeout_sec,
            s,
            "market_request_timeout_sec",
            errors
        );
        yh_config_infra::config_collect_bool!(self.allow_sideload, s, "allow_sideload", errors);
        yh_config_infra::config_collect_bool!(
            self.enable_wasm_runtime,
            s,
            "enable_wasm_runtime",
            errors
        );
        yh_config_infra::config_collect_bool!(
            self.enable_process_runtime,
            s,
            "enable_process_runtime",
            errors
        );
        yh_config_infra::config_collect_bool!(
            self.enable_docker_runtime,
            s,
            "enable_docker_runtime",
            errors
        );
        yh_config_infra::config_collect_not_empty!(
            self.docker_engine_command,
            s,
            "docker_engine_command",
            errors
        );
    }

    pub fn is_enabled(&self) -> bool {
        yh_config_infra::config_require_copy!(self.enabled, "extension_manager_v2", "enabled")
    }

    pub fn get_root_dir(&self) -> String {
        yh_config_infra::config_require_clone!(self.root_dir, "extension_manager_v2", "root_dir")
    }

    pub fn get_temp_dir(&self) -> String {
        yh_config_infra::config_require_clone!(self.temp_dir, "extension_manager_v2", "temp_dir")
    }

    pub fn get_market_request_timeout_sec(&self) -> u64 {
        yh_config_infra::config_require_copy!(
            self.market_request_timeout_sec,
            "extension_manager_v2",
            "market_request_timeout_sec"
        )
    }

    pub fn is_allow_sideload(&self) -> bool {
        yh_config_infra::config_require_copy!(
            self.allow_sideload,
            "extension_manager_v2",
            "allow_sideload"
        )
    }

    pub fn is_enable_wasm_runtime(&self) -> bool {
        yh_config_infra::config_require_copy!(
            self.enable_wasm_runtime,
            "extension_manager_v2",
            "enable_wasm_runtime"
        )
    }

    pub fn is_enable_process_runtime(&self) -> bool {
        yh_config_infra::config_require_copy!(
            self.enable_process_runtime,
            "extension_manager_v2",
            "enable_process_runtime"
        )
    }

    pub fn is_enable_docker_runtime(&self) -> bool {
        yh_config_infra::config_require_copy!(
            self.enable_docker_runtime,
            "extension_manager_v2",
            "enable_docker_runtime"
        )
    }

    pub fn get_docker_engine_command(&self) -> String {
        yh_config_infra::config_require_clone!(
            self.docker_engine_command,
            "extension_manager_v2",
            "docker_engine_command"
        )
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, ConfigDoc)]
pub struct ExtensionManagerV2AppConfig {
    #[config(desc_zh = "插件系统 v2 配置", desc_en = "Plugin system v2 configuration")]
    pub extension_manager_v2: ExtensionManagerV2Config,
}

impl ConfigApp for ExtensionManagerV2AppConfig {
    fn get_section_name() -> &'static str {
        "extension_manager_v2"
    }
}

impl ExtensionManagerV2AppConfig {
    pub fn validate(&self, errors: &mut Vec<String>) {
        self.extension_manager_v2
            .validate(Self::get_section_name(), errors);
    }
}

pub struct ExtensionManagerV2ConfigManager {
    pub inner: BaseConfigManager<ExtensionManagerV2AppConfig>,
}

impl ExtensionManagerV2ConfigManager {
    pub async fn new(config_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let inner = BaseConfigManager::new(config_path).await?;
        Ok(Self { inner })
    }

    pub fn get_config_arc(&self) -> Arc<RwLock<Arc<ExtensionManagerV2AppConfig>>> {
        self.inner.get_config_arc()
    }

    pub async fn validate(&self) -> Result<(), String> {
        self.inner
            .with_config(|cfg| {
                let mut errors = Vec::new();
                cfg.validate(&mut errors);
                if errors.is_empty() {
                    Ok(())
                } else {
                    Err(errors.join(", "))
                }
            })
            .await
    }
}

impl_config_manager_boilerplate!(
    ExtensionManagerV2ConfigManager,
    ExtensionManagerV2AppConfig,
    EXTENSION_MANAGER_V2_CONFIG_MANAGER,
    init_extension_manager_v2_config,
    get_extension_manager_v2_config_manager
);

pub fn get_extension_manager_v2_config() -> Option<Arc<RwLock<Arc<ExtensionManagerV2AppConfig>>>> {
    get_config_arc()
}
