//! Tests for TOML deserialization → `RuntimeConfig` mapping. The persistence
//! wiring is opt-in and lives entirely in `config.rs` (the runtime tests build
//! `RuntimeConfig` directly, bypassing deserialization), so this covers the
//! config-layer gap.

use lightsandbox_server::config::AppConfig;

const MINIMAL_TOML: &str = r#"
[server]
host = "127.0.0.1"
port = 8080

[runtime]
type = "local"
workspace_root = "./data/workspaces"

[limits]
max_sandboxes = 100
max_concurrent_exec = 20
default_ttl_seconds = 600
default_exec_timeout_seconds = 60
max_stdout_bytes = 1048576
max_stderr_bytes = 1048576
max_file_size_bytes = 10485760
max_read_file_bytes = 10485760

[gc]
enabled = true
interval_seconds = 30
remove_expired = true

[security]
allow_absolute_paths = false
allow_path_traversal = false
hide_host_paths = true
"#;

#[test]
fn persistence_defaults_off_when_section_absent() {
    let config: AppConfig = toml::from_str(MINIMAL_TOML).unwrap();
    assert!(
        config.runtime_config().persistence_db_path.is_none(),
        "persistence must default to off when the section is absent"
    );
}

#[test]
fn persistence_enabled_sets_db_path() {
    let toml_text =
        format!("{MINIMAL_TOML}\n[persistence]\nenabled = true\npath = \"./data/custom.redb\"\n");
    let config: AppConfig = toml::from_str(&toml_text).unwrap();
    let path = config
        .runtime_config()
        .persistence_db_path
        .expect("persistence_db_path should be set when enabled");
    assert_eq!(path, std::path::PathBuf::from("./data/custom.redb"));
}

#[test]
fn persistence_section_present_but_disabled_is_none() {
    let toml_text = format!("{MINIMAL_TOML}\n[persistence]\nenabled = false\n");
    let config: AppConfig = toml::from_str(&toml_text).unwrap();
    assert!(config.runtime_config().persistence_db_path.is_none());
}
