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
- `installer::register_plugin_from_zip_bytes`
- `installer::materialize_plugin_from_zip`
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
- `handlers::materialize_plugin_from_zip`
- `runtime::wasm::start_wasm_module_runtime`
- `runtime::wasm::start_wasm_component_runtime`

## 主要算法/实现点

- ZIP 安装链路（两阶段）：(1) `register_plugin_from_zip_bytes` 解压到 `packages/{plugin_id}/{version}`，写入 registry/version/audit，标记 `pending`；(2) `materialize_plugin_from_zip` 校验 runtime 物料存在性、执行 lifecycle commands（`install_commands`/`upgrade_commands`/`uninstall_commands`），标记 `installed`。
- 启动运行时前检查 `install_status != "pending"`，阻止未 Materialize 的插件启动。
- 系统启动时自动检测 pending 插件数量并在终端告警。
- 公共 UI 挂载使用“文件优先，缺失回退 `index.html`”的 SPA 入口策略。
- HTTP 代理直接复用 runtime handle 的 `route_base_url`，并把用户上下文转发成 `X-Plugin-*` 头。
- WebSocket 代理维持双向 message 桥接，target 由 `route_base_url + /ws/*` 推导。
- SQLite broker 不把逻辑路径直接映射到任意 VFS 后端，而是固定落到宿主本地受控目录后返回 DSN。
- shared record broker 和 migration broker 都采用宿主统一表 + `plugin_id` 命名空间隔离，而不是让插件直接碰主库内部表。
- task 治理先走“注册表 + scheduler 第一版接管 + HTTP task hook 触发”，没有直接执行任意插件命令。
- nav 协议先存 `label/route/icon/visibility/sort_order/group_key/position/required_permission`，再由主前端消费渲染。

## 安装状态常量

定义在 `installer.rs`：

| 常量 | 值 | 说明 |
|------|-----|------|
| `INSTALL_STATUS_PENDING` | `"pending"` | ZIP 已注册，未 Materialize |
| `INSTALL_STATUS_INSTALLED` | `"installed"` | 安装完成或运行时已停止 |
| `INSTALL_STATUS_RUNNING` | `"running"` | 运行时已启动 |
| `INSTALL_STATUS_UNINSTALLED` | `"uninstalled"` | 预留（当前卸载走硬删除） |

## 当前支持边界

- 安装：zip（`.zip.fupkg`）/ 市场下载地址（统一两阶段 ZIP）
- 运行时：`process` / `docker` / `wasm-module` / `wasm-component` 第一版外部 runner
- 运行时：`process` / `docker` / `wasm-module` / `wasm-component` 第一版外部 runner
- 挂载：UI / HTTP / WebSocket
- broker：KV namespace / shared record / migration state / SQLite / task / nav
- 管理：权限授予 / 启停 / 卸载 / 运行态查询 / 市场目录查询
