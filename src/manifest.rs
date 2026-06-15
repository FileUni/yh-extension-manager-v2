use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PluginPermission {
    AuthRead,
    UserLookup,
    UserPermissionCheck,
    VfsRead,
    VfsWrite,
    KvRead,
    KvWrite,
    KvDelete,
    DbSharedRead,
    DbSharedWrite,
    DbSqlite,
    WebApi,
    WebSocket,
    Scheduler,
    Network,
    ProcessExecution,
    DockerExecution,
}

impl PluginPermission {
    pub fn as_key(&self) -> &'static str {
        match self {
            Self::AuthRead => "auth-read",
            Self::UserLookup => "user-lookup",
            Self::UserPermissionCheck => "user-permission-check",
            Self::VfsRead => "vfs-read",
            Self::VfsWrite => "vfs-write",
            Self::KvRead => "kv-read",
            Self::KvWrite => "kv-write",
            Self::KvDelete => "kv-delete",
            Self::DbSharedRead => "db-shared-read",
            Self::DbSharedWrite => "db-shared-write",
            Self::DbSqlite => "db-sqlite",
            Self::WebApi => "web-api",
            Self::WebSocket => "web-socket",
            Self::Scheduler => "scheduler",
            Self::Network => "network",
            Self::ProcessExecution => "process-execution",
            Self::DockerExecution => "docker-execution",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PluginRuntimeKind {
    WasmComponent,
    WasmModule,
    Process,
    Docker,
}

impl PluginRuntimeKind {
    pub fn as_kebab_str(&self) -> &'static str {
        match self {
            PluginRuntimeKind::WasmComponent => "wasm-component",
            PluginRuntimeKind::WasmModule => "wasm-module",
            PluginRuntimeKind::Process => "process",
            PluginRuntimeKind::Docker => "docker",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginUiManifest {
    pub root: String,
    pub mount_path: Option<String>,
    pub sandboxed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginMarketManifest {
    pub keywords: Vec<String>,
    pub screenshots: Vec<String>,
    pub homepage_url: Option<String>,
    pub repository_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub struct WasmRuntimeManifest {
    pub artifact: String,
    pub entrypoint: Option<String>,
    pub component: Option<bool>,
    pub args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    #[serde(alias = "base_url", alias = "base-url")]
    pub base_url: Option<String>,
    pub network: Option<PluginNetworkManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct ProcessRuntimeManifest {
    pub program: String,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub stdin: Option<String>,
    pub base_url: Option<String>,
    pub network: Option<PluginNetworkManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct DockerPortMapping {
    pub host: Option<u16>,
    pub container: u16,
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct DockerVolumeMapping {
    pub source: String,
    pub target: String,
    pub read_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct DockerRuntimeManifest {
    pub image: Option<String>,
    pub oci_archive: Option<String>,
    pub compose_file: Option<String>,
    pub command: Option<Vec<String>>,
    pub args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub ports: Option<Vec<DockerPortMapping>>,
    pub volumes: Option<Vec<DockerVolumeMapping>>,
    pub workdir: Option<String>,
    pub base_url: Option<String>,
    pub network: Option<PluginNetworkManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PluginNetworkMode {
    Proxy,
    Direct,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginNetworkManifest {
    pub mode: Option<PluginNetworkMode>,
    pub port: Option<u16>,
    pub base_url: Option<String>,
}

impl PluginNetworkManifest {
    pub fn mode(&self) -> PluginNetworkMode {
        self.mode.clone().unwrap_or(PluginNetworkMode::Proxy)
    }

    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }
}

impl ProcessRuntimeManifest {
    pub fn effective_base_url(&self) -> Option<String> {
        self.network
            .as_ref()
            .and_then(|n| n.base_url.clone())
            .or_else(|| self.base_url.clone())
    }
}

impl WasmRuntimeManifest {
    pub fn effective_base_url(&self) -> Option<String> {
        self.network
            .as_ref()
            .and_then(|n| n.base_url.clone())
            .or_else(|| self.base_url.clone())
    }
}

impl DockerRuntimeManifest {
    pub fn effective_base_url(&self) -> Option<String> {
        self.network
            .as_ref()
            .and_then(|n| n.base_url.clone())
            .or_else(|| self.base_url.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PluginRuntimeManifest {
    WasmComponent(WasmRuntimeManifest),
    WasmModule(WasmRuntimeManifest),
    #[serde(alias = "process")]
    Process(ProcessRuntimeManifest),
    #[serde(alias = "docker")]
    Docker(DockerRuntimeManifest),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PluginSetupAction {
    Download,
    Extract,
    Move,
    Copy,
    Symlink,
    Chmod,
    Mkdir,
    Remove,
    Run,
    Touch,
    WriteFile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(untagged)]
pub enum PluginSetupCondition {
    FileNotExists { file_not_exists: String },
    FileExists { file_exists: String },
    EnvNotSet { env_not_set: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginSetupStep {
    pub action: PluginSetupAction,
    pub url: Option<String>,
    pub source: Option<String>,
    pub dest: Option<String>,
    pub archive_format: Option<String>,
    pub strip_components: Option<u32>,
    pub mode: Option<String>,
    pub args: Option<Vec<String>>,
    pub content: Option<String>,
    #[serde(alias = "sha256", alias = "sha-256")]
    pub sha256: Option<String>,
    pub archive: Option<String>,
    #[serde(alias = "if")]
    pub if_condition: Option<PluginSetupCondition>,
    pub symlink_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub description: String,
    pub author: String,
    pub runtime: PluginRuntimeManifest,
    pub permissions: Vec<PluginPermission>,
    pub tags: Vec<String>,
    pub ui: Option<PluginUiManifest>,
    pub market: Option<PluginMarketManifest>,
    pub homepage_url: Option<String>,
    pub repository_url: Option<String>,
    pub checksum_sha256: Option<String>,
    pub install_commands: Option<Vec<PluginSetupStep>>,
    pub upgrade_commands: Option<Vec<PluginSetupStep>>,
    pub uninstall_commands: Option<Vec<PluginSetupStep>>,
    pub config_schema: Option<serde_json::Value>,
}

impl PluginRuntimeManifest {
    pub fn network(&self) -> PluginNetworkManifest {
        let direct_base_url = match self {
            PluginRuntimeManifest::WasmComponent(r) | PluginRuntimeManifest::WasmModule(r) => {
                r.base_url.clone()
            }
            PluginRuntimeManifest::Process(r) => r.base_url.clone(),
            PluginRuntimeManifest::Docker(r) => r.base_url.clone(),
        };
        let network = match self {
            PluginRuntimeManifest::WasmComponent(r)
            | PluginRuntimeManifest::WasmModule(r) => r.network.clone(),
            PluginRuntimeManifest::Process(r) => r.network.clone(),
            PluginRuntimeManifest::Docker(r) => r.network.clone(),
        };
        let mut resolved = network.unwrap_or(PluginNetworkManifest {
            mode: None,
            port: None,
            base_url: None,
        });
        if resolved.base_url.is_none() {
            resolved.base_url = direct_base_url;
        }
        resolved
    }

    pub fn base_url(&self) -> Option<String> {
        self.network().base_url
    }
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("plugin id is required".to_string());
        }
        if self.name.trim().is_empty() {
            return Err("plugin name is required".to_string());
        }
        if self.version.trim().is_empty() {
            return Err("plugin version is required".to_string());
        }
        if self.summary.trim().is_empty() {
            return Err("plugin summary is required".to_string());
        }
        if self.description.trim().is_empty() {
            return Err("plugin description is required".to_string());
        }
        if self.author.trim().is_empty() {
            return Err("plugin author is required".to_string());
        }

        match &self.runtime {
            PluginRuntimeManifest::WasmComponent(runtime)
            | PluginRuntimeManifest::WasmModule(runtime) => {
                if runtime.artifact.trim().is_empty() {
                    return Err("wasm artifact is required".to_string());
                }
            }
            PluginRuntimeManifest::Process(runtime) => {
                if runtime.program.trim().is_empty() {
                    return Err("process program is required".to_string());
                }
            }
            PluginRuntimeManifest::Docker(runtime) => {
                let image_empty = runtime
                    .image
                    .as_ref()
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true);
                let archive_empty = runtime
                    .oci_archive
                    .as_ref()
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true);
                let compose_empty = runtime
                    .compose_file
                    .as_ref()
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true);
                if image_empty && archive_empty && compose_empty {
                    return Err(
                        "docker runtime requires image, oci_archive, or compose_file".to_string(),
                    );
                }
            }
        }

        for (label, steps) in [
            ("install_commands", &self.install_commands),
            ("upgrade_commands", &self.upgrade_commands),
            ("uninstall_commands", &self.uninstall_commands),
        ] {
            if let Some(entries) = steps {
                for (i, step) in entries.iter().enumerate() {
                    if matches!(step.action, PluginSetupAction::Download)
                        && step.url.as_ref().map(|v| v.trim().is_empty()).unwrap_or(true)
                    {
                        return Err(format!(
                            "{}[{}].url is required for Download action in {}",
                            label, i, label
                        ));
                    }
                    if let Some(ref sha) = step.sha256 {
                        if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                            return Err(format!(
                                "{}[{}].sha256 must be a 64-character hex string, got '{}'",
                                label, i, sha
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn runtime_kind(&self) -> PluginRuntimeKind {
        match &self.runtime {
            PluginRuntimeManifest::WasmComponent(_) => PluginRuntimeKind::WasmComponent,
            PluginRuntimeManifest::WasmModule(_) => PluginRuntimeKind::WasmModule,
            PluginRuntimeManifest::Process(_) => PluginRuntimeKind::Process,
            PluginRuntimeManifest::Docker(_) => PluginRuntimeKind::Docker,
        }
    }

    pub fn runtime_artifact(&self) -> Option<&str> {
        match &self.runtime {
            PluginRuntimeManifest::WasmComponent(runtime)
            | PluginRuntimeManifest::WasmModule(runtime) => Some(runtime.artifact.as_str()),
            PluginRuntimeManifest::Process(runtime) => Some(runtime.program.as_str()),
            PluginRuntimeManifest::Docker(_) => None,
        }
    }
}
