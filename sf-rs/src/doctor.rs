//! `sf doctor`: what sf finds on this system and how each action will work, as plain text.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::actions::Env;
use crate::engines::{Engines, find};
use crate::platform::Clipboard;

/// Runs a program with arguments and gives its standard output; None when it cannot start, fails or
/// does not end in time.
pub type Run<'a> = &'a dyn Fn(&Path, &[&str]) -> Option<String>;

/// ugrep versions, measured with Debian's and Ubuntu's packages (docs/sf-tui-design.md): Matching lines
/// needs `--replace` (3.6.0); before 5.0.0 (3.7.2, 3.11.2) only a line's first match is marked.
const UGREP_REPLACE: (u32, u32, u32) = (3, 6, 0);
const UGREP_ALL_MATCHES: (u32, u32, u32) = (5, 0, 0);
/// git config --show-scope, which the diff view needs to turn off the repository's own filters.
const GIT_SHOW_SCOPE: (u32, u32, u32) = (2, 26, 0);

const FILE_MANAGER: &str = "org.freedesktop.FileManager1";

/// The report. `config` is the config file's path, None when there is no config folder; `root` the
/// search root (sf's current folder), None when it cannot be read; `tmux` tmux's `show-options -s`, None
/// outside tmux.
pub fn report(
    env: &Env,
    engines: &Engines,
    config: Option<&Path>,
    root: Option<&Path>,
    tmux: Option<&str>,
    run: Run,
) -> String {
    let p = &env.platform;
    let mut out = format!("sf {}\n", env!("CARGO_PKG_VERSION"));
    let mut row = |label: &str, value: &str| {
        let line = format!("{label:<14}{value}");
        let _ = writeln!(out, "{}", line.trim_end());
    };
    let shown = |name: &str, path: &Path| format!("{name} {}", path.display());

    let mut platform = match &p.wsl {
        Some(distro) if distro.is_empty() => vec!["WSL".to_string()],
        Some(distro) => vec![format!("WSL ({distro})")],
        None => {
            let mut parts = vec!["Linux".to_string()];
            parts.extend(p.wayland.as_ref().map(|w| format!("Wayland ({w})")));
            parts.extend(p.x11.as_ref().map(|x| format!("X11 ({x})")));
            parts.extend(p.desktop.clone());
            if p.wayland.is_none() && p.x11.is_none() {
                // no display: "tty" or "wayland" (e.g. under sudo) tells why
                parts.extend(p.session_type.as_ref().map(|t| format!("session {t}")));
            }
            parts
        }
    };
    if p.tmux {
        platform.push("inside tmux".into());
    }
    row("Platform", &platform.join(", "));
    if let Some(root) = root.filter(|r| p.wsl.is_some() && r.starts_with("/mnt")) {
        row(
            "Search root",
            &format!(
                "{} ! a WSL drive mount, much slower: run sf from the Linux filesystem (~)",
                root.display()
            ),
        );
    }
    // tmux's `always` sends modified keys in its xterm format unasked, a form crossterm drops
    let setting = |name| tmux.and_then(|t| crate::platform::setting(t, name));
    if p.tmux
        && setting("extended-keys") == Some("always")
        && setting("extended-keys-format") == Some("xterm")
    {
        row(
            "tmux",
            "extended-keys always with the xterm format: Shift-Tab, Ctrl-Alt-F and Ctrl-Alt-H \
             cannot reach sf; set -s extended-keys-format csi-u",
        );
    }

    let program = |name: &str| find(&p.path, &[name]);
    if p.wsl.is_some() {
        row(
            "Windows",
            &program("powershell.exe").map_or_else(
                || "powershell.exe not found (WSL interop and appendWindowsPath must be on)".into(),
                |ps| shown("powershell.exe", &ps),
            ),
        );
        row(
            "Paths",
            &program("wslpath")
                .map_or_else(|| "wslpath not found".into(), |w| shown("wslpath", &w)),
        );
        row("Open", "Invoke-Item (PowerShell)");
        row("Reveal", "explorer.exe /select, (PowerShell)");
    } else {
        row(
            "Open",
            &program("xdg-open").map_or_else(
                || "xdg-open not found (package xdg-utils)".into(),
                |x| shown("xdg-open", &x),
            ),
        );
        let reveal = match program("dbus-send") {
            None => "dbus-send not found: xdg-open opens the folder".to_string(),
            Some(dbus) => file_manager(&dbus, run),
        };
        row("Reveal", &reveal);
    }
    let clipboard = match p.clipboard() {
        Clipboard::WlCopy(c) => shown("wl-copy", &c),
        Clipboard::Xclip(c) => shown("xclip", &c),
        Clipboard::Xsel(c) => shown("xsel", &c),
        Clipboard::PowerShell(_) => "Set-Clipboard (PowerShell)".into(),
        Clipboard::Tmux(t) => format!("OSC 52 through tmux {} (no clipboard program)", t.display()),
        Clipboard::Osc52 => {
            "OSC 52 to the terminal (no clipboard program; the terminal must allow it)".into()
        }
    };
    row("Clipboard", &clipboard);

    row("Engines", "");
    // Ctrl-O's editor where it is: `$EDITOR` may name a path or a program in PATH
    let editor = env.editor.as_ref().and_then(|e| {
        let given = Path::new(&e.program);
        if given.components().count() > 1 {
            Some(given.to_path_buf())
        } else {
            given.to_str().and_then(program)
        }
    });
    let list: [(&str, Option<PathBuf>, &str); 10] = [
        ("fd", engines.fd.clone(), "needed for Files and Folders"),
        ("bat", engines.bat.clone(), "file previews"),
        ("ugrep", engines.ugrep.clone(), "needed for Content"),
        ("ugrep+", engines.ugrep_plus.clone(), "Archives & documents"),
        ("git", engines.git.clone(), "needed for Git, diffs and logs"),
        ("delta", engines.delta.clone(), "commits and diffs"),
        ("file", engines.file.clone(), "previews of binary files"),
        ("ls", engines.ls.clone(), "folder previews"),
        ("editor", editor, "Ctrl-O: install nvim or set $EDITOR"),
        (
            "tmux",
            program("tmux"),
            "optional: the actions in windows of their own",
        ),
    ];
    let width = list
        .iter()
        .filter_map(|(_, path, _)| path.as_ref().map(|p| p.as_os_str().len()))
        .max()
        .unwrap_or(0);
    for (name, path, purpose) in &list {
        let value = match path {
            None => format!("not found ({purpose})"),
            Some(path) => {
                let flag = if *name == "tmux" { "-V" } else { "--version" };
                let version = run(path, &[flag]);
                let first = version
                    .as_deref()
                    .and_then(|v| v.lines().map(str::trim).find(|l| !l.is_empty()))
                    .map_or("(no version)", |l| {
                        l.split(';').next().unwrap_or(l).trim_end()
                    });
                let mut value = format!("{:<width$}  {first}", path.display());
                // the diff view asks git for the repository's filters with --show-scope
                if *name == "git" && version_in(first, 2).is_some_and(|v| v < GIT_SHOW_SCOPE) {
                    value.push_str("  ! older than 2.26: Ctrl-G shows no diff (no --show-scope)");
                }
                let ugrep = name.starts_with("ugrep").then(|| version_in(first, 1));
                match ugrep.flatten() {
                    // sf starts each ugrep+ with --no-config (5.0.0), so a searched folder's .ugrep
                    // runs nothing
                    Some(v) if *name == "ugrep+" && v < UGREP_ALL_MATCHES => value.push_str(
                        "  ! older than 5.0.0: Archives and Documents will not work (no --no-config)",
                    ),
                    Some(v) if v < UGREP_REPLACE => {
                        value.push_str("  ! older than 3.6.0: Matching lines will not work")
                    }
                    Some(v) if v < UGREP_ALL_MATCHES => value.push_str(
                        "  ! older than 5.0.0: Matching lines marks only a line's first match",
                    ),
                    _ => {}
                }
                value
            }
        };
        row(&format!("  {name}"), &value);
    }

    match config {
        None => row(
            "Config",
            "no config folder: neither XDG_CONFIG_HOME nor HOME is set",
        ),
        Some(path) if !path.exists() => row(
            "Config",
            &format!("{} (not there yet: defaults)", path.display()),
        ),
        Some(path) => {
            row("Config", &path.display().to_string());
            if let Err(e) = crate::config::load(path) {
                row("  invalid:", &e);
            }
        }
    }
    out
}

/// Whether a file manager answers `ShowItems` on the session bus.
fn file_manager(dbus_send: &Path, run: Run) -> String {
    let ask = |method: &str, arg: Option<&str>| {
        let mut args = vec![
            "--session",
            "--print-reply=literal",
            "--dest=org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            method,
        ];
        args.extend(arg);
        run(dbus_send, &args).unwrap_or_default()
    };
    let name = format!("string:{FILE_MANAGER}");
    if ask("org.freedesktop.DBus.NameHasOwner", Some(&name)).contains("true") {
        "file manager over D-Bus (running)".into()
    } else if ask("org.freedesktop.DBus.ListActivatableNames", None)
        .split_whitespace()
        .any(|n| n == FILE_MANAGER)
    {
        "file manager over D-Bus (starts when asked)".into()
    } else {
        "no file manager on D-Bus: xdg-open opens the folder".into()
    }
}

/// (major, minor, patch) from word `n` of a first `--version` line: 1 in "ugrep 7.8.5 x86_64-…",
/// 2 in "git version 2.47.3".
fn version_in(line: &str, n: usize) -> Option<(u32, u32, u32)> {
    let mut numbers = line
        .split_whitespace()
        .nth(n)?
        .split('.')
        .map(|n| n.parse().ok());
    Some((
        numbers.next()??,
        numbers.next()??,
        numbers.next().flatten().unwrap_or(0),
    ))
}

/// `program args` with stdin from /dev/null and stderr dropped; its stdout when it ends with success
/// within `wait`, else None (and its process group is killed).
pub fn run_program(
    path_var: &OsStr,
    program: &Path,
    args: &[&str],
    wait: Duration,
) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .env("PATH", path_var)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    let deadline = Instant::now() + wait;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                // SAFETY: killpg on the group of a child sf started (process_group(0) above).
                unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
                let _ = child.wait();
                return None;
            }
            _ => return None,
        }
    }
    let bytes = rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::testutil::{executable, temp_dir};
    use std::path::PathBuf;

    /// The text after `label` on its line, spaces collapsed.
    fn line(report: &str, label: &str) -> String {
        let row = report
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{label} ")))
            .unwrap_or_else(|| panic!("no {label} line in\n{report}"));
        let rest = row.trim_start().strip_prefix(label).unwrap();
        rest.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    struct Setup {
        bin: PathBuf,
        env: Env,
        engines: Engines,
        config: PathBuf,
    }

    /// A temp folder with the (empty, executable) programs and a config path that does not exist yet.
    fn setup(name: &str, programs: &[&str], platform: Platform) -> Setup {
        let base = temp_dir(name);
        let bin = base.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for p in programs {
            executable(&bin.join(p), "");
        }
        let path = bin.clone().into_os_string();
        let engines = crate::engines::discover(&path);
        Setup {
            bin,
            env: Env {
                editor: crate::actions::find_editor(&path, None, None),
                platform: Platform { path, ..platform },
                sf: None,
                window_env: Vec::new(),
            },
            engines,
            config: base.join("config.toml"),
        }
    }

    /// Versions and D-Bus answers as the real programs give them.
    fn answers(program: &Path, args: &[&str]) -> Option<String> {
        let name = program.file_name()?.to_str()?;
        let out = match (name, args) {
            ("fd", ["--version"]) => "fd 10.3.0\n",
            ("ugrep" | "ugrep+", ["--version"]) => {
                "ugrep 7.8.5 x86_64-pc-linux-gnu +avx2; -P:pcre2jit\nLicense: BSD-3-Clause\n"
            }
            ("tmux", ["-V"]) => "tmux 3.5a\n",
            ("dbus-send", [.., "org.freedesktop.DBus.NameHasOwner", _]) => "   boolean true\n",
            _ => return None,
        };
        Some(out.to_string())
    }

    fn wayland_tmux() -> Platform {
        Platform {
            wayland: Some("wayland-0".into()),
            desktop: Some("KDE".into()),
            tmux: true,
            ..Platform::default()
        }
    }

    #[test]
    fn linux_report_names_each_method() {
        let s = setup(
            "doctor_linux",
            &["xdg-open", "dbus-send", "wl-copy", "fd", "tmux"],
            wayland_tmux(),
        );
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        let bin = s.bin.display();
        assert_eq!(
            line(&r, "Platform"),
            "Linux, Wayland (wayland-0), KDE, inside tmux"
        );
        assert_eq!(line(&r, "Open"), format!("xdg-open {bin}/xdg-open"));
        assert_eq!(line(&r, "Reveal"), "file manager over D-Bus (running)");
        assert_eq!(line(&r, "Clipboard"), format!("wl-copy {bin}/wl-copy"));
    }

    #[test]
    fn session_type_shows_only_without_a_display() {
        let tty = Platform {
            session_type: Some("tty".into()),
            ..Platform::default()
        };
        let s = setup("doctor_tty", &[], tty);
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert_eq!(line(&r, "Platform"), "Linux, session tty");
        let wayland = Platform {
            session_type: Some("wayland".into()),
            ..wayland_tmux()
        };
        let s = setup("doctor_session", &[], wayland);
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert_eq!(
            line(&r, "Platform"),
            "Linux, Wayland (wayland-0), KDE, inside tmux"
        );
    }

    #[test]
    fn reveal_says_when_only_the_folder_opens() {
        let s = setup("doctor_no_fm", &["xdg-open", "dbus-send"], wayland_tmux());
        let none = |_: &Path, _: &[&str]| Some("   boolean false\n   array [\n   ]\n".to_string());
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &none);
        assert!(
            line(&r, "Reveal").contains("xdg-open opens the folder"),
            "{r}"
        );
        let activatable = |_: &Path, args: &[&str]| {
            Some(match args.last() {
                Some(&"org.freedesktop.DBus.ListActivatableNames") => {
                    "   array [\n      org.freedesktop.FileManager1   ]\n".to_string()
                }
                _ => "   boolean false\n".to_string(),
            })
        };
        let r = report(
            &s.env,
            &s.engines,
            Some(&s.config),
            None,
            None,
            &activatable,
        );
        assert_eq!(
            line(&r, "Reveal"),
            "file manager over D-Bus (starts when asked)"
        );
        let s = setup("doctor_no_dbus", &["xdg-open"], wayland_tmux());
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert!(line(&r, "Reveal").starts_with("dbus-send not found"), "{r}");
    }

    #[test]
    fn clipboard_without_a_program_says_osc52() {
        let s = setup("doctor_osc52_tmux", &["tmux"], Platform::default());
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert!(line(&r, "Clipboard").starts_with("OSC 52"), "{r}");
    }

    #[test]
    fn engines_show_path_and_version_or_what_needs_them() {
        let s = setup("doctor_engines", &["fd", "ugrep"], wayland_tmux());
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert_eq!(line(&r, "fd"), format!("{}/fd fd 10.3.0", s.bin.display()));
        assert_eq!(
            line(&r, "ugrep"),
            format!(
                "{}/ugrep ugrep 7.8.5 x86_64-pc-linux-gnu +avx2",
                s.bin.display()
            ),
            "the version line ends before its feature list"
        );
        assert!(r.lines().all(|l| !l.ends_with(' ')), "{r:?}");
        assert!(line(&r, "delta").starts_with("not found"), "{r}");
        assert!(line(&r, "editor").contains("Ctrl-O"), "{r}");
        assert!(!line(&r, "ugrep").contains("older"), "{r}");
    }

    #[test]
    fn a_git_without_show_scope_is_warned_about() {
        let s = setup("doctor_old_git", &["git"], wayland_tmux());
        for (version, old) in [
            ("git version 2.25.1", true),
            ("git version 2.26.0", false),
            ("git version 2.39.5 (Apple Git-154)", false),
        ] {
            let answer = |_: &Path, _: &[&str]| Some(format!("{version}\n"));
            let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answer);
            let row = line(&r, "git");
            assert_eq!(row.contains("older than 2.26"), old, "{version}: {row}");
        }
    }

    #[test]
    fn an_old_ugrep_is_warned_about() {
        // measured: 3.3.3 has no --replace; 3.7.2 and 3.11.2 mark only a line's first match; 5.0.0 as 7.8.5
        let s = setup("doctor_old_ugrep", &["ugrep", "ugrep+"], wayland_tmux());
        for (version, plus) in [
            (
                "ugrep 4.5.2 x86_64-pc-linux-gnu",
                Some("Archives and Documents"),
            ),
            ("ugrep 5.0.0 x86_64-pc-linux-gnu", None),
        ] {
            let answer = |_: &Path, _: &[&str]| Some(format!("{version}\n"));
            let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answer);
            let row = line(&r, "ugrep+");
            match plus {
                Some(w) => assert!(row.contains(w), "{version}: {row}"),
                None => assert!(!row.contains("older"), "{version}: {row}"),
            }
        }
        for (version, warning) in [
            (
                "ugrep 3.3.3 x86_64-pc-linux-gnu +sse2",
                Some("older than 3.6.0"),
            ),
            ("ugrep 3.6", Some("older than 5.0.0")),
            ("ugrep 3.11.2 x86_64-pc-linux-gnu", Some("older than 5.0.0")),
            ("ugrep 5.0.0 x86_64-pc-linux-gnu +sse2; -P:pcre2jit", None),
        ] {
            let answer = |_: &Path, _: &[&str]| Some(format!("{version}\n"));
            let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answer);
            let row = line(&r, "ugrep");
            match warning {
                Some(w) => assert!(row.contains(w), "{version}: {row}"),
                None => assert!(!row.contains("older"), "{version}: {row}"),
            }
        }
    }

    #[test]
    fn wsl_report_goes_through_powershell() {
        let wsl = Platform {
            wsl: Some("Ubuntu-24.04".into()),
            ..Platform::default()
        };
        let s = setup("doctor_wsl", &["powershell.exe", "wslpath"], wsl.clone());
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        let bin = s.bin.display();
        assert_eq!(line(&r, "Platform"), "WSL (Ubuntu-24.04)");
        assert_eq!(
            line(&r, "Windows"),
            format!("powershell.exe {bin}/powershell.exe")
        );
        assert_eq!(line(&r, "Paths"), format!("wslpath {bin}/wslpath"));
        assert_eq!(line(&r, "Clipboard"), "Set-Clipboard (PowerShell)");
        let s = setup("doctor_wsl_no_interop", &[], wsl);
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert!(line(&r, "Windows").contains("interop"), "{r}");
    }

    #[test]
    fn a_search_root_under_mnt_is_warned_about_in_wsl() {
        let wsl = Platform {
            wsl: Some("Ubuntu-24.04".into()),
            ..Platform::default()
        };
        let s = setup("doctor_mnt", &[], wsl);
        let at = |root: Option<&str>| {
            report(
                &s.env,
                &s.engines,
                Some(&s.config),
                root.map(Path::new),
                None,
                &answers,
            )
        };
        for root in ["/mnt", "/mnt/c/Users/bilal/project"] {
            assert_eq!(
                line(&at(Some(root)), "Search root"),
                format!(
                    "{root} ! a WSL drive mount, much slower: run sf from the Linux filesystem (~)"
                )
            );
        }
        for root in [Some("/home/bilal/x"), Some("/mntx/c"), None] {
            let r = at(root);
            assert!(!r.contains("Search root"), "{root:?}: {r}");
        }
    }

    #[test]
    fn a_search_root_under_mnt_outside_wsl_is_no_warning() {
        // plain Linux mounts disks under /mnt too; only WSL's drive mounts are slow
        let s = setup("doctor_mnt_linux", &[], wayland_tmux());
        for root in ["/mnt", "/mnt/c/Users/bilal/project"] {
            let r = report(
                &s.env,
                &s.engines,
                Some(&s.config),
                Some(Path::new(root)),
                None,
                &answers,
            );
            assert!(!r.contains("Search root"), "{root}: {r}");
        }
    }

    #[test]
    fn tmux_extended_keys_always_with_the_xterm_format_is_warned_about() {
        // `always` sends ESC[27;7;102~ without being asked; crossterm drops it
        let s = setup("doctor_tmux_keys", &[], wayland_tmux());
        let at = |options: &str| {
            report(
                &s.env,
                &s.engines,
                Some(&s.config),
                None,
                Some(options),
                &answers,
            )
        };
        let always_xterm = "escape-time 10\nextended-keys always\nextended-keys-format xterm\n";
        assert_eq!(
            line(&at(always_xterm), "tmux"),
            "extended-keys always with the xterm format: Shift-Tab, Ctrl-Alt-F and Ctrl-Alt-H \
             cannot reach sf; set -s extended-keys-format csi-u"
        );
        for options in [
            "extended-keys on\nextended-keys-format csi-u\n",
            "extended-keys off\nextended-keys-format xterm\n",
        ] {
            let r = at(options);
            assert!(!r.lines().any(|l| l.starts_with("tmux ")), "{options}: {r}");
        }
        // outside tmux the server's options are no matter of sf's
        let s = setup("doctor_tmux_keys_outside", &[], Platform::default());
        let r = report(
            &s.env,
            &s.engines,
            Some(&s.config),
            None,
            Some(always_xterm),
            &answers,
        );
        assert!(!r.lines().any(|l| l.starts_with("tmux ")), "{r}");
    }

    #[test]
    fn config_is_there_absent_or_invalid() {
        let s = setup("doctor_config", &[], wayland_tmux());
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert!(
            line(&r, "Config").ends_with("(not there yet: defaults)"),
            "{r}"
        );
        std::fs::write(&s.config, "[files]\nhidden = \"x\"\n").unwrap();
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert!(line(&r, "invalid:").contains("line 2"), "{r}");
        std::fs::write(&s.config, "[files]\nhidden = true\n").unwrap();
        let r = report(&s.env, &s.engines, Some(&s.config), None, None, &answers);
        assert_eq!(line(&r, "Config"), s.config.display().to_string());
        let r = report(&s.env, &s.engines, None, None, None, &answers);
        assert!(line(&r, "Config").contains("HOME"), "{r}");
    }

    #[test]
    fn run_program_gives_stdout_and_gives_up_on_a_hang() {
        let dir = temp_dir("doctor_run");
        executable(
            &dir.join("v"),
            "#!/bin/sh\necho \"v 1.0 $1\"\necho oops >&2\n",
        );
        executable(&dir.join("bad"), "#!/bin/sh\necho no\nexit 3\n");
        executable(&dir.join("hang"), "#!/bin/sh\nexec sleep 5\n");
        let path = OsStr::new("/usr/bin:/bin");
        let wait = Duration::from_millis(300);
        assert_eq!(
            run_program(path, &dir.join("v"), &["--version"], wait),
            Some("v 1.0 --version\n".into())
        );
        assert_eq!(run_program(path, &dir.join("bad"), &[], wait), None);
        let started = std::time::Instant::now();
        assert_eq!(run_program(path, &dir.join("hang"), &[], wait), None);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(run_program(path, &dir.join("absent"), &[], wait), None);
    }
}
