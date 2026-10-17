//! The external programs sf runs, found once at start.

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Where each engine lives; `None` when it is not installed. A tab that needs a missing one says so.
#[derive(Clone, Debug, Default)]
pub struct Engines {
    pub fd: Option<PathBuf>,
    pub bat: Option<PathBuf>,
    pub ugrep: Option<PathBuf>,
    pub ugrep_plus: Option<PathBuf>,
    pub git: Option<PathBuf>,
    pub delta: Option<PathBuf>,
    pub file: Option<PathBuf>,
    pub ls: Option<PathBuf>,
    /// bat's and delta's syntax theme: `$BAT_THEME`, else base16 (the terminal's own 16 colours,
    /// which the Ghostty theme sets to Catppuccin).
    pub theme: String,
}

/// Looks the engines up in `path_var` (a `$PATH` value). Debian and Ubuntu name fd and bat
/// `fdfind` and `batcat`.
pub fn discover(path_var: &OsStr) -> Engines {
    let find = |names: &[&str]| find(path_var, names);
    Engines {
        fd: find(&["fd", "fdfind"]),
        bat: find(&["bat", "batcat"]),
        ugrep: find(&["ugrep"]),
        ugrep_plus: find(&["ugrep+"]),
        git: find(&["git"]),
        delta: find(&["delta"]),
        file: find(&["file"]),
        ls: find(&["ls"]),
        theme: std::env::var("BAT_THEME")
            .ok()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "base16".into()),
    }
}

/// The bundle of a release package: `lib/sf` beside the folder of `exe` (`~/.local/bin/sf` →
/// `~/.local/lib/sf`), when it exists. It holds the engines and the sub-functions.
pub fn bundle_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?.parent()?.join("lib/sf");
    dir.is_dir().then_some(dir)
}

/// `path_var` with `bundle` in front, so the bundled engines win and every program sf starts finds
/// them too (ugrep+ runs `ug`, the sub-functions run fzf, bat and ugrep). A bundle path that cannot
/// stand in `$PATH` (it holds a `:`) leaves `path_var` as it is; `sf doctor` then shows the engines
/// found without it.
pub fn with_bundle(bundle: Option<&Path>, path_var: &OsStr) -> OsString {
    let Some(bundle) = bundle else {
        return path_var.to_owned();
    };
    let rest = std::env::split_paths(path_var).filter(|d| !d.as_os_str().is_empty());
    std::env::join_paths(std::iter::once(bundle.to_path_buf()).chain(rest))
        .unwrap_or_else(|_| path_var.to_owned())
}

/// The first of `names` found as an executable file in `path_var`, names in order of preference.
pub fn find(path_var: &OsStr, names: &[&str]) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = std::env::split_paths(path_var)
        .filter(|d| !d.as_os_str().is_empty())
        .collect();
    names.iter().find_map(|name| {
        dirs.iter().map(|d| d.join(name)).find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{executable, temp_dir};

    #[test]
    fn discover_finds_fdfind_and_batcat() {
        let dir = temp_dir("engines_debian_names");
        executable(&dir.join("fdfind"), "");
        executable(&dir.join("batcat"), "");
        let e = discover(dir.as_os_str());
        assert_eq!(e.fd, Some(dir.join("fdfind")));
        assert_eq!(e.bat, Some(dir.join("batcat")));
        assert_eq!(e.ugrep, None);
    }

    #[test]
    fn earlier_path_entry_wins_and_fd_beats_fdfind() {
        let a = temp_dir("engines_path_a");
        let b = temp_dir("engines_path_b");
        executable(&a.join("ugrep"), "");
        executable(&b.join("ugrep"), "");
        executable(&a.join("fdfind"), "");
        executable(&b.join("fd"), "");
        let path: OsString = std::env::join_paths([&a, &b]).unwrap();
        let e = discover(&path);
        assert_eq!(e.ugrep, Some(a.join("ugrep")));
        assert_eq!(e.fd, Some(b.join("fd")));
    }

    #[test]
    fn bundle_is_lib_sf_beside_the_bin_folder() {
        let root = temp_dir("bundle_dir");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let exe = root.join("bin/sf");
        assert_eq!(bundle_dir(&exe), None, "no lib/sf: a cargo install");
        std::fs::create_dir_all(root.join("lib/sf")).unwrap();
        assert_eq!(bundle_dir(&exe), Some(root.join("lib/sf")));
    }

    #[test]
    fn bundled_engines_come_before_path() {
        let bundle = temp_dir("bundle_first");
        let system = temp_dir("bundle_system");
        for dir in [&bundle, &system] {
            executable(&dir.join("ugrep"), "");
        }
        executable(&system.join("git"), "");
        let path = with_bundle(Some(&bundle), system.as_os_str());
        let e = discover(&path);
        assert_eq!(e.ugrep, Some(bundle.join("ugrep")));
        assert_eq!(
            e.git,
            Some(system.join("git")),
            "what the bundle lacks comes from PATH"
        );
        assert_eq!(with_bundle(None, OsStr::new("/a:/b")), "/a:/b");
        assert_eq!(
            with_bundle(Some(&bundle), OsStr::new("")),
            bundle.as_os_str()
        );
    }

    #[test]
    fn non_executable_files_and_folders_are_skipped() {
        let dir = temp_dir("engines_not_exec");
        std::fs::write(dir.join("git"), "").unwrap();
        std::fs::create_dir(dir.join("delta")).unwrap();
        let e = discover(dir.as_os_str());
        assert_eq!(e.git, None);
        assert_eq!(e.delta, None);
    }
}
