use crate::entities::{plugin_audit_log, plugin_registry, plugin_version};
use crate::manifest::{PluginManifest, PluginPermission, PluginRuntimeManifest};
use crate::permissions::{
    PluginPermissionGrantItem, permission_keys_to_items, replace_plugin_permission_grants,
};
use crate::runtime;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use utoipa::ToSchema;
use zip::ZipArchive;

pub const FILEUNI_PLUGIN_MARKET_BASE_URL: &str = "https://www.fileuni.com/api/plugins";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct InstallPluginResult {
    pub plugin_id: String,
    pub version: String,
    pub package_dir: String,
    pub checksum_sha256: String,
    pub prepared_runtime_kind: String,
}

#[derive(Debug, Clone)]
pub struct InstallPluginOptions {
    pub source_kind: String,
    pub market_origin: Option<String>,
    pub actor_user_id: Option<String>,
}

pub fn compute_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn safe_join(base: &Path, name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    if path.is_absolute() {
        return Err(format!("zip entry cannot be absolute: {}", name));
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("zip entry cannot escape package root: {}", name));
    }
    Ok(base.join(path))
}

pub fn read_manifest_from_zip_bytes(zip_bytes: &[u8]) -> Result<PluginManifest, String> {
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(reader).map_err(|e| format!("invalid plugin package: {}", e))?;
    let mut manifest_file = archive
        .by_name("plugin.json")
        .map_err(|e| format!("plugin.json is required: {}", e))?;
    let mut manifest_text = String::new();
    manifest_file
        .read_to_string(&mut manifest_text)
        .map_err(|e| format!("failed to read plugin.json: {}", e))?;
    let manifest: PluginManifest = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("invalid plugin manifest: {}", e))?;
    manifest.validate()?;
    Ok(manifest)
}

pub async fn read_manifest_from_package_dir(package_dir: &Path) -> Result<PluginManifest, String> {
    let manifest_path = package_dir.join("plugin.json");
    let manifest_text = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(|e| {
            format!(
                "failed to read installed plugin manifest '{}': {}",
                manifest_path.display(),
                e
            )
        })?;
    let manifest: PluginManifest = serde_json::from_str(&manifest_text).map_err(|e| {
        format!(
            "invalid installed plugin manifest '{}': {}",
            manifest_path.display(),
            e
        )
    })?;
    manifest.validate()?;
    Ok(manifest)
}

fn extract_plugin_zip_to_dir_blocking(zip_bytes: &[u8], target_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target_dir).map_err(|e| {
        format!(
            "failed to create package dir '{}': {}",
            target_dir.display(),
            e
        )
    })?;
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(reader).map_err(|e| format!("invalid plugin package: {}", e))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("failed to read zip entry {}: {}", index, e))?;
        let out_path = safe_join(target_dir, entry.name())?;
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| {
                format!(
                    "failed to create extracted dir '{}': {}",
                    out_path.display(),
                    e
                )
            })?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "failed to create extracted parent dir '{}': {}",
                    parent.display(),
                    e
                )
            })?;
        }
        let mut buffer = Vec::new();
        entry
            .read_to_end(&mut buffer)
            .map_err(|e| format!("failed to read zip entry '{}': {}", entry.name(), e))?;
        std::fs::write(&out_path, buffer).map_err(|e| {
            format!(
                "failed to write extracted file '{}': {}",
                out_path.display(),
                e
            )
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut mode = entry.unix_mode().unwrap_or(0o644);
            if out_path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                == Some("runtime")
                && (out_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| !name.ends_with(".wasm"))
                    .unwrap_or(false))
            {
                mode |= 0o755;
            }
            let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
        }
    }

    Ok(())
}

pub async fn extract_plugin_zip_to_dir(zip_bytes: &[u8], target_dir: &Path) -> Result<(), String> {
    let zip_bytes = zip_bytes.to_vec();
    let target_dir = target_dir.to_path_buf();
    tokio::task::spawn_blocking(move || extract_plugin_zip_to_dir_blocking(&zip_bytes, &target_dir))
        .await
        .map_err(|e| format!("plugin extraction task failed: {}", e))?
}

fn build_default_permission_grants(
    permissions: &[PluginPermission],
) -> Vec<PluginPermissionGrantItem> {
    permission_keys_to_items(permissions, &[])
}

fn validate_prepared_runtime(
    manifest: &PluginManifest,
    install_root: &Path,
) -> Result<String, String> {
    let handle = match &manifest.runtime {
        PluginRuntimeManifest::WasmComponent(runtime_manifest) => {
            runtime::wasm::prepare_wasm_runtime(
                &manifest.id,
                install_root,
                runtime_manifest,
                "wasm-component",
            )?
        }
        PluginRuntimeManifest::WasmModule(runtime_manifest) => runtime::wasm::prepare_wasm_runtime(
            &manifest.id,
            install_root,
            runtime_manifest,
            "wasm-module",
        )?,
        PluginRuntimeManifest::Process(runtime_manifest) => {
            runtime::process::prepare_process_runtime(&manifest.id, install_root, runtime_manifest)?
        }
        PluginRuntimeManifest::Docker(runtime_manifest) => {
            runtime::docker::prepare_docker_runtime(&manifest.id, runtime_manifest)?
        }
    };
    Ok(handle.runtime_kind)
}

pub async fn install_plugin_from_zip_bytes(
    db: &DatabaseConnection,
    packages_root: &Path,
    zip_bytes: &[u8],
    options: InstallPluginOptions,
) -> Result<InstallPluginResult, String> {
    let manifest = read_manifest_from_zip_bytes(zip_bytes)?;
    let checksum_sha256 = compute_sha256(zip_bytes);
    if let Some(expected) = &manifest.checksum_sha256
        && expected != &checksum_sha256
    {
        return Err("plugin package checksum does not match manifest checksum_sha256".to_string());
    }

    let package_dir = packages_root.join(&manifest.id).join(&manifest.version);
    if tokio::fs::try_exists(&package_dir).await.map_err(|e| {
        format!(
            "failed to check package dir '{}': {}",
            package_dir.display(),
            e
        )
    })? {
        tokio::fs::remove_dir_all(&package_dir).await.map_err(|e| {
            format!(
                "failed to replace existing package dir '{}': {}",
                package_dir.display(),
                e
            )
        })?;
    }

    extract_plugin_zip_to_dir(zip_bytes, &package_dir).await?;
    let prepared_runtime_kind = validate_prepared_runtime(&manifest, &package_dir)?;

    let now = chrono::Utc::now();
    let plugin_existing = plugin_registry::Entity::find_by_id(manifest.id.clone())
        .one(db)
        .await
        .map_err(|e| format!("failed to load plugin registry: {}", e))?;

    if let Some(existing) = plugin_existing {
        let mut active: plugin_registry::ActiveModel = existing.into();
        active.display_name = Set(manifest.name.clone());
        active.runtime_kind = Set(prepared_runtime_kind.clone());
        active.source_kind = Set(options.source_kind.clone());
        active.current_version = Set(Some(manifest.version.clone()));
        active.install_status = Set("installed".to_string());
        active.market_origin = Set(options.market_origin.clone());
        active.updated_at = Set(now);
        active
            .update(db)
            .await
            .map_err(|e| format!("failed to update plugin registry: {}", e))?;
    } else {
        let model = plugin_registry::ActiveModel {
            id: Set(manifest.id.clone()),
            display_name: Set(manifest.name.clone()),
            runtime_kind: Set(prepared_runtime_kind.clone()),
            source_kind: Set(options.source_kind.clone()),
            current_version: Set(Some(manifest.version.clone())),
            install_status: Set("installed".to_string()),
            enabled: Set(false),
            market_origin: Set(options.market_origin.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        model
            .insert(db)
            .await
            .map_err(|e| format!("failed to insert plugin registry: {}", e))?;
    }

    let version_model = plugin_version::ActiveModel {
        id: Set(uuid::Uuid::now_v7().to_string()),
        plugin_id: Set(manifest.id.clone()),
        version: Set(manifest.version.clone()),
        package_path: Set(package_dir.to_string_lossy().to_string()),
        checksum_sha256: Set(Some(checksum_sha256.clone())),
        install_status: Set("installed".to_string()),
        installed_at: Set(Some(now)),
        created_at: Set(now),
    };
    version_model
        .insert(db)
        .await
        .map_err(|e| format!("failed to insert plugin version: {}", e))?;

    replace_plugin_permission_grants(
        db,
        &manifest.id,
        &build_default_permission_grants(&manifest.permissions),
    )
    .await
    .map_err(|e| format!("failed to save plugin permission grants: {}", e))?;

    let audit_model = plugin_audit_log::ActiveModel {
        id: Set(uuid::Uuid::now_v7().to_string()),
        plugin_id: Set(manifest.id.clone()),
        action: Set("install".to_string()),
        message: Set(format!(
            "Installed plugin {} {}",
            manifest.id, manifest.version
        )),
        actor_user_id: Set(options.actor_user_id),
        created_at: Set(now),
    };
    audit_model
        .insert(db)
        .await
        .map_err(|e| format!("failed to insert plugin audit log: {}", e))?;

    Ok(InstallPluginResult {
        plugin_id: manifest.id,
        version: manifest.version,
        package_dir: package_dir.to_string_lossy().to_string(),
        checksum_sha256,
        prepared_runtime_kind,
    })
}

pub async fn read_plugin_zip_from_path(path: &Path) -> Result<Vec<u8>, String> {
    tokio::fs::read(path)
        .await
        .map_err(|e| format!("failed to read plugin package '{}': {}", path.display(), e))
}

pub fn read_plugin_zip_from_file(file: &mut File) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("failed to read plugin package file: {}", e))?;
    Ok(bytes)
}
