use yh_extension_manager_v2::{PLUGIN_DEFAULT_CONFIG_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME};

#[test]
fn test_plugin_file_naming_constants() {
    assert_eq!(PLUGIN_MANIFEST_FILE_NAME, "plugin.json");
    assert_eq!(PLUGIN_DEFAULT_CONFIG_FILE_NAME, "config.toml");
}

#[test]
fn test_manifest_file_name_is_json() {
    assert!(PLUGIN_MANIFEST_FILE_NAME.ends_with(".json"));
}

#[test]
fn test_config_file_name_is_toml() {
    assert!(PLUGIN_DEFAULT_CONFIG_FILE_NAME.ends_with(".toml"));
}
