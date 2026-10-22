//! 安装参数可以离线断言，命令执行及安装记录仍由父模块负责。

use std::path::Path;
use std::process::Command;

use super::KnownPlugin;

pub(super) fn build_install_command(
    plugin: KnownPlugin,
    root: &Path,
    path: Option<&Path>,
    force: bool,
    version: Option<&str>,
    locked: bool,
) -> Command {
    let mut command = Command::new("cargo");
    command.arg("install");
    if force {
        command.arg("--force");
    }
    if locked {
        command.arg("--locked");
    }
    command.arg("--root").arg(root);
    if let Some(path) = path {
        command.arg("--path").arg(path);
    } else {
        command.arg(plugin.crate_name);
        if let Some(version) = version {
            command.arg("--version").arg(version);
        }
    }
    command
}

#[cfg(test)]
mod tests {
    use super::build_install_command;
    use std::ffi::{OsStr, OsString};
    use std::path::Path;

    fn expected(values: &[&str]) -> Vec<OsString> {
        values.iter().map(|value| OsString::from(*value)).collect()
    }

    fn command_args(
        path: Option<&Path>,
        force: bool,
        version: Option<&str>,
        locked: bool,
    ) -> Vec<OsString> {
        let plugin = super::super::resolve_known_plugin("sse").expect("known plugin");
        let command = build_install_command(
            plugin,
            Path::new("portable root/插件/sse"),
            path,
            force,
            version,
            locked,
        );
        assert_eq!(command.get_program(), OsStr::new("cargo"));
        command.get_args().map(OsString::from).collect()
    }

    #[test]
    fn registry_install_uses_the_selected_root_and_locked_dependencies() {
        assert_eq!(
            command_args(None, false, None, true),
            expected(&[
                "install",
                "--locked",
                "--root",
                "portable root/插件/sse",
                "netutils-plugin-sse",
            ])
        );
    }

    #[test]
    fn force_and_registry_version_keep_their_original_arguments() {
        assert_eq!(
            command_args(None, true, Some("0.2.3"), true),
            expected(&[
                "install",
                "--force",
                "--locked",
                "--root",
                "portable root/插件/sse",
                "netutils-plugin-sse",
                "--version",
                "0.2.3",
            ])
        );
    }

    #[test]
    fn local_path_is_one_argument_and_does_not_receive_a_registry_version() {
        assert_eq!(
            command_args(
                Some(Path::new("local source/插件")),
                false,
                Some("0.2.3"),
                true
            ),
            expected(&[
                "install",
                "--locked",
                "--root",
                "portable root/插件/sse",
                "--path",
                "local source/插件",
            ])
        );
    }

    #[test]
    fn unlocked_install_only_omits_the_locked_switch() {
        assert_eq!(
            command_args(None, false, None, false),
            expected(&[
                "install",
                "--root",
                "portable root/插件/sse",
                "netutils-plugin-sse",
            ])
        );
    }
}
