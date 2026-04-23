use crate::manifest::DockerRuntimeManifest;
use crate::runtime::{RuntimeHandle, RuntimeStatus};
use std::path::Path;
use tokio::process::Command;

pub struct DockerRuntimeLaunchContext<'a> {
    pub docker_engine_command: &'a str,
    pub host_api_base_url: &'a str,
    pub host_api_token: &'a str,
    pub plugin_config_dir: &'a str,
    pub plugin_config_file: &'a str,
}

pub fn prepare_docker_runtime(
    plugin_id: &str,
    runtime: &DockerRuntimeManifest,
) -> Result<RuntimeHandle, String> {
    let detail = runtime
        .image
        .clone()
        .or_else(|| runtime.oci_archive.clone())
        .or_else(|| runtime.compose_file.clone())
        .ok_or_else(|| "docker runtime has no image/archive/compose source".to_string())?;
    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "docker".to_string(),
        status: RuntimeStatus::Prepared,
        detail,
        pid: None,
        instance_ref: None,
        route_base_url: runtime.base_url.clone().or_else(|| {
            runtime
                .ports
                .as_ref()
                .and_then(|ports| ports.first())
                .map(|port| format!("http://127.0.0.1:{}", port.host.unwrap_or(port.container)))
        }),
    })
}

pub async fn start_docker_runtime(
    plugin_id: &str,
    install_root: &Path,
    runtime: &DockerRuntimeManifest,
    launch: &DockerRuntimeLaunchContext<'_>,
) -> Result<RuntimeHandle, String> {
    let instance_name = format!("fileuni-plg-{}", plugin_id.replace('.', "-"));
    if let Some(archive) = &runtime.oci_archive {
        let status = Command::new(launch.docker_engine_command)
            .args(["load", "-i"])
            .arg(install_root.join(archive))
            .status()
            .await
            .map_err(|e| format!("failed to load OCI archive: {}", e))?;
        if !status.success() {
            return Err(format!("docker load exited with status {}", status));
        }
    }
    if let Some(compose_file) = &runtime.compose_file {
        let status = Command::new(launch.docker_engine_command)
            .args(["compose", "-f"])
            .arg(install_root.join(compose_file))
            .args(["up", "-d"])
            .status()
            .await
            .map_err(|e| format!("failed to start docker compose runtime: {}", e))?;
        if !status.success() {
            return Err(format!("docker compose up exited with status {}", status));
        }
        return Ok(RuntimeHandle {
            plugin_id: plugin_id.to_string(),
            runtime_kind: "docker".to_string(),
            status: RuntimeStatus::Running,
            detail: install_root
                .join(compose_file)
                .to_string_lossy()
                .to_string(),
            pid: None,
            instance_ref: Some(instance_name),
            route_base_url: runtime.base_url.clone(),
        });
    }

    let image = runtime
        .image
        .as_ref()
        .ok_or_else(|| "docker image is required for docker run mode".to_string())?;
    let mut command = Command::new(launch.docker_engine_command);
    command.args(["run", "-d", "--rm", "--name", &instance_name]);
    if let Some(ports) = &runtime.ports {
        for port in ports {
            let host = port.host.unwrap_or(port.container);
            let protocol = port.protocol.as_deref().unwrap_or("tcp");
            command
                .arg("-p")
                .arg(format!("{}:{}/{}", host, port.container, protocol));
        }
    }
    if let Some(volumes) = &runtime.volumes {
        for volume in volumes {
            let suffix = if volume.read_only.unwrap_or(false) {
                ":ro"
            } else {
                ""
            };
            command
                .arg("-v")
                .arg(format!("{}:{}{}", volume.source, volume.target, suffix));
        }
    }
    if let Some(env) = &runtime.env {
        for (key, value) in env {
            command.arg("-e").arg(format!("{}={}", key, value));
        }
    }
    command
        .arg("-e")
        .arg(format!("FILEUNI_PLUGIN_ID={}", plugin_id));
    command.arg("-e").arg(format!(
        "FILEUNI_PLUGIN_HOST_API_BASE_URL={}",
        launch.host_api_base_url
    ));
    command.arg("-e").arg(format!(
        "FILEUNI_PLUGIN_HOST_API_TOKEN={}",
        launch.host_api_token
    ));
    command.arg("-e").arg(format!(
        "FILEUNI_PLUGIN_CONFIG_DIR={}",
        launch.plugin_config_dir
    ));
    command.arg("-e").arg(format!(
        "FILEUNI_PLUGIN_CONFIG_FILE={}",
        launch.plugin_config_file
    ));
    command.arg(image);
    if let Some(cmd) = &runtime.command {
        command.args(cmd);
    }
    if let Some(args) = &runtime.args {
        command.args(args);
    }
    let output = command
        .output()
        .await
        .map_err(|e| format!("failed to start docker runtime '{}': {}", image, e))?;
    if !output.status.success() {
        return Err(format!(
            "docker run exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(RuntimeHandle {
        plugin_id: plugin_id.to_string(),
        runtime_kind: "docker".to_string(),
        status: RuntimeStatus::Running,
        detail: image.clone(),
        pid: None,
        instance_ref: Some(instance_name),
        route_base_url: runtime.base_url.clone().or_else(|| {
            runtime
                .ports
                .as_ref()
                .and_then(|ports| ports.first())
                .map(|port| format!("http://127.0.0.1:{}", port.host.unwrap_or(port.container)))
        }),
    })
}

pub async fn stop_docker_runtime(
    handle: &RuntimeHandle,
    docker_engine_command: &str,
) -> Result<(), String> {
    let Some(instance_name) = &handle.instance_ref else {
        return Err("docker runtime has no container reference to stop".to_string());
    };
    let status = Command::new(docker_engine_command)
        .args(["stop", instance_name])
        .status()
        .await
        .map_err(|e| format!("failed to stop docker runtime '{}': {}", instance_name, e))?;
    if !status.success() {
        return Err(format!("docker stop exited with status {}", status));
    }
    Ok(())
}
