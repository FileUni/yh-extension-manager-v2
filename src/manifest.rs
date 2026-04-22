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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct ProcessRuntimeManifest {
    pub program: String,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub stdin: Option<String>,
    pub base_url: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PluginRuntimeManifest {
    WasmComponent(WasmRuntimeManifest),
    WasmModule(WasmRuntimeManifest),
    Process(ProcessRuntimeManifest),
    Docker(DockerRuntimeManifest),
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
