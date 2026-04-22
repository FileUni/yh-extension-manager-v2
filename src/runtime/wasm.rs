use crate::manifest::WasmRuntimeManifest;
use crate::runtime::{RuntimeHandle, RuntimeStatus};
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub fn prepare_wasm_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &WasmRuntimeManifest,
    runtime_kind: &str,
) -> Result<RuntimeHandle, String> {
    let artifact = install_root.join(&runtime.artifact);
    if !artifact.exists() {
        return Err(format!(
            "wasm runtime artifact not found: {}",
            artifact.display()
        ));
    }
    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: runtime_kind.to_string(),
        status: RuntimeStatus::Prepared,
        detail: artifact.to_string_lossy().to_string(),
        pid: None,
        instance_ref: None,
        route_base_url: None,
    })
}

pub async fn start_wasm_module_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &WasmRuntimeManifest,
    host_api_base_url: &str,
    host_api_token: &str,
    plugin_config_dir: &str,
    plugin_config_file: &str,
) -> Result<RuntimeHandle, String> {
    let artifact = install_root.join(&runtime.artifact);
    if !artifact.exists() {
        return Err(format!(
            "wasm module artifact not found: {}",
            artifact.display()
        ));
    }

    let runner = std::env::var("FILEUNI_WASM_RUNNER").unwrap_or_else(|_| "wasmtime".to_string());
    let (pid, instance_ref, detail) = spawn_wasm_or_dev_fallback(
        &runner,
        install_root,
        &artifact,
        runtime,
        plugin_id,
        host_api_base_url,
        host_api_token,
        plugin_config_dir,
        plugin_config_file,
        false,
    )
    .await?;

    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "wasm-module".to_string(),
        status: RuntimeStatus::Running,
        detail,
        pid: Some(pid),
        instance_ref: Some(instance_ref),
        route_base_url: runtime.base_url.clone(),
    })
}

pub async fn start_wasm_component_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &WasmRuntimeManifest,
    host_api_base_url: &str,
    host_api_token: &str,
    plugin_config_dir: &str,
    plugin_config_file: &str,
) -> Result<RuntimeHandle, String> {
    let artifact = install_root.join(&runtime.artifact);
    if !artifact.exists() {
        return Err(format!(
            "wasm component artifact not found: {}",
            artifact.display()
        ));
    }

    let runner = std::env::var("FILEUNI_WASM_COMPONENT_RUNNER").unwrap_or_else(|_| {
        std::env::var("FILEUNI_WASM_RUNNER").unwrap_or_else(|_| "wasmtime".to_string())
    });
    let (pid, instance_ref, detail) = spawn_wasm_or_dev_fallback(
        &runner,
        install_root,
        &artifact,
        runtime,
        plugin_id,
        host_api_base_url,
        host_api_token,
        plugin_config_dir,
        plugin_config_file,
        true,
    )
    .await?;

    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "wasm-component".to_string(),
        status: RuntimeStatus::Running,
        detail,
        pid: Some(pid),
        instance_ref: Some(instance_ref),
        route_base_url: runtime.base_url.clone(),
    })
}

pub async fn stop_wasm_runtime(handle: &RuntimeHandle) -> Result<(), String> {
    let Some(pid) = handle.pid else {
        return Err("wasm runtime has no pid to stop".to_string());
    };
    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .await
        .map_err(|e| format!("failed to stop wasm runtime pid {}: {}", pid, e))?;
    if !status.success() {
        return Err(format!("kill exited with status {}", status));
    }
    Ok(())
}

async fn spawn_wasm_or_dev_fallback(
    runner: &str,
    install_root: &Path,
    artifact: &Path,
    runtime: &WasmRuntimeManifest,
    plugin_id: &str,
    host_api_base_url: &str,
    host_api_token: &str,
    plugin_config_dir: &str,
    plugin_config_file: &str,
    is_component: bool,
) -> Result<(u32, String, String), String> {
    let mut command = Command::new(runner);
    if is_component {
        command.arg("run");
    }
    command.arg(artifact);
    if let Some(args) = &runtime.args {
        command.args(args);
    }
    if let Some(env) = &runtime.env {
        command.envs(env);
    }
    command.env("FILEUNI_PLUGIN_ID", plugin_id);
    command.env("FILEUNI_PLUGIN_HOST_API_BASE_URL", host_api_base_url);
    command.env("FILEUNI_PLUGIN_HOST_API_TOKEN", host_api_token);
    command.env("FILEUNI_PLUGIN_CONFIG_DIR", plugin_config_dir);
    command.env("FILEUNI_PLUGIN_CONFIG_FILE", plugin_config_file);
    match command.spawn() {
        Ok(child) => {
            let pid = child.id().ok_or_else(|| format!("runner '{}' did not provide pid", runner))?;
            drop(child);
            return Ok((pid, runner.to_string(), artifact.to_string_lossy().to_string()));
        }
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("failed to spawn wasm runner '{}': {}", runner, error));
            }
        }
    }

    let stem = artifact
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "failed to derive wasm artifact stem".to_string())?;
    let dev_server = PathBuf::from(install_root).join("runtime").join(format!("{}-dev-server", stem));
    if !dev_server.exists() {
        return Err(format!(
            "failed to spawn wasm runner '{}' and no development fallback binary was found at {}",
            runner,
            dev_server.display()
        ));
    }
    let mut fallback = Command::new(&dev_server);
    if let Some(env) = &runtime.env {
        fallback.envs(env);
    }
    fallback.env("FILEUNI_PLUGIN_ID", plugin_id);
    fallback.env("FILEUNI_PLUGIN_HOST_API_BASE_URL", host_api_base_url);
    fallback.env("FILEUNI_PLUGIN_HOST_API_TOKEN", host_api_token);
    fallback.env("FILEUNI_PLUGIN_CONFIG_DIR", plugin_config_dir);
    fallback.env("FILEUNI_PLUGIN_CONFIG_FILE", plugin_config_file);
    let child = fallback
        .spawn()
        .map_err(|e| format!("failed to spawn development fallback '{}': {}", dev_server.display(), e))?;
    let pid = child.id().ok_or_else(|| format!("development fallback '{}' did not provide pid", dev_server.display()))?;
    drop(child);
    Ok((pid, format!("{} (dev-fallback)", runner), dev_server.to_string_lossy().to_string()))
}
