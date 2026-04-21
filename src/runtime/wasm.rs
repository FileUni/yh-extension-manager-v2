use crate::manifest::WasmRuntimeManifest;
use crate::runtime::{RuntimeHandle, RuntimeStatus};
use std::path::Path;
use tokio::process::Command;

pub fn prepare_wasm_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &WasmRuntimeManifest,
    runtime_kind: &str,
) -> Result<RuntimeHandle, String> {
    let artifact = install_root.join(&runtime.artifact);
    if !artifact.exists() {
        return Err(format!("wasm runtime artifact not found: {}", artifact.display()));
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
) -> Result<RuntimeHandle, String> {
    let artifact = install_root.join(&runtime.artifact);
    if !artifact.exists() {
        return Err(format!("wasm module artifact not found: {}", artifact.display()));
    }

    let runner = std::env::var("FILEUNI_WASM_RUNNER").unwrap_or_else(|_| "wasmtime".to_string());
    let mut command = Command::new(&runner);
    command.arg(&artifact);
    if let Some(args) = &runtime.args {
        command.args(args);
    }
    if let Some(env) = &runtime.env {
        command.envs(env);
    }
    command.env("FILEUNI_PLUGIN_ID", plugin_id);
    command.env("FILEUNI_PLUGIN_HOST_API_BASE_URL", host_api_base_url);
    command.env("FILEUNI_PLUGIN_HOST_API_TOKEN", host_api_token);
    let child = command
        .spawn()
        .map_err(|e| format!("failed to spawn wasm module runner '{}': {}", runner, e))?;
    let pid = child.id();
    drop(child);

    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "wasm-module".to_string(),
        status: RuntimeStatus::Running,
        detail: artifact.to_string_lossy().to_string(),
        pid,
        instance_ref: Some(runner),
        route_base_url: None,
    })
}

pub async fn start_wasm_component_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &WasmRuntimeManifest,
    host_api_base_url: &str,
    host_api_token: &str,
) -> Result<RuntimeHandle, String> {
    let artifact = install_root.join(&runtime.artifact);
    if !artifact.exists() {
        return Err(format!("wasm component artifact not found: {}", artifact.display()));
    }

    let runner = std::env::var("FILEUNI_WASM_COMPONENT_RUNNER")
        .unwrap_or_else(|_| std::env::var("FILEUNI_WASM_RUNNER").unwrap_or_else(|_| "wasmtime".to_string()));
    let mut command = Command::new(&runner);
    command.arg("run").arg(&artifact);
    if let Some(args) = &runtime.args {
        command.args(args);
    }
    if let Some(env) = &runtime.env {
        command.envs(env);
    }
    command.env("FILEUNI_PLUGIN_ID", plugin_id);
    command.env("FILEUNI_PLUGIN_HOST_API_BASE_URL", host_api_base_url);
    command.env("FILEUNI_PLUGIN_HOST_API_TOKEN", host_api_token);
    let child = command
        .spawn()
        .map_err(|e| format!("failed to spawn wasm component runner '{}': {}", runner, e))?;
    let pid = child.id();
    drop(child);

    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "wasm-component".to_string(),
        status: RuntimeStatus::Running,
        detail: artifact.to_string_lossy().to_string(),
        pid,
        instance_ref: Some(runner),
        route_base_url: None,
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
