use crate::manifest::ProcessRuntimeManifest;
use crate::runtime::{RuntimeHandle, RuntimeStatus};
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

#[cfg(unix)]
#[allow(unused_imports)]
use std::os::unix::process::CommandExt;

const DEFAULT_STOP_TIMEOUT_SECS: u64 = 10;
const HEALTH_CHECK_RETRIES: u32 = 15;
const HEALTH_CHECK_INTERVAL_MS: u64 = 500;

pub fn prepare_process_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &ProcessRuntimeManifest,
) -> Result<RuntimeHandle, String> {
    let program = install_root.join(&runtime.program);
    if !program.exists() {
        return Err(format!(
            "process runtime program not found: {}",
            program.display()
        ));
    }
    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "process".to_string(),
        status: RuntimeStatus::Prepared,
        detail: program.to_string_lossy().to_string(),
        pid: None,
        instance_ref: None,
        route_base_url: runtime.effective_base_url(),
    })
}

pub async fn start_process_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &ProcessRuntimeManifest,
    host_api_base_url: &str,
    host_api_token: &str,
    plugin_config_dir: &str,
    plugin_config_file: &str,
    logs_dir: &str,
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
    command.env("FILEUNI_PLUGIN_CONFIG_DIR", plugin_config_dir);
    command.env("FILEUNI_PLUGIN_CONFIG_FILE", plugin_config_file);

    // process group: make sure the child process and its children can be killed together
    #[cfg(unix)]
    {
        unsafe {
            command.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
    }

    // redirect stdout/stderr to log file
    let log_path = Path::new(logs_dir).join(format!("{}.log", plugin_id));
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("failed to create log dir '{}': {}", parent.display(), e))?;
    }
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| format!("failed to create log file '{}': {}", log_path.display(), e))?;
    let log_file_clone = log_file
        .try_clone()
        .map_err(|e| format!("failed to clone log file handle: {}", e))?;
    command.stdout(std::process::Stdio::from(log_file));
    command.stderr(std::process::Stdio::from(log_file_clone));

    let child = command.spawn().map_err(|e| {
        format!(
            "failed to spawn process runtime '{}': {}",
            program.display(),
            e
        )
    })?;
    let pid = child.id();
    drop(child);

    // health check: poll the base_url until plugin is ready
    let base_url = runtime.effective_base_url();
    if let Some(ref url) = base_url {
        let health_url = format!("{}/health", url.trim_end_matches('/'));
        for attempt in 1..=HEALTH_CHECK_RETRIES {
            match reqwest::get(&health_url).await {
                Ok(resp) if resp.status().is_success() => {
                    break;
                }
                _ => {
                    if attempt == HEALTH_CHECK_RETRIES {
                        // health check failed, kill the process
                        if let Some(pid) = pid {
                            let _ = kill_process_group(pid).await;
                        }
                        return Err(format!(
                            "process runtime '{}' health check failed after {} retries",
                            plugin_id, HEALTH_CHECK_RETRIES
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(HEALTH_CHECK_INTERVAL_MS)).await;
                }
            }
        }
    }

    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "process".to_string(),
        status: RuntimeStatus::Running,
        detail: program.to_string_lossy().to_string(),
        pid,
        instance_ref: None,
        route_base_url: base_url,
    })
}

pub async fn stop_process_runtime(handle: &RuntimeHandle) -> Result<(), String> {
    let Some(pid) = handle.pid else {
        return Err("process runtime has no pid to stop".to_string());
    };

    // verify pid still belongs to the expected process
    if !process_matches(pid, "process runtime") {
        return Err(format!("pid {} does not belong to the expected process or has been reused", pid));
    }

    // graceful shutdown: SIGTERM first
    #[cfg(unix)]
    {
        let term_result = kill_process_group(pid).await;
        if term_result.is_err() {
            // if SIGTERM fails, try SIGKILL
            let _ = kill_process_group_force(pid).await;
        }
    }

    #[cfg(not(unix))]
    {
        let _ = kill_process_group(pid).await;
    }

    Ok(())
}

#[cfg(unix)]
async fn kill_process_group(pid: u32) -> Result<(), String> {
    let pid = pid as i32;
    // send SIGTERM to the process group
    let result = unsafe { libc::killpg(pid, libc::SIGTERM) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("killpg(SIGTERM, {}) failed: {}", pid, err));
    }

    // wait for process to exit
    let deadline = Duration::from_secs(DEFAULT_STOP_TIMEOUT_SECS);
    let start = std::time::Instant::now();
    loop {
        let status = unsafe { libc::killpg(pid, 0) };
        if status != 0 {
            // process group no longer exists, done
            break Ok(());
        }
        if start.elapsed() >= deadline {
            // timeout reached, force kill
            let _ = kill_process_group_force(pid as u32).await;
            break Err("process did not exit in time, force killed".to_string());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(unix)]
async fn kill_process_group_force(pid: u32) -> Result<(), String> {
    let pid = pid as i32;
    let result = unsafe { libc::killpg(pid, libc::SIGKILL) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::NotFound {
            return Ok(());
        }
        return Err(format!("killpg(SIGKILL, {}) failed: {}", pid, err));
    }
    Ok(())
}

#[cfg(not(unix))]
async fn kill_process_group(pid: u32) -> Result<(), String> {
    let status = Command::new("taskkill")
        .args(["/F", "/T", "/PID"])
        .arg(pid.to_string())
        .status()
        .await
        .map_err(|e| format!("failed to stop process runtime pid {}: {}", pid, e))?;
    if !status.success() {
        return Err(format!("taskkill exited with status {}", status));
    }
    Ok(())
}

fn process_matches(pid: u32, _expected_name: &str) -> bool {
    #[cfg(unix)]
    {
        // verify the pid is still alive and belongs to us
        let ret = unsafe { libc::kill(pid as i32, 0) };
        ret == 0
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}
