use crate::manifest::ProcessRuntimeManifest;
use crate::runtime::{RuntimeHandle, RuntimeStatus};
use std::path::Path;
use tokio::process::Command;

pub fn prepare_process_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &ProcessRuntimeManifest,
) -> Result<RuntimeHandle, String> {
    let program = install_root.join(&runtime.program);
    if !program.exists() {
        return Err(format!("process runtime program not found: {}", program.display()));
    }
    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "process".to_string(),
        status: RuntimeStatus::Prepared,
        detail: program.to_string_lossy().to_string(),
        pid: None,
        instance_ref: None,
        route_base_url: runtime.base_url.clone(),
    })
}

pub async fn start_process_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &ProcessRuntimeManifest,
    host_api_base_url: &str,
    host_api_token: &str,
) -> Result<RuntimeHandle, String> {
    let program = install_root.join(&runtime.program);
    let mut command = Command::new(&program);
    if let Some(args) = &runtime.args {
        command.args(args);
    }
    if let Some(cwd) = &runtime.cwd {
        command.current_dir(install_root.join(cwd));
    } else {
        command.current_dir(install_root);
    }
    if let Some(env) = &runtime.env {
        command.envs(env);
    }
    command.env("FILEUNI_PLUGIN_ID", plugin_id);
    command.env("FILEUNI_PLUGIN_HOST_API_BASE_URL", host_api_base_url);
    command.env("FILEUNI_PLUGIN_HOST_API_TOKEN", host_api_token);
    let child = command
        .spawn()
        .map_err(|e| format!("failed to spawn process runtime '{}': {}", program.display(), e))?;
    let pid = child.id();
    drop(child);
    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "process".to_string(),
        status: RuntimeStatus::Running,
        detail: program.to_string_lossy().to_string(),
        pid,
        instance_ref: None,
        route_base_url: runtime.base_url.clone(),
    })
}

pub async fn stop_process_runtime(handle: &RuntimeHandle) -> Result<(), String> {
    let Some(pid) = handle.pid else {
        return Err("process runtime has no pid to stop".to_string());
    };
    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .await
        .map_err(|e| format!("failed to stop process runtime pid {}: {}", pid, e))?;
    if !status.success() {
        return Err(format!("kill exited with status {}", status));
    }
    Ok(())
}
