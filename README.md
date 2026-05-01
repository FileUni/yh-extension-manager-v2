# yh-extension-manager-v2

## Key Modules

| Path | Key Functions |
|------|---------------|
| `src/manager.rs` | `init_plugin_runtime_manager`, `start/stop_plugin_runtime`, `read_config_snapshot` |
| `src/installer.rs` | `install_plugin_from_zip_bytes`, `read_manifest_from_zip_bytes`, `extract_plugin_zip_to_dir` |
| `src/host_api.rs` | `ensure_sqlite_database`, `upsert_shared_record`, `execute_migration`, `upsert_task/nav_item` |
| `src/public.rs` | `serve_plugin_ui_file`, `proxy_plugin_api`, `proxy_plugin_ws_inner` |
| `src/handlers.rs` | HTTP handlers: start/stop/uninstall/status |
| `src/manifest.rs` | `PluginManifest`, `PluginRuntimeManifest`, permissions |
| `src/runtime/wasm.rs` | `start_wasm_module/component_runtime`, dev fallback |
| `src/runtime/process.rs` | `start_process_runtime`, spawn external executable |
| `src/runtime/docker.rs` | `start_docker_runtime`: OCI archive / compose / image modes |

## Wasm Runtime

`start_wasm_module_runtime` / `start_wasm_component_runtime`: spawn external runner (default `wasmtime`, configurable via `FILEUNI_WASM_RUNNER`). Env vars: `FILEUNI_PLUGIN_ID`, `FILEUNI_PLUGIN_HOST_API_*`, `FILEUNI_PLUGIN_CONFIG_*`. Dev fallback: if runner missing, spawn `{artifact_stem}-dev-server` from `runtime/` dir. Stop: `kill` pid.

## Process Runtime

`start_process_runtime`: spawn executable from manifest `program` field. Args, cwd, env from manifest; plugin context via env injection. Stop: `kill` pid.

## Docker Runtime

`start_docker_runtime` supports: (1) OCI archive: `docker load -i` then run; (2) Compose: `docker compose -f up -d`; (3) Image run: `docker run -d --rm --name fileuni-plg-{plugin_id}` with port/volume/env mapping. Stop: `docker stop` via instance_ref.

## Core Algorithms

- **Install**: unzip -> verify runtime artifact -> write registry/version/audit
- **UI**: file-first, fallback to `index.html` SPA
- **HTTP proxy**: reuse `route_base_url`, inject `X-Plugin-*` headers
- **WS proxy**: bidirectional bridge, target from `route_base_url + /ws/*`
- **SQLite**: fixed host path (not VFS), return DSN to plugin
- **Shared record/migration**: single host table with `plugin_id` namespace
- **Task**: host table + HTTP hook (no arbitrary exec)

## Runtime Manifest

```rust
enum PluginRuntimeManifest {
    WasmComponent(WasmRuntimeManifest),
    WasmModule(WasmRuntimeManifest),
    Process(ProcessRuntimeManifest),
    Docker(DockerRuntimeManifest),
}
```

## Broker Capabilities

KV (`KvRead/Write/Delete`), Shared record (`DbSharedRead/Write`), SQLite (`DbSqlite`), Task (`Scheduler`), Nav items (`label/route/icon/group_key/position/required_permission`).