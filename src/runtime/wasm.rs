use crate::manifest::WasmRuntimeManifest;
use crate::runtime::{RuntimeHandle, RuntimeStatus};
use std::path::Path;

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
