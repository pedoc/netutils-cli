use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use serde_json::Value;

pub(super) const FIXTURE_FILTER: &str = "plugin_fixture::fake_plugin_process";
pub(super) const CHILD_MARKER_ENV: &str = "NETUTILS_PLUGIN_TEST_CHILD";
pub(super) const CHILD_MARKER_VALUE: &str = "bundled-discovery-native-fixture";
pub(super) const PLUGIN_EXIT_CODE: i32 = 37;
const OUTPUT_PREFIX: &str = "NETUTILS_PLUGIN_FIXTURE=";
const DIRECTORY_ATTEMPTS: usize = 100;
const CLEANUP_ATTEMPTS: usize = 8;
const CLEANUP_DELAY: Duration = Duration::from_millis(25);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new(label: &str) -> Self {
        for _ in 0..DIRECTORY_ATTEMPTS {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "netutils-plugin-{label}-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("failed to create {}: {error}", path.display()),
            }
        }
        panic!("failed to allocate a unique directory for {label}");
    }

    fn remove(&self) -> io::Result<()> {
        let attempts = if cfg!(windows) { CLEANUP_ATTEMPTS } else { 1 };
        for attempt in 0..attempts {
            match fs::remove_dir_all(&self.path) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(_) if attempt + 1 < attempts => thread::sleep(CLEANUP_DELAY),
                Err(error) => return Err(error),
            }
        }
        unreachable!("cleanup always returns on its final attempt")
    }

    fn close(self) {
        self.remove()
            .unwrap_or_else(|error| panic!("failed to clean up {}: {error}", self.path.display()));
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        if let Err(error) = self.remove() {
            eprintln!("failed to clean up {}: {error}", self.path.display());
        }
    }
}

pub(super) struct PluginFixture {
    temporary: TemporaryDirectory,
    workspace: PathBuf,
    pub(super) bundle: PathBuf,
    pub(super) home: PathBuf,
    pub(super) cwd: PathBuf,
    empty_path: PathBuf,
}

impl PluginFixture {
    pub(super) fn new(label: &str) -> Self {
        let temporary = TemporaryDirectory::new(label);
        let workspace = fs::canonicalize(&temporary.path).expect("canonical fixture directory");
        let bundle = workspace.join("portable bundle 测试");
        let home = workspace.join("isolated-home");
        let cwd = workspace.join("unrelated-work").join("nested");
        let empty_path = workspace.join("empty-path");
        for directory in [&bundle, &home, &cwd, &empty_path] {
            fs::create_dir_all(directory).expect("create fixture directory");
        }
        fs::copy(
            env!("CARGO_BIN_EXE_netutils"),
            bundle.join(executable_name("netutils")),
        )
        .expect("copy netutils into the portable bundle");
        Self {
            temporary,
            workspace,
            bundle,
            home,
            cwd,
            empty_path,
        }
    }

    pub(super) fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(super) fn bundled_root(&self) -> PathBuf {
        self.bundle.join("plugins")
    }

    pub(super) fn user_root(&self) -> PathBuf {
        self.home.join(".netutils").join("plugins")
    }

    pub(super) fn command(&self) -> Command {
        self.command_at(&self.bundle)
    }

    pub(super) fn command_at(&self, bundle: &Path) -> Command {
        self.isolated_command(bundle.join(executable_name("netutils")))
    }

    pub(super) fn command_from_path(&self) -> Command {
        let mut command = self.isolated_command(executable_name("netutils"));
        set_path(&mut command, &self.bundle);
        command
    }

    fn isolated_command(&self, executable: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(executable);
        command
            .current_dir(&self.cwd)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env_remove("NETUTILS_PLUGIN_DIR")
            .env_remove(CHILD_MARKER_ENV)
            .env("NETUTILS_LANG", "en")
            .env("NO_COLOR", "1");
        set_path(&mut command, &self.empty_path);
        command
    }

    pub(super) fn finish(self) {
        self.temporary.close();
    }
}

pub(super) fn executable_name(binary: &str) -> String {
    format!("{binary}{}", env::consts::EXE_SUFFIX)
}

pub(super) fn add_plugin(root: &Path, name: &str) -> PathBuf {
    add_binary(root, name, &format!("netutils-{name}"))
}

pub(super) fn add_binary(root: &Path, plugin: &str, binary: &str) -> PathBuf {
    copy_fake_executable(&root.join(plugin).join("bin").join(executable_name(binary)))
}

pub(super) fn copy_fake_executable(destination: &Path) -> PathBuf {
    fs::create_dir_all(destination.parent().expect("fake plugin parent"))
        .expect("create fake plugin directory");
    fs::copy(
        env::current_exe().expect("current integration test executable"),
        destination,
    )
    .unwrap_or_else(|error| panic!("failed to copy {}: {error}", destination.display()));
    destination.to_path_buf()
}

pub(super) fn write_legacy_lock(root: &Path, name: &str, version: &str) -> PathBuf {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("create lock directory");
    let path = directory.join("plugin-lock.json");
    let lock = serde_json::json!({
        "name": name,
        "binary": format!("netutils-{name}"),
        "crate_name": format!("netutils-plugin-{name}"),
        "version": version,
        "source": "registry",
        "source_value": format!("netutils-plugin-{name}"),
        "installed_at_unix": 1,
        "binary_path": directory.join("bin").join(executable_name(&format!("netutils-{name}"))),
        "core_version": "0.3.22"
    });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&lock).expect("serialize old lock"),
    )
    .expect("write old lock without a hash field");
    path
}

pub(super) fn set_path(command: &mut Command, directory: &Path) {
    command.env(
        "PATH",
        env::join_paths([directory]).expect("encode the isolated PATH"),
    );
}

pub(super) fn run(command: &mut Command, args: &[&str]) -> Output {
    command.args(args);
    command
        .output()
        .unwrap_or_else(|error| panic!("failed to run {command:?}: {error}"))
}

pub(super) fn run_json(command: &mut Command, args: &[&str]) -> Value {
    let output = run(command, args);
    assert!(
        output.status.success(),
        "{args:?} exited {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}\nstdout: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

pub(super) fn reported_root(command: &mut Command) -> PathBuf {
    let value = run_json(command, &["--json", "plugin", "dir"]);
    let object = value.as_object().expect("plugin dir must remain an object");
    assert_eq!(object.len(), 1, "plugin dir JSON shape changed: {value}");
    let path = PathBuf::from(value["dir"].as_str().expect("dir must remain a string"));
    // Windows 的主程序路径可能带扩展前缀，比较实际目录而不是两种等价拼写。
    if path.exists() {
        fs::canonicalize(&path).expect("canonical reported plugin root")
    } else {
        path
    }
}

pub(super) fn run_plugin(command: &mut Command, name: &str) -> Output {
    command.env(CHILD_MARKER_ENV, CHILD_MARKER_VALUE);
    run(
        command,
        &[
            name,
            "--exact",
            FIXTURE_FILTER,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ],
    )
}

pub(super) fn assert_plugin_output(output: &Output, expected: &Path, command: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(PLUGIN_EXIT_CODE),
        "unexpected child exit\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let marker = stdout
        .lines()
        .find_map(|line| line.strip_prefix(OUTPUT_PREFIX))
        .unwrap_or_else(|| panic!("native plugin fixture did not run: {stdout}"));
    let value: Value = serde_json::from_str(marker).expect("native plugin marker JSON");
    let actual = Path::new(value["executable"].as_str().expect("fixture executable"));
    assert_eq!(
        fs::canonicalize(actual).expect("canonical actual executable"),
        fs::canonicalize(expected).expect("canonical expected executable")
    );
    assert_eq!(value["command"], command);
    assert_eq!(value["output"], "human");
}

pub(super) fn assert_plugin_missing(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not installed"),
        "unexpected missing-plugin error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(OUTPUT_PREFIX));
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum SnapshotEntry {
    Directory,
    File { length: usize, digest: Vec<u8> },
}

pub(super) fn snapshot(root: &Path) -> BTreeMap<PathBuf, SnapshotEntry> {
    let mut entries = BTreeMap::new();
    if root.exists() {
        snapshot_entry(root, root, &mut entries);
    }
    entries
}

fn snapshot_entry(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, SnapshotEntry>) {
    let metadata = fs::symlink_metadata(path).expect("read snapshot metadata");
    let relative = path.strip_prefix(root).expect("relative snapshot path");
    if metadata.is_dir() {
        entries.insert(relative.to_path_buf(), SnapshotEntry::Directory);
        for entry in fs::read_dir(path).expect("read snapshot directory") {
            snapshot_entry(root, &entry.expect("read snapshot entry").path(), entries);
        }
    } else {
        assert!(
            metadata.is_file(),
            "unexpected fixture entry: {}",
            path.display()
        );
        let bytes = fs::read(path).expect("read snapshot file");
        let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
        entries.insert(
            relative.to_path_buf(),
            SnapshotEntry::File {
                length: bytes.len(),
                digest: digest.as_ref().to_vec(),
            },
        );
    }
}

// 复制测试程序而不是脚本，才能在 Windows 上验证真正的进程分派与退出码。
#[test]
#[ignore = "仅由复制的原生插件子进程通过完整测试名调用"]
fn fake_plugin_process() {
    assert_eq!(
        env::var(CHILD_MARKER_ENV).as_deref(),
        Ok(CHILD_MARKER_VALUE),
        "the native plugin fixture requires its child marker"
    );
    let value = serde_json::json!({
        "executable": env::current_exe().expect("fixture executable path"),
        "command": env::var("NETUTILS_PLUGIN_NAME").expect("forwarded plugin name"),
        "output": env::var("NETUTILS_OUTPUT").expect("forwarded output mode")
    });
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "\n{OUTPUT_PREFIX}{value}").expect("write native plugin marker");
    stdout.flush().expect("flush native plugin marker");
    std::process::exit(PLUGIN_EXIT_CODE);
}
