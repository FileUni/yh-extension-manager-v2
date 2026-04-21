# yh-extension-manager-v2

## 核心文件

- `src/manager.rs`
- `src/installer.rs`
- `src/host_api.rs`
- `src/public.rs`
- `src/runtime/wasm.rs`
- `src/runtime/process.rs`
- `src/runtime/docker.rs`
- `src/handlers.rs`
- `src/manifest.rs`
- `src/registry.rs`
- `src/entities/plugin_registry.rs`
- `src/entities/plugin_version.rs`
- `src/entities/plugin_permission_grant.rs`
- `src/entities/plugin_shared_record.rs`
- `src/entities/plugin_migration_state.rs`
- `src/entities/plugin_task.rs`
- `src/entities/plugin_nav_item.rs`

## 核心函数

- `manager::init_plugin_runtime_manager`
- `manager::read_config_snapshot`
- `installer::install_plugin_from_zip_bytes`
- `installer::read_manifest_from_zip_bytes`
- `installer::extract_plugin_zip_to_dir`
- `public::serve_plugin_ui_file`
- `public::proxy_plugin_api`
- `public::proxy_plugin_ws_inner`
- `host_api::ensure_sqlite_database`
- `host_api::upsert_shared_record`
- `host_api::execute_migration`
- `host_api::upsert_task`
- `host_api::upsert_nav_item`
- `handlers::start_plugin_runtime`
- `handlers::stop_plugin_runtime`
- `handlers::uninstall_plugin`
- `runtime::wasm::start_wasm_module_runtime`
- `runtime::wasm::start_wasm_component_runtime`

## 主要算法/实现点

- 安装链路先解压到 `packages/{plugin_id}/{version}`，再用 manifest 验证 runtime 物料存在性，最后才写 registry/version/audit。
- 公共 UI 挂载使用“文件优先，缺失回退 `index.html`”的 SPA 入口策略。
- HTTP 代理直接复用 runtime handle 的 `route_base_url`，并把用户上下文转发成 `X-Plugin-*` 头。
- WebSocket 代理维持双向 message 桥接，target 由 `route_base_url + /ws/*` 推导。
- SQLite broker 不把逻辑路径直接映射到任意 VFS 后端，而是固定落到宿主本地受控目录后返回 DSN。
- shared record broker 和 migration broker 都采用宿主统一表 + `plugin_id` 命名空间隔离，而不是让插件直接碰主库内部表。
- task 治理先走“注册表 + scheduler 第一版接管 + HTTP task hook 触发”，没有直接执行任意插件命令。
- nav 协议先存 `label/route/icon/visibility/sort_order/group_key/position/required_permission`，再由主前端消费渲染。

## 当前支持边界

- 安装：zip / 市场下载地址
- 运行时：`process` / `docker` / `wasm-module` / `wasm-component` 第一版外部 runner
- 挂载：UI / HTTP / WebSocket
- broker：KV namespace / shared record / migration state / SQLite / task / nav
- 管理：权限授予 / 启停 / 卸载 / 运行态查询 / 市场目录查询
