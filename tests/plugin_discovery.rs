//! 发行包插件发现的离线进程回归：路径和环境均隔离，不运行 cargo 或网络插件。

#[path = "common/plugin_fixture.rs"]
mod plugin_fixture;

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use plugin_fixture::{
    add_binary, add_plugin, assert_plugin_missing, assert_plugin_output, copy_fake_executable,
    executable_name, reported_root, run, run_json, run_plugin, set_path, snapshot,
    write_legacy_lock, PluginFixture,
};

fn listed<'a>(plugins: &'a Value, name: &str) -> &'a Value {
    plugins
        .as_array()
        .expect("plugin list must remain an array")
        .iter()
        .find(|plugin| plugin["name"] == name)
        .unwrap_or_else(|| panic!("plugin list did not include {name}: {plugins}"))
}

fn assert_listed_binary(plugin: &Value, binary: &Path) {
    assert_eq!(plugin["installed"], true);
    let actual = plugin["path"]
        .as_str()
        .expect("installed path must be a string");
    assert_eq!(
        fs::canonicalize(actual).expect("canonical listed binary"),
        fs::canonicalize(binary).expect("canonical expected binary")
    );
}

fn assert_entry_schema(plugin: &Value) {
    for field in ["name", "binary", "crate_name", "status", "integrity"] {
        assert!(plugin[field].is_string(), "{field} changed type: {plugin}");
    }
    for field in ["supported", "installed"] {
        assert!(plugin[field].is_boolean(), "{field} changed type: {plugin}");
    }
    for field in ["version", "source", "path"] {
        let value = plugin
            .get(field)
            .unwrap_or_else(|| panic!("missing {field} field: {plugin}"));
        assert!(
            value.is_null() || value.is_string(),
            "{field} changed type: {plugin}"
        );
    }
    assert!(
        plugin["platforms"]
            .as_array()
            .expect("platforms must remain an array")
            .iter()
            .all(Value::is_string),
        "platforms changed type: {plugin}"
    );
}

#[test]
fn discovers_adjacent_bundle_without_configuration() {
    let fixture = PluginFixture::new("zero-config");
    let root = fixture.bundled_root();
    let binary = add_plugin(&root, "mcp");

    assert_eq!(reported_root(&mut fixture.command()), root);
    let plugins = run_json(&mut fixture.command(), &["--json", "plugin", "list"]);
    let mcp = listed(&plugins, "mcp");
    assert_listed_binary(mcp, &binary);
    assert_eq!(mcp["status"], "untracked");
    assert_eq!(mcp["integrity"], "unrecorded");
    assert_plugin_output(&run_plugin(&mut fixture.command(), "mcp"), &binary, "mcp");
    fixture.finish();
}

#[test]
fn explicit_directory_wins_all_other_plugin_sources() {
    let fixture = PluginFixture::new("explicit");
    let explicit = fixture.workspace().join("explicit plugins");
    let binary = add_plugin(&explicit, "mcp");
    add_plugin(&fixture.bundled_root(), "mcp");
    add_plugin(&fixture.user_root(), "mcp");
    let path_directory = fixture.workspace().join("plugin-path");
    copy_fake_executable(&path_directory.join(executable_name("netutils-mcp")));
    let configured = || {
        let mut command = fixture.command();
        command.env("NETUTILS_PLUGIN_DIR", &explicit);
        set_path(&mut command, &path_directory);
        command
    };

    assert_eq!(reported_root(&mut configured()), explicit);
    let plugins = run_json(&mut configured(), &["--json", "plugin", "list"]);
    assert_listed_binary(listed(&plugins, "mcp"), &binary);
    assert_plugin_output(&run_plugin(&mut configured(), "mcp"), &binary, "mcp");
    fixture.finish();
}

#[test]
fn invalid_explicit_directory_never_falls_back_to_bundle_or_home() {
    let fixture = PluginFixture::new("invalid-explicit");
    add_plugin(&fixture.bundled_root(), "mcp");
    add_plugin(&fixture.user_root(), "mcp");
    let missing = fixture.workspace().join("missing-explicit");
    let file = fixture.workspace().join("explicit-is-a-file");
    fs::write(&file, b"not a directory").expect("create invalid explicit root");
    let path_directory = fixture.workspace().join("plugin-path");
    let path_binary = copy_fake_executable(&path_directory.join(executable_name("netutils-mcp")));

    for explicit in [&missing, &file] {
        let configured = || {
            let mut command = fixture.command();
            command.env("NETUTILS_PLUGIN_DIR", explicit);
            command
        };
        assert_eq!(reported_root(&mut configured()), *explicit);
        let plugins = run_json(&mut configured(), &["--json", "plugin", "list"]);
        assert_eq!(listed(&plugins, "mcp")["installed"], false);
        assert_plugin_missing(&run_plugin(&mut configured(), "mcp"));

        let mut with_path = configured();
        set_path(&mut with_path, &path_directory);
        assert_plugin_output(&run_plugin(&mut with_path, "mcp"), &path_binary, "mcp");
    }
    fixture.finish();
}

#[test]
fn bundle_takes_precedence_over_home_and_path() {
    let fixture = PluginFixture::new("bundle-priority");
    let binary = add_plugin(&fixture.bundled_root(), "mcp");
    add_plugin(&fixture.user_root(), "mcp");
    let path_directory = fixture.workspace().join("plugin-path");
    copy_fake_executable(&path_directory.join(executable_name("netutils-mcp")));
    let configured = || {
        let mut command = fixture.command();
        set_path(&mut command, &path_directory);
        command
    };

    assert_eq!(reported_root(&mut configured()), fixture.bundled_root());
    let plugins = run_json(&mut configured(), &["--json", "plugin", "list"]);
    assert_listed_binary(listed(&plugins, "mcp"), &binary);
    assert_plugin_output(&run_plugin(&mut configured(), "mcp"), &binary, "mcp");
    fixture.finish();
}

#[test]
fn bundle_discovery_does_not_depend_on_working_directory() {
    let fixture = PluginFixture::new("cwd-independent");
    let binary = add_plugin(&fixture.bundled_root(), "mcp");
    add_plugin(&fixture.cwd.join("plugins"), "mcp");
    add_plugin(
        &fixture
            .cwd
            .parent()
            .expect("working directory parent")
            .join("plugins"),
        "mcp",
    );
    let other_cwd = fixture.workspace().join("another-work");
    add_plugin(&other_cwd.join("plugins"), "mcp");

    for cwd in [&fixture.cwd, &other_cwd] {
        assert_eq!(
            reported_root(fixture.command().current_dir(cwd)),
            fixture.bundled_root()
        );
        assert_plugin_output(
            &run_plugin(fixture.command().current_dir(cwd), "mcp"),
            &binary,
            "mcp",
        );
    }
    fixture.finish();
}

#[test]
fn absent_bundle_never_searches_working_directory_or_ancestors() {
    let fixture = PluginFixture::new("cwd-no-bundle");
    let binary = add_plugin(&fixture.user_root(), "mcp");
    add_plugin(&fixture.cwd.join("plugins"), "mcp");
    add_plugin(
        &fixture
            .cwd
            .parent()
            .expect("working directory parent")
            .join("plugins"),
        "mcp",
    );

    assert!(!fixture.bundled_root().exists());
    assert_eq!(reported_root(&mut fixture.command()), fixture.user_root());
    assert_plugin_output(&run_plugin(&mut fixture.command(), "mcp"), &binary, "mcp");
    fixture.finish();
}

#[test]
fn bundle_is_found_when_core_is_started_from_path() {
    let fixture = PluginFixture::new("core-from-path");
    let binary = add_plugin(&fixture.bundled_root(), "mcp");

    assert_eq!(
        reported_root(&mut fixture.command_from_path()),
        fixture.bundled_root()
    );
    assert_plugin_output(
        &run_plugin(&mut fixture.command_from_path(), "mcp"),
        &binary,
        "mcp",
    );
    fixture.finish();
}

#[test]
fn moving_complete_bundle_preserves_zero_configuration_discovery() {
    let fixture = PluginFixture::new("relocated");
    let original_binary = add_plugin(&fixture.bundled_root(), "mcp");
    let relocated = fixture.workspace().join("moved bundle 测试");
    let binary = relocated.join(
        original_binary
            .strip_prefix(&fixture.bundle)
            .expect("plugin is inside bundle"),
    );
    fs::rename(&fixture.bundle, &relocated).expect("move the complete unused bundle");

    assert!(!fixture.bundle.exists());
    assert_eq!(
        reported_root(&mut fixture.command_at(&relocated)),
        relocated.join("plugins")
    );
    let plugins = run_json(
        &mut fixture.command_at(&relocated),
        &["--json", "plugin", "list"],
    );
    assert_listed_binary(listed(&plugins, "mcp"), &binary);
    assert_plugin_output(
        &run_plugin(&mut fixture.command_at(&relocated), "mcp"),
        &binary,
        "mcp",
    );
    fixture.finish();
}

#[test]
fn absent_or_file_bundle_preserves_the_user_directory() {
    let fixture = PluginFixture::new("user-fallback");
    let binary = add_plugin(&fixture.user_root(), "mcp");
    write_legacy_lock(&fixture.user_root(), "mcp", "1.2.3");

    for file_instead_of_directory in [false, true] {
        if file_instead_of_directory {
            fs::write(fixture.bundled_root(), b"not a directory")
                .expect("create a file named plugins");
        }
        assert_eq!(reported_root(&mut fixture.command()), fixture.user_root());
        let plugins = run_json(&mut fixture.command(), &["--json", "plugin", "list"]);
        assert_listed_binary(listed(&plugins, "mcp"), &binary);
        assert_eq!(listed(&plugins, "mcp")["version"], "1.2.3");
        assert_plugin_output(&run_plugin(&mut fixture.command(), "mcp"), &binary, "mcp");
    }
    fixture.finish();
}

#[test]
fn empty_bundle_hides_home_plugins_but_keeps_path_fallback() {
    let fixture = PluginFixture::new("empty-bundle");
    fs::create_dir(fixture.bundled_root()).expect("create an empty bundle plugin root");
    add_plugin(&fixture.user_root(), "mcp");
    write_legacy_lock(&fixture.user_root(), "mcp", "9.9.9");

    assert_eq!(
        reported_root(&mut fixture.command()),
        fixture.bundled_root()
    );
    let plugins = run_json(&mut fixture.command(), &["--json", "plugin", "list"]);
    let mcp = listed(&plugins, "mcp");
    assert_eq!(mcp["installed"], false);
    assert_eq!(mcp["status"], "not-installed");
    assert_eq!(mcp["integrity"], "--");
    assert!(mcp["version"].is_null());
    assert_plugin_missing(&run_plugin(&mut fixture.command(), "mcp"));

    let path_directory = fixture.workspace().join("plugin-path");
    let binary = copy_fake_executable(&path_directory.join(executable_name("netutils-mcp")));
    let mut from_path = fixture.command();
    set_path(&mut from_path, &path_directory);
    assert_plugin_output(&run_plugin(&mut from_path, "mcp"), &binary, "mcp");
    let mut listing = fixture.command();
    set_path(&mut listing, &path_directory);
    let plugins = run_json(&mut listing, &["--json", "plugin", "list"]);
    assert_eq!(listed(&plugins, "mcp")["installed"], false);
    fixture.finish();
}

#[test]
fn nonempty_bundle_does_not_merge_a_missing_home_plugin() {
    let fixture = PluginFixture::new("no-home-overlay");
    let sse = add_plugin(&fixture.bundled_root(), "sse");
    add_plugin(&fixture.user_root(), "mcp");
    write_legacy_lock(&fixture.user_root(), "mcp", "9.9.9");

    assert_eq!(
        reported_root(&mut fixture.command()),
        fixture.bundled_root()
    );
    let plugins = run_json(&mut fixture.command(), &["--json", "plugin", "list"]);
    assert_listed_binary(listed(&plugins, "sse"), &sse);
    let mcp = listed(&plugins, "mcp");
    assert_eq!(mcp["installed"], false);
    assert!(mcp["path"].is_null());
    assert!(mcp["version"].is_null());
    assert_plugin_missing(&run_plugin(&mut fixture.command(), "mcp"));
    fixture.finish();
}

#[test]
fn aliases_execute_the_correct_bundled_binaries_and_propagate_exit_codes() {
    let fixture = PluginFixture::new("aliases");
    let sse = add_plugin(&fixture.bundled_root(), "sse");
    let ws = add_plugin(&fixture.bundled_root(), "ws");
    let websocket = add_binary(&fixture.bundled_root(), "ws", "netutils-websocket");
    add_plugin(&fixture.user_root(), "sse");
    add_plugin(&fixture.user_root(), "ws");
    add_binary(&fixture.user_root(), "ws", "netutils-websocket");

    for (command, binary) in [
        ("event", &sse),
        ("sse", &sse),
        ("ws", &ws),
        ("websocket", &websocket),
    ] {
        assert_plugin_output(
            &run_plugin(&mut fixture.command(), command),
            binary,
            command,
        );
    }
    let plugins = run_json(&mut fixture.command(), &["--json", "plugin", "list"]);
    assert_listed_binary(listed(&plugins, "sse"), &sse);
    assert_listed_binary(listed(&plugins, "ws"), &ws);
    fixture.finish();
}

#[test]
fn json_reads_selected_legacy_metadata_without_writing_any_plugin_tree() {
    let fixture = PluginFixture::new("metadata-read-only");
    let root = fixture.bundled_root();
    let mcp_binary = add_plugin(&root, "mcp");
    let sse_binary = add_plugin(&root, "sse");
    write_legacy_lock(&root, "mcp", "1.2.3");
    add_plugin(&fixture.user_root(), "mcp");
    write_legacy_lock(&fixture.user_root(), "mcp", "9.9.9");
    let bundle_before = snapshot(&root);
    let home_before = snapshot(&fixture.home);

    assert_eq!(reported_root(&mut fixture.command()), root);
    let plugins = run_json(&mut fixture.command(), &["--json", "plugin", "list"]);
    for plugin in plugins.as_array().expect("plugin list array") {
        assert_entry_schema(plugin);
    }
    let mcp = listed(&plugins, "mcp");
    assert_listed_binary(mcp, &mcp_binary);
    assert_eq!(mcp["status"], "ok");
    assert_eq!(mcp["version"], "1.2.3");
    assert_eq!(mcp["source"], "registry:netutils-plugin-mcp");
    assert_eq!(mcp["integrity"], "unrecorded");
    let sse = listed(&plugins, "sse");
    assert_listed_binary(sse, &sse_binary);
    assert_eq!(sse["status"], "untracked");
    assert_eq!(sse["integrity"], "unrecorded");
    assert!(sse["version"].is_null());
    assert!(sse["source"].is_null());
    let ws = listed(&plugins, "ws");
    assert_eq!(ws["installed"], false);
    assert_eq!(ws["integrity"], "--");
    assert!(ws["path"].is_null());
    assert_plugin_output(
        &run_plugin(&mut fixture.command(), "mcp"),
        &mcp_binary,
        "mcp",
    );
    assert_plugin_output(
        &run_plugin(&mut fixture.command(), "event"),
        &sse_binary,
        "event",
    );
    assert_eq!(snapshot(&root), bundle_before);
    assert_eq!(snapshot(&fixture.home), home_before);
    fixture.finish();
}

#[test]
fn inspection_and_missing_execution_do_not_create_default_directories() {
    let fixture = PluginFixture::new("no-directory-writes");
    let bundle_before = snapshot(&fixture.bundled_root());
    let home_before = snapshot(&fixture.home);

    assert_eq!(reported_root(&mut fixture.command()), fixture.user_root());
    let plugins = run_json(&mut fixture.command(), &["--json", "plugin", "list"]);
    for plugin in plugins.as_array().expect("plugin list array") {
        assert_eq!(plugin["installed"], false);
        assert_eq!(plugin["integrity"], "--");
    }
    assert_plugin_missing(&run_plugin(&mut fixture.command(), "mcp"));
    assert!(!fixture.user_root().exists());
    assert_eq!(snapshot(&fixture.bundled_root()), bundle_before);
    assert_eq!(snapshot(&fixture.home), home_before);
    fixture.finish();
}

#[test]
fn remove_modifies_only_the_selected_bundle_plugin_and_lock() {
    let fixture = PluginFixture::new("remove-bundle");
    let root = fixture.bundled_root();
    add_plugin(&root, "mcp");
    write_legacy_lock(&root, "mcp", "1.2.3");
    add_plugin(&root, "sse");
    add_plugin(&fixture.user_root(), "mcp");
    write_legacy_lock(&fixture.user_root(), "mcp", "9.9.9");
    let home_before = snapshot(&fixture.home);
    let other_plugin_before = snapshot(&root.join("sse"));

    let removed = run_json(
        &mut fixture.command(),
        &["--json", "plugin", "remove", "mcp"],
    );
    assert_eq!(removed["removed"], true);
    assert_eq!(removed["name"], "mcp");
    assert_eq!(
        PathBuf::from(removed["path"].as_str().expect("removed path string")),
        root.join("mcp")
    );
    assert!(!root.join("mcp").exists());
    assert_eq!(snapshot(&fixture.home), home_before);
    assert_eq!(snapshot(&root.join("sse")), other_plugin_before);
    let plugins = run_json(&mut fixture.command(), &["--json", "plugin", "list"]);
    assert_eq!(listed(&plugins, "mcp")["installed"], false);
    assert!(listed(&plugins, "mcp")["version"].is_null());
    assert_plugin_missing(&run_plugin(&mut fixture.command(), "mcp"));
    fixture.finish();
}

#[test]
fn remove_respects_explicit_root_without_touching_bundle_or_home() {
    let fixture = PluginFixture::new("remove-explicit");
    let explicit = fixture.workspace().join("explicit plugins");
    add_plugin(&explicit, "mcp");
    write_legacy_lock(&explicit, "mcp", "1.2.3");
    add_plugin(&fixture.bundled_root(), "mcp");
    write_legacy_lock(&fixture.bundled_root(), "mcp", "2.3.4");
    add_plugin(&fixture.user_root(), "mcp");
    write_legacy_lock(&fixture.user_root(), "mcp", "9.9.9");
    let bundle_before = snapshot(&fixture.bundled_root());
    let home_before = snapshot(&fixture.home);

    let removed = run_json(
        fixture.command().env("NETUTILS_PLUGIN_DIR", &explicit),
        &["--json", "plugin", "remove", "mcp"],
    );
    assert_eq!(removed["removed"], true);
    assert_eq!(
        PathBuf::from(removed["path"].as_str().expect("removed path string")),
        explicit.join("mcp")
    );
    assert!(!explicit.join("mcp").exists());
    assert_eq!(snapshot(&fixture.bundled_root()), bundle_before);
    assert_eq!(snapshot(&fixture.home), home_before);
    fixture.finish();
}

#[test]
fn remove_never_falls_back_to_a_home_copy_missing_from_bundle() {
    let fixture = PluginFixture::new("remove-no-overlay");
    fs::create_dir(fixture.bundled_root()).expect("create empty bundle root");
    add_plugin(&fixture.user_root(), "mcp");
    write_legacy_lock(&fixture.user_root(), "mcp", "9.9.9");
    let bundle_before = snapshot(&fixture.bundled_root());
    let home_before = snapshot(&fixture.home);

    let output = run(
        &mut fixture.command(),
        &["--json", "plugin", "remove", "mcp"],
    );
    assert_eq!(output.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&output.stdout).expect("JSON remove error");
    assert!(error["error"]
        .as_str()
        .expect("error must remain a string")
        .contains("not installed"));
    assert_eq!(snapshot(&fixture.bundled_root()), bundle_before);
    assert_eq!(snapshot(&fixture.home), home_before);
    fixture.finish();
}
