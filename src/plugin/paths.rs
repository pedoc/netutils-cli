//! 插件根目录政策与无全局环境修改的选择测试。

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// 安装前后及列举时会多次取根，包目录中途变化不能让二进制和锁落在不同位置。
static PLUGIN_BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub(super) fn plugin_base_dir() -> PathBuf {
    PLUGIN_BASE_DIR
        .get_or_init(|| {
            let explicit = env::var_os("NETUTILS_PLUGIN_DIR");
            let executable = if explicit.is_none() {
                env::current_exe().ok()
            } else {
                None
            };
            let user_profile = env::var_os("USERPROFILE");
            let home = env::var_os("HOME");
            select_plugin_base_dir(
                explicit.as_deref(),
                executable.as_deref(),
                user_profile.as_deref(),
                home.as_deref(),
                Path::is_dir,
            )
        })
        .clone()
}

fn select_plugin_base_dir(
    explicit_dir: Option<&OsStr>,
    executable: Option<&Path>,
    user_profile: Option<&OsStr>,
    home: Option<&OsStr>,
    is_dir: impl FnOnce(&Path) -> bool,
) -> PathBuf {
    if let Some(directory) = explicit_dir {
        return PathBuf::from(directory);
    }
    if let Some(directory) = executable
        .and_then(Path::parent)
        .map(|parent| parent.join("plugins"))
    {
        if is_dir(&directory) {
            return directory;
        }
    }
    user_profile
        .or(home)
        .map(|home| Path::new(home).join(".netutils").join("plugins"))
        .unwrap_or_else(|| Path::new(".netutils").join("plugins"))
}

#[cfg(test)]
mod tests {
    use super::select_plugin_base_dir;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    #[test]
    fn explicit_directory_wins_without_probing_the_bundle() {
        let root = select_plugin_base_dir(
            Some(OsStr::new("explicit plugins")),
            Some(Path::new("bundle/netutils")),
            Some(OsStr::new("profile")),
            Some(OsStr::new("home")),
            |_| panic!("explicit configuration must not probe another directory"),
        );
        assert_eq!(root, PathBuf::from("explicit plugins"));
    }

    #[test]
    fn explicit_empty_directory_keeps_its_existing_meaning() {
        let root = select_plugin_base_dir(
            Some(OsStr::new("")),
            Some(Path::new("bundle/netutils")),
            Some(OsStr::new("profile")),
            None,
            |_| panic!("an empty explicit value is still configured"),
        );
        assert_eq!(root, PathBuf::new());
    }

    #[test]
    fn explicit_relative_directory_is_not_normalized() {
        let value = OsStr::new("../custom plugins");
        let root = select_plugin_base_dir(Some(value), None, None, None, |_| false);
        assert_eq!(root, PathBuf::from(value));
    }

    #[test]
    fn explicit_missing_directory_is_not_replaced_by_a_bundle() {
        let root = select_plugin_base_dir(
            Some(OsStr::new("not-created-yet")),
            Some(Path::new("bundle/netutils")),
            Some(OsStr::new("profile")),
            None,
            |_| true,
        );
        assert_eq!(root, PathBuf::from("not-created-yet"));
    }

    #[test]
    fn executable_sibling_directory_wins_over_both_user_directories() {
        let expected = Path::new("bundle").join("plugins");
        let root = select_plugin_base_dir(
            None,
            Some(Path::new("bundle/netutils")),
            Some(OsStr::new("profile")),
            Some(OsStr::new("home")),
            |candidate| {
                assert_eq!(candidate, expected);
                true
            },
        );
        assert_eq!(root, expected);
    }

    #[test]
    fn empty_bundle_directory_does_not_need_a_plugin_or_lock() {
        let root = select_plugin_base_dir(
            None,
            Some(Path::new("empty bundle/netutils")),
            None,
            None,
            |_| true,
        );
        assert_eq!(root, Path::new("empty bundle").join("plugins"));
    }

    #[test]
    fn non_directory_bundle_candidate_keeps_the_user_default() {
        let root = select_plugin_base_dir(
            None,
            Some(Path::new("bundle/netutils")),
            Some(OsStr::new("profile")),
            Some(OsStr::new("home")),
            |_| false,
        );
        assert_eq!(root, Path::new("profile").join(".netutils").join("plugins"));
    }

    #[test]
    fn unavailable_executable_location_keeps_the_user_default() {
        let root = select_plugin_base_dir(
            None,
            None,
            Some(OsStr::new("profile")),
            Some(OsStr::new("home")),
            |_| panic!("there is no executable directory to probe"),
        );
        assert_eq!(root, Path::new("profile").join(".netutils").join("plugins"));
    }

    #[test]
    fn executable_without_a_parent_does_not_guess_the_working_directory() {
        let root = select_plugin_base_dir(
            None,
            Some(Path::new("")),
            None,
            Some(OsStr::new("home")),
            |_| panic!("a missing parent must not become a cwd probe"),
        );
        assert_eq!(root, Path::new("home").join(".netutils").join("plugins"));
    }

    #[test]
    fn user_profile_keeps_priority_over_home() {
        let root = select_plugin_base_dir(
            None,
            None,
            Some(OsStr::new("profile")),
            Some(OsStr::new("home")),
            |_| false,
        );
        assert_eq!(root, Path::new("profile").join(".netutils").join("plugins"));
    }

    #[test]
    fn empty_user_profile_is_not_changed_into_a_missing_variable() {
        let root = select_plugin_base_dir(
            None,
            None,
            Some(OsStr::new("")),
            Some(OsStr::new("home")),
            |_| false,
        );
        assert_eq!(root, Path::new(".netutils").join("plugins"));
    }

    #[test]
    fn home_is_used_when_user_profile_is_unset() {
        let root = select_plugin_base_dir(
            None,
            Some(Path::new("bundle/netutils")),
            None,
            Some(OsStr::new("home")),
            |_| false,
        );
        assert_eq!(root, Path::new("home").join(".netutils").join("plugins"));
    }

    #[test]
    fn missing_home_variables_keep_the_relative_legacy_fallback() {
        let root = select_plugin_base_dir(None, None, None, None, |_| false);
        assert_eq!(root, Path::new(".netutils").join("plugins"));
    }

    #[test]
    fn only_the_executable_sibling_is_checked() {
        let mut checked = Vec::new();
        let root = select_plugin_base_dir(
            None,
            Some(Path::new("outer/bundle/netutils")),
            None,
            Some(OsStr::new("home")),
            |candidate| {
                checked.push(candidate.to_path_buf());
                false
            },
        );
        assert_eq!(checked, vec![Path::new("outer/bundle").join("plugins")]);
        assert_eq!(root, Path::new("home").join(".netutils").join("plugins"));
    }

    #[cfg(windows)]
    #[test]
    fn explicit_non_unicode_directory_is_preserved() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let value = OsString::from_wide(&[0x0070, 0xd800]);
        let root = select_plugin_base_dir(Some(&value), None, None, None, |_| false);
        assert_eq!(root.as_os_str(), value.as_os_str());
    }

    #[cfg(unix)]
    #[test]
    fn explicit_non_unicode_directory_is_preserved() {
        use std::os::unix::ffi::OsStrExt;
        let value = OsStr::from_bytes(b"plugins-\xff");
        let root = select_plugin_base_dir(Some(value), None, None, None, |_| false);
        assert_eq!(root.as_os_str(), value);
    }
}
