use crate::entities::{plugin_audit_log, plugin_registry, plugin_version};
use crate::manifest::{
    PluginManifest, PluginPermission, PluginRuntimeManifest, PluginSetupAction,
    PluginSetupCondition, PluginSetupStep,
};
use crate::permissions::{
    PluginPermissionGrantItem, permission_keys_to_items, replace_plugin_permission_grants,
};
use crate::runtime;
use flate2::read::GzDecoder;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use tar::Archive;
use utoipa::ToSchema;
use zip::ZipArchive;

pub const FILEUNI_PLUGIN_MARKET_BASE_URL: &str = "https://www.fileuni.com/api/plugins";

pub const PLUGIN_MANIFEST_FILE_NAME: &str = "plugin.json";
pub const PLUGIN_DEFAULT_CONFIG_FILE_NAME: &str = "config.toml";

pub const INSTALL_STATUS_PENDING: &str = "pending";
pub const INSTALL_STATUS_INSTALLED: &str = "installed";
pub const INSTALL_STATUS_RUNNING: &str = "running";
pub const INSTALL_STATUS_UNINSTALLED: &str = "uninstalled";

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

fn resolve_template_vars(template: &str, version: &str, plugin_id: &str, name: &str) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let arch_str = match arch {
        "x86_64" => "amd64",
        "aarch64" => "aarch64",
        other => other,
    };
    let name_encoded = name.replace(' ', "%20");
    template
        .replace("{version}", version)
        .replace("{os}", os)
        .replace("{arch}", arch_str)
        .replace("{plugin_id}", plugin_id)
        .replace("{name}", &name_encoded)
}

fn check_setup_condition(
    condition: &PluginSetupCondition,
    pkg_dir: &Path,
) -> Result<bool, String> {
    match condition {
        PluginSetupCondition::FileNotExists { file_not_exists } => {
            let path = pkg_dir.join(file_not_exists);
            Ok(!path_exists(&path))
        }
        PluginSetupCondition::FileExists { file_exists } => {
            let path = pkg_dir.join(file_exists);
            Ok(path_exists(&path))
        }
        PluginSetupCondition::EnvNotSet { env_not_set } => {
            Ok(std::env::var(env_not_set).is_err())
        }
    }
}

fn path_exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

fn verify_sha256(data: &[u8], expected: &str) -> Result<(), String> {
    let actual = compute_sha256(data);
    if actual != expected {
        return Err(format!(
            "SHA-256 mismatch: expected {} but computed {}",
            expected, actual
        ));
    }
    Ok(())
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
    let mut archive =
        ZipArchive::new(reader).map_err(|e| format!("invalid plugin package: {}", e))?;
    let mut manifest_file = archive
        .by_name(PLUGIN_MANIFEST_FILE_NAME)
        .map_err(|e| format!("{} is required in plugin package root: {}", PLUGIN_MANIFEST_FILE_NAME, e))?;
    let mut manifest_text = String::new();
    manifest_file
        .read_to_string(&mut manifest_text)
        .map_err(|e| format!("failed to read {}: {}", PLUGIN_MANIFEST_FILE_NAME, e))?;
    let manifest: PluginManifest = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("invalid plugin manifest: {}", e))?;
    manifest.validate()?;
    Ok(manifest)
}

pub async fn read_manifest_from_package_dir(package_dir: &Path) -> Result<PluginManifest, String> {
    let manifest_path = package_dir.join(PLUGIN_MANIFEST_FILE_NAME);
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
    let mut archive =
        ZipArchive::new(reader).map_err(|e| format!("invalid plugin package: {}", e))?;

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

/// Perform basic validation at registration time without executing lifecycle commands
/// This catches obvious errors like missing runtime artifacts before DB write
fn validate_prepared_runtime_basic(
    manifest: &PluginManifest,
    install_root: &Path,
) -> Result<(), String> {
    match &manifest.runtime {
        PluginRuntimeManifest::WasmComponent(runtime_manifest) |
        PluginRuntimeManifest::WasmModule(runtime_manifest) => {
            // Check wasm artifact exists
            let artifact_path = install_root.join(&runtime_manifest.artifact);
            if !artifact_path.exists() {
                return Err(format!(
                    "wasm artifact '{}' not found in package",
                    runtime_manifest.artifact
                ));
            }
        }
        PluginRuntimeManifest::Process(runtime_manifest) => {
            // Check program executable exists
            let program_path = install_root.join(&runtime_manifest.program);
            if !program_path.exists() {
                return Err(format!(
                    "process program '{}' not found in package",
                    runtime_manifest.program
                ));
            }
        }
        PluginRuntimeManifest::Docker(runtime_manifest) => {
            // For docker, check if image name or oci_archive or compose_file is specified
            if runtime_manifest.image.is_none()
                && runtime_manifest.oci_archive.is_none()
                && runtime_manifest.compose_file.is_none()
            {
                return Err("docker runtime must specify image, oci_archive, or compose_file".to_string());
            }

            // If oci_archive specified, check it exists
            if let Some(ref archive) = runtime_manifest.oci_archive {
                let archive_path = install_root.join(archive);
                if !archive_path.exists() {
                    return Err(format!(
                        "docker oci_archive '{}' not found in package",
                        archive
                    ));
                }
            }

            // If compose_file specified, check it exists
            if let Some(ref compose) = runtime_manifest.compose_file {
                let compose_path = install_root.join(compose);
                if !compose_path.exists() {
                    return Err(format!(
                        "docker compose_file '{}' not found in package",
                        compose
                    ));
                }
            }
        }
    }
    Ok(())
}


pub async fn register_plugin_from_zip_bytes(
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

    // Perform basic validation at registration time to catch obvious errors
    // Full validation with lifecycle commands happens during materialize
    validate_prepared_runtime_basic(&manifest, &package_dir)?;

    let now = chrono::Utc::now();
    let plugin_existing = plugin_registry::Entity::find_by_id(manifest.id.clone())
        .one(db)
        .await
        .map_err(|e| format!("failed to load plugin registry: {}", e))?;

    if let Some(existing) = plugin_existing {
        let mut active: plugin_registry::ActiveModel = existing.into();
        active.display_name = Set(manifest.name.clone());
        active.source_kind = Set(options.source_kind.clone());
        active.current_version = Set(Some(manifest.version.clone()));
        active.install_status = Set(INSTALL_STATUS_PENDING.to_string());
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
            runtime_kind: Set("".to_string()),
            source_kind: Set(options.source_kind.clone()),
            current_version: Set(Some(manifest.version.clone())),
            install_status: Set(INSTALL_STATUS_PENDING.to_string()),
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
        install_status: Set(INSTALL_STATUS_PENDING.to_string()),
        installed_at: Set(None),
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
        action: Set("register".to_string()),
        message: Set(format!(
            "Registered plugin {} {} (pending)",
            manifest.id, manifest.version
        )),
        actor_user_id: Set(options.actor_user_id),
        created_at: Set(now),
    };
    audit_model
        .insert(db)
        .await
        .map_err(|e| format!("failed to insert plugin audit log: {}", e))?;

    let prepared_runtime_kind = manifest.runtime_kind().as_kebab_str().to_string();
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

pub async fn materialize_plugin_from_zip(
    db: &DatabaseConnection,
    packages_root: &Path,
    plugin_id: &str,
    version: &str,
    download_url_overrides: &HashMap<String, String>,
    actor_user_id: Option<String>,
) -> Result<InstallPluginResult, String> {
    let package_dir = packages_root.join(plugin_id).join(version);
    let manifest = read_manifest_from_package_dir(&package_dir).await?;

    let install_commands = manifest.install_commands.as_deref().unwrap_or(&[]);
    execute_commands_with_overrides(
        install_commands,
        &package_dir,
        download_url_overrides,
        &manifest.version,
        &manifest.id,
        &manifest.name,
    )
    .await?;

    let prepared_kind = validate_prepared_runtime(&manifest, &package_dir)?;

    let prev_version = plugin_registry::Entity::find_by_id(plugin_id.to_string())
        .one(db)
        .await
        .map_err(|e| format!("failed to load plugin registry: {}", e))?
        .and_then(|r| r.current_version);

        if let Some(ref old_ver) = prev_version {
            if old_ver != version {
                let upgrade_commands = manifest.upgrade_commands.as_deref().unwrap_or(&[]);
                execute_commands_with_overrides(
                    upgrade_commands,
                    &package_dir,
                    download_url_overrides,
                    &manifest.version,
                    &manifest.id,
                    &manifest.name,
                )
                .await?;

            let old_dir = packages_root.join(plugin_id).join(old_ver);
            if tokio::fs::try_exists(&old_dir).await.unwrap_or(false) {
                tokio::fs::remove_dir_all(&old_dir).await.map_err(|e| {
                    format!("failed to remove old version dir '{}': {}", old_dir.display(), e)
                })?;
            }
        }
    }

    let now = chrono::Utc::now();
    let plugin_existing = plugin_registry::Entity::find_by_id(plugin_id.to_string())
        .one(db)
        .await
        .map_err(|e| format!("failed to load plugin registry: {}", e))?;

    if let Some(existing) = plugin_existing {
        let mut active: plugin_registry::ActiveModel = existing.into();
        active.current_version = Set(Some(version.to_string()));
        active.install_status = Set(INSTALL_STATUS_INSTALLED.to_string());
        active.enabled = Set(false);
        active.runtime_kind = Set(prepared_kind.clone());
        active.updated_at = Set(now);
        active.update(db)
            .await
            .map_err(|e| format!("failed to update plugin registry: {}", e))?;
    }

    if let Some(version_record) = plugin_version::Entity::find()
        .filter(plugin_version::Column::PluginId.eq(plugin_id))
        .filter(plugin_version::Column::Version.eq(version))
        .one(db)
        .await
        .map_err(|e| format!("failed to find plugin version: {}", e))?
    {
        let mut active: plugin_version::ActiveModel = version_record.into();
        active.install_status = Set(INSTALL_STATUS_INSTALLED.to_string());
        active.installed_at = Set(Some(now));
        active.update(db)
            .await
            .map_err(|e| format!("failed to update plugin version: {}", e))?;
    }

    let audit_model = plugin_audit_log::ActiveModel {
        id: Set(uuid::Uuid::now_v7().to_string()),
        plugin_id: Set(plugin_id.to_string()),
        action: Set("materialize".to_string()),
        message: Set(format!("Materialized plugin {} {}", plugin_id, version)),
        actor_user_id: Set(actor_user_id),
        created_at: Set(now),
    };
    audit_model.insert(db)
        .await
        .map_err(|e| format!("failed to insert plugin audit log: {}", e))?;

    Ok(InstallPluginResult {
        plugin_id: plugin_id.to_string(),
        version: version.to_string(),
        package_dir: package_dir.to_string_lossy().to_string(),
        checksum_sha256: String::new(),
        prepared_runtime_kind: prepared_kind,
    })
}

// ---- 辅助函数 ----

fn strip_prefix_components(path: &Path, n: u32) -> Option<PathBuf> {
    let comps: Vec<_> = path.components().collect();
    if comps.len() <= n as usize {
        return None;
    }
    let mut result = PathBuf::new();
    for comp in &comps[n as usize..] {
        result.push(comp);
    }
    Some(result)
}

async fn download_to_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::get(url).await
        .map_err(|e| format!("failed to download '{}': {}", url, e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("download failed with status {} for '{}'", status, url));
    }
    response.bytes().await
        .map(|b| b.to_vec())
        .map_err(|e| format!("failed to read response body from '{}': {}", url, e))
}



fn detect_archive_format(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Some("tar.gz")
    } else if lower.ends_with(".zip") {
        Some("zip")
    } else if lower.ends_with(".gz") {
        Some("gzip")
    } else {
        None
    }
}

fn extract_archive(archive_path: &Path, dest_dir: &Path, strip: u32) -> Result<(), String> {
    let path_str = archive_path.to_string_lossy();
    match detect_archive_format(&path_str) {
        Some("zip") => extract_zip_with_strip(archive_path, dest_dir, strip),
        Some("tar.gz") => extract_tar_gz_with_strip(archive_path, dest_dir, strip),
        Some("gzip") => extract_gzip_single(archive_path, dest_dir),
        _ => Err(format!("unsupported archive format: {}", path_str)),
    }
}

fn extract_zip_with_strip(archive_path: &Path, dest_dir: &Path, strip: u32) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|e| format!("failed to open '{}': {}", archive_path.display(), e))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| format!("invalid zip '{}': {}", archive_path.display(), e))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("failed to read zip entry {}: {}", i, e))?;
        let Some(entry_path) = entry.enclosed_name() else { continue };

        let out_path = if strip > 0 {
            match strip_prefix_components(&entry_path, strip) {
                Some(p) => dest_dir.join(p),
                None => continue,
            }
        } else {
            dest_dir.join(entry_path)
        };

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| {
                format!("failed to create dir '{}': {}", out_path.display(), e)
            })?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("failed to create parent dir '{}': {}", parent.display(), e)
            })?;
        }
        let mut buffer = Vec::new();
        entry.read_to_end(&mut buffer)
            .map_err(|e| format!("failed to read zip entry '{}': {}", entry.name(), e))?;
        std::fs::write(&out_path, buffer)
            .map_err(|e| format!("failed to write '{}': {}", out_path.display(), e))?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

fn extract_tar_gz_with_strip(archive_path: &Path, dest_dir: &Path, strip: u32) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|e| format!("failed to open '{}': {}", archive_path.display(), e))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    for entry in archive.entries()
        .map_err(|e| format!("failed to read tar entries from '{}': {}", archive_path.display(), e))?
    {
        let mut entry = entry
            .map_err(|e| format!("failed to read tar entry: {}", e))?;
        let entry_path = entry.path()
            .map_err(|e| format!("failed to read tar entry path: {}", e))?
            .into_owned();

        let out_path = if strip > 0 {
            match strip_prefix_components(&entry_path, strip) {
                Some(p) => dest_dir.join(p),
                None => continue,
            }
        } else {
            dest_dir.join(&entry_path)
        };

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("failed to create dir '{}': {}", parent.display(), e)
            })?;
        }

        entry.unpack(&out_path)
            .map_err(|e| format!("failed to unpack '{}': {}", out_path.display(), e))?;
    }
    Ok(())
}

fn extract_archive_with_format(
    archive_path: &Path,
    dest_dir: &Path,
    format: &str,
    strip: u32,
) -> Result<(), String> {
    match format {
        "zip" => extract_zip_with_strip(archive_path, dest_dir, strip),
        "tar.gz" | "tgz" => extract_tar_gz_with_strip(archive_path, dest_dir, strip),
        "gzip" => extract_gzip_single(archive_path, dest_dir),
        other => detect_and_extract(archive_path, dest_dir, strip, other),
    }
}

fn detect_and_extract(
    archive_path: &Path,
    dest_dir: &Path,
    strip: u32,
    _format_hint: &str,
) -> Result<(), String> {
    // fallback to auto-detection
    match detect_archive_format(&archive_path.to_string_lossy()) {
        Some("zip") => extract_zip_with_strip(archive_path, dest_dir, strip),
        Some("tar.gz") => extract_tar_gz_with_strip(archive_path, dest_dir, strip),
        Some("gzip") => extract_gzip_single(archive_path, dest_dir),
        _ => Err(format!(
            "unsupported archive format: {}",
            archive_path.display()
        )),
    }
}

fn extract_gzip_single(archive_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let input = File::open(archive_path)
        .map_err(|e| format!("failed to open '{}': {}", archive_path.display(), e))?;
    let mut decoder = GzDecoder::new(input);

    let stem = archive_path.file_stem()
        .ok_or_else(|| format!("cannot determine output name for '{}'", archive_path.display()))?;
    let out_path = dest_dir.join(stem);

    let mut output = File::create(&out_path)
        .map_err(|e| format!("failed to create '{}': {}", out_path.display(), e))?;
    std::io::copy(&mut decoder, &mut output)
        .map_err(|e| format!("failed to decompress '{}': {}", archive_path.display(), e))?;
    Ok(())
}

async fn execute_commands_with_overrides(
    steps: &[PluginSetupStep],
    pkg_dir: &Path,
    overrides: &HashMap<String, String>,
    version: &str,
    plugin_id: &str,
    name: &str,
) -> Result<(), String> {
    let mut dl_idx = 0usize;
    for step in steps {
        // check condition
        if let Some(ref condition) = step.if_condition {
            let should_run = check_setup_condition(condition, pkg_dir)?;
            if !should_run {
                continue;
            }
        }

        // resolve template variables in url, source, dest, symlink_target
        let mut resolved = step.clone();
        if let Some(ref url) = resolved.url {
            resolved.url = Some(resolve_template_vars(url, version, plugin_id, name));
        }
        if let Some(ref source) = resolved.source {
            resolved.source = Some(resolve_template_vars(source, version, plugin_id, name));
        }
        if let Some(ref dest) = resolved.dest {
            resolved.dest = Some(resolve_template_vars(dest, version, plugin_id, name));
        }
        if let Some(ref target) = resolved.symlink_target {
            resolved.symlink_target =
                Some(resolve_template_vars(target, version, plugin_id, name));
        }

        if resolved.action == PluginSetupAction::Download {
            let key = dl_idx.to_string();
            if let Some(override_url) = overrides.get(&key) {
                resolved.url = Some(override_url.clone());
            }
            dl_idx += 1;
        }

        execute_setup_step(&resolved, pkg_dir, pkg_dir).await?;
    }
    Ok(())
}

async fn execute_setup_step(
    step: &PluginSetupStep,
    work_dir: &Path,
    pkg_dir: &Path,
) -> Result<(), String> {
    let src = step.source.as_deref().map(|s| {
        let p = Path::new(s);
        if p.is_absolute() { p.to_path_buf() } else { work_dir.join(s) }
    });
    let dst = step.dest.as_deref().map(|d| pkg_dir.join(d));

    match step.action {
        PluginSetupAction::Download => {
            let url = step.url.as_deref().ok_or("download action requires url")?;
            let dest = dst.ok_or("download action requires dest")?;

            // download
            let data = download_to_bytes(url).await?;

            // verify checksum
            if let Some(ref expected_sha) = step.sha256 {
                verify_sha256(&data, expected_sha)?;
            }

            // auto-extract or write file
            if let Some(ref archive_fmt) = step.archive.clone() {
                let dest_path = dest.clone();
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!(
                            "failed to create dir '{}': {}",
                            parent.display(),
                            e
                        )
                    })?;
                }
                std::fs::write(&dest_path, &data).map_err(|e| {
                    format!("failed to write archive '{}': {}", dest_path.display(), e)
                })?;
                let extract_dir = dest_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| pkg_dir.to_path_buf());
                let strip = step.strip_components.unwrap_or(0);
                let archive_fmt = archive_fmt.clone();
                let dest_for_extract = dest_path.clone();
                tokio::task::spawn_blocking(move || {
                    extract_archive_with_format(&dest_for_extract, &extract_dir, &archive_fmt, strip)
                })
                .await
                .map_err(|e| format!("extract task failed: {}", e))?
                .map_err(|e| e)?;
                // clean up archive
                let _ = std::fs::remove_file(&dest_path);
            } else {
                // write file directly
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!(
                            "failed to create dir '{}': {}",
                            parent.display(),
                            e
                        )
                    })?;
                }
                std::fs::write(&dest, &data).map_err(|e| {
                    format!("failed to write '{}': {}", dest.display(), e)
                })?;
            }
        }
        PluginSetupAction::Extract => {
            let src_path = src.ok_or("extract action requires source")?;
            let dest_dir = dst.unwrap_or_else(|| pkg_dir.to_path_buf());
            let strip = step.strip_components.unwrap_or(0);
            tokio::task::spawn_blocking(move || extract_archive(&src_path, &dest_dir, strip))
                .await
                .map_err(|e| format!("extract task failed: {}", e))?
                .map_err(|e| e)?;
        }
        PluginSetupAction::Move => {
            let src_path = src.ok_or("move action requires source")?;
            let dst_path = dst.ok_or("move action requires dest")?;
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create dir '{}': {}", parent.display(), e))?;
            }
            std::fs::rename(&src_path, &dst_path)
                .map_err(|e| format!("move '{}' to '{}' failed: {}", src_path.display(), dst_path.display(), e))?;
        }
        PluginSetupAction::Copy => {
            let src_path = src.ok_or("copy action requires source")?;
            let dst_path = dst.ok_or("copy action requires dest")?;
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create dir '{}': {}", parent.display(), e))?;
            }
            tokio::fs::copy(&src_path, &dst_path).await
                .map_err(|e| format!("copy '{}' to '{}' failed: {}", src_path.display(), dst_path.display(), e))?;
        }
        PluginSetupAction::Symlink => {
            #[cfg(unix)]
            {
                let link_path = dst.ok_or("symlink action requires dest")?;
                let target = step
                    .symlink_target
                    .as_deref()
                    .or(step.source.as_deref())
                    .ok_or("symlink action requires symlink_target or source")?;
                if let Some(parent) = link_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("failed to create dir '{}': {}", parent.display(), e))?;
                }
                std::os::unix::fs::symlink(target, &link_path)
                    .map_err(|e| format!("symlink '{}' -> '{}' failed: {}", link_path.display(), target, e))?;
            }
            #[cfg(not(unix))]
            return Err("symlink is not supported on this platform".to_string());
        }
        PluginSetupAction::Chmod => {
            #[cfg(unix)]
            {
                let target = src.ok_or("chmod action requires source")?;
                let mode_str = step.mode.as_deref().ok_or("chmod action requires mode")?;
                let mode = u32::from_str_radix(mode_str, 8)
                    .map_err(|_| format!("invalid mode '{}': must be octal", mode_str))?;
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))
                    .map_err(|e| format!("chmod '{}' to '{}' failed: {}", target.display(), mode_str, e))?;
            }
        }
        PluginSetupAction::Mkdir => {
            let dir = dst.or_else(|| step.source.as_ref().map(|s| pkg_dir.join(s)))
                .ok_or("mkdir action requires source or dest")?;
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("mkdir '{}' failed: {}", dir.display(), e))?;
        }
        PluginSetupAction::Remove => {
            let target = dst.or_else(|| step.source.as_ref().map(|s| {
                let p = Path::new(s);
                if p.is_absolute() { p.to_path_buf() } else { pkg_dir.join(s) }
            })).ok_or("remove action requires source or dest")?;
            if tokio::fs::try_exists(&target).await.unwrap_or(false) {
                if target.is_dir() {
                    tokio::fs::remove_dir_all(&target).await
                        .map_err(|e| format!("remove dir '{}' failed: {}", target.display(), e))?;
                } else {
                    tokio::fs::remove_file(&target).await
                        .map_err(|e| format!("remove file '{}' failed: {}", target.display(), e))?;
                }
            }
        }
        PluginSetupAction::Touch => {
            let target = dst.ok_or("touch action requires dest")?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create dir '{}': {}", parent.display(), e))?;
            }
            std::fs::write(&target, [])
                .map_err(|e| format!("touch '{}' failed: {}", target.display(), e))?;
        }
        PluginSetupAction::WriteFile => {
            let target = dst.ok_or("write_file action requires dest")?;
            let content = step.content.as_deref().unwrap_or("");
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create dir '{}': {}", parent.display(), e))?;
            }
            std::fs::write(&target, content)
                .map_err(|e| format!("write_file '{}' failed: {}", target.display(), e))?;
        }
        PluginSetupAction::Run => {
            let bin_rel = step.source.as_deref().ok_or("run action requires source")?;
            let bin_path = pkg_dir.join(bin_rel);
            if !tokio::fs::try_exists(&bin_path).await.unwrap_or(false) {
                return Err(format!("run target '{}' does not exist", bin_path.display()));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let meta = std::fs::metadata(&bin_path)
                    .map_err(|e| format!("failed to stat '{}': {}", bin_path.display(), e))?;
                if meta.permissions().mode() & 0o111 == 0 {
                    std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755))
                        .map_err(|e| format!("failed to chmod '{}': {}", bin_path.display(), e))?;
                }
            }
            let args = step.args.as_deref().unwrap_or(&[]);
            let output = tokio::process::Command::new(&bin_path)
                .args(args)
                .current_dir(pkg_dir)
                .output()
                .await
                .map_err(|e| format!("failed to execute '{}': {}", bin_path.display(), e))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "command '{}' failed with exit code {:?}: {}",
                    bin_path.display(),
                    output.status.code(),
                    stderr.trim()
                ));
            }
        }
    }
    Ok(())
}
