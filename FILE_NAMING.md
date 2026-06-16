# Plugin System File Naming Conventions

This document defines the file naming standards for the FileUni Plugin System v2.

## Core Constants

The plugin system uses the following file naming constants (defined in `src/installer.rs`):

| Constant | Value | Purpose |
|----------|-------|---------|
| `PLUGIN_MANIFEST_FILE_NAME` | `plugin.json` | Plugin manifest file, must exist at the root of plugin ZIP packages |
| `PLUGIN_DEFAULT_CONFIG_FILE_NAME` | `config.toml` | Default plugin configuration file name |

## File Structure

### Plugin Package (ZIP)

```
plugin-package.zip
├── plugin.json              # REQUIRED: Plugin manifest (PLUGIN_MANIFEST_FILE_NAME)
├── runtime/                 # Binaries / wasm / scripts
├── ui/                      # Frontend static resources
├── migrations/              # SQL migration files
└── data/                    # Default data files
```

### Runtime Directory Layout

```
{root_dir}/
├── packages/{plugin_id}/{version}/
│   └── plugin.json          # Installed manifest
├── config/{plugin_id}/
│   └── config.toml          # Default config (PLUGIN_DEFAULT_CONFIG_FILE_NAME)
├── state/{plugin_id}/
├── logs/{plugin_id}/
├── runtime/{plugin_id}/
└── shared/
```

## Configuration File Rules

1. **Default Configuration**
   - File name: `config.toml` (constant `PLUGIN_DEFAULT_CONFIG_FILE_NAME`)
   - Location: `{root_dir}/config/{plugin_id}/config.toml`
   - Created automatically by `ensure_plugin_config_paths()` if not exists
   - Exposed via `FILEUNI_PLUGIN_CONFIG_FILE` environment variable

2. **Custom Configuration Files**
   - Plugins can create additional config files via host API `POST /config/ensure`
   - Custom files must be in the same config directory: `{root_dir}/config/{plugin_id}/`
   - Plugins are responsible for managing custom file lifecycle
   - File names are sanitized (non-alphanumeric characters become `_`)

3. **Best Practices**
   - Always read config path from `FILEUNI_PLUGIN_CONFIG_FILE` environment variable
   - Never hardcode `config.toml` or any config file name in plugin code
   - Use `FILEUNI_PLUGIN_CONFIG_DIR` to construct paths for custom config files

## Manifest File Rules

1. **Location**
   - Must be at the root of plugin ZIP package
   - Exactly named `plugin.json` (constant `PLUGIN_MANIFEST_FILE_NAME`)

2. **Validation**
   - System rejects ZIP packages without `plugin.json` at root
   - Must be valid UTF-8 JSON
   - Must conform to `PluginManifest` schema

3. **References in Code**
   - Always use `PLUGIN_MANIFEST_FILE_NAME` constant
   - Never hardcode `"plugin.json"` string

## Environment Variables

The plugin runtime injects these environment variables:

- `FILEUNI_PLUGIN_ID`: Plugin identifier
- `FILEUNI_PLUGIN_HOST_API_BASE_URL`: Host API base URL
- `FILEUNI_PLUGIN_HOST_API_TOKEN`: Authentication token
- `FILEUNI_PLUGIN_CONFIG_DIR`: Plugin config directory path
- `FILEUNI_PLUGIN_CONFIG_FILE`: Full path to default config file

## Migration from Hardcoded Strings

If you are migrating existing plugin code:

### Before
```rust
let manifest = read_manifest("plugin.json")?;
let config_path = config_dir.join("config.toml");
```

### After
```rust
use yh_extension_manager_v2::{PLUGIN_MANIFEST_FILE_NAME, PLUGIN_DEFAULT_CONFIG_FILE_NAME};

let manifest = read_manifest(PLUGIN_MANIFEST_FILE_NAME)?;
let config_path = config_dir.join(PLUGIN_DEFAULT_CONFIG_FILE_NAME);
```

### In Plugin Code (Recommended)
```rust
// Read from environment variable instead of hardcoding
let config_file = std::env::var("FILEUNI_PLUGIN_CONFIG_FILE")
    .expect("FILEUNI_PLUGIN_CONFIG_FILE not set");
let config = read_config(&config_file)?;
```

## Testing

Unit tests are provided in `tests/file_naming_test.rs` to verify:
- Constant values are correct
- File extensions match expectations
- Constants are properly exported

Run tests with:
```bash
cargo test -p yh-extension-manager-v2 file_naming
```

## See Also

- `docs/插件系统v2.md` - Complete plugin system documentation
- `src/installer.rs` - Constant definitions
- `src/manager.rs` - Config path management
- `src/host_api.rs` - Host API config endpoints
