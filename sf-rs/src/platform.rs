//! Where sf runs, a Linux desktop or WSL, and the actions that reach the desktop there: open with the
//! default application, show in the file manager, copy to the clipboard. Every value reaches a program
//! as an argument, on stdin or in an environment variable, never inside a shell or PowerShell string.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Where sf runs, read once at start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Platform {
    /// WSL, with the distribution's name (empty when only the kernel or the interop tells).
    pub wsl: Option<String>,
    /// `$WAYLAND_DISPLAY`, `$DISPLAY`, `$XDG_SESSION_TYPE`, `$XDG_CURRENT_DESKTOP`; unset or empty: None.
    pub wayland: Option<String>,
    pub x11: Option<String>,
    pub session_type: Option<String>,
    pub desktop: Option<String>,
    /// Inside tmux (`$TMUX`).
    pub tmux: bool,
    /// The `$PATH` the programs are looked up in.
    pub path: OsString,
    /// `$WSLENV`, kept when sf adds its own variable to it.
    pub wslenv: Option<OsString>,
}

/// How text gets to the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clipboard {
    WlCopy(PathBuf),
    Xclip(PathBuf),
    Xsel(PathBuf),
    /// WSL: `Set-Clipboard` in Windows.
    PowerShell(PathBuf),
    /// No clipboard program: tmux sends the text to the terminal as OSC 52 (`load-buffer -w`).
    Tmux(PathBuf),
    /// No clipboard program: OSC 52 written to the terminal, which may or may not allow it.
    Osc52,
}

/// Exists on WSL while Windows programs can be started from Linux.
pub const WSL_INTEROP_FILE: &str = "/proc/sys/fs/binfmt_misc/WSLInterop";

/// The PowerShell statements; the value is always `$env:SF_VALUE`. None holds a `"`: WSL quotes each
/// argument for Windows, and a `"` inside one would change where it ends.
const PS_OPEN: &str = "Invoke-Item -LiteralPath $env:SF_VALUE";
const PS_REVEAL: &str = "Start-Process -FilePath explorer.exe -ArgumentList ('/select,' + [char]34 + $env:SF_VALUE + [char]34)";
const PS_COPY: &str = "Set-Clipboard -Value $env:SF_VALUE";

impl Platform {
    pub fn detect() -> Platform {
        let version = std::fs::read_to_string("/proc/version").ok();
        let interop = Path::new(WSL_INTEROP_FILE).exists();
        Platform::from_parts(|k| std::env::var_os(k), version.as_deref(), interop)
    }

    /// The platform from the environment (`var`), the text of `/proc/version` and whether
    /// `/proc/sys/fs/binfmt_misc/WSLInterop` exists. WSL 1 says "Microsoft" in its kernel version,
    /// WSL 2 "microsoft-standard".
    pub fn from_parts(
        var: impl Fn(&str) -> Option<OsString>,
        proc_version: Option<&str>,
        interop: bool,
    ) -> Platform {
        let set = |k: &str| var(k).filter(|v| !v.is_empty());
        let text = |k: &str| set(k).map(|v| v.to_string_lossy().into_owned());
        let distro = text("WSL_DISTRO_NAME");
        let microsoft = proc_version.is_some_and(|v| v.to_ascii_lowercase().contains("microsoft"));
        let wsl = (distro.is_some() || set("WSL_INTEROP").is_some() || interop || microsoft)
            .then(|| distro.unwrap_or_default());
        Platform {
            wsl,
            wayland: text("WAYLAND_DISPLAY"),
            x11: text("DISPLAY"),
            session_type: text("XDG_SESSION_TYPE"),
            desktop: text("XDG_CURRENT_DESKTOP"),
            tmux: set("TMUX").is_some(),
            path: var("PATH").unwrap_or_default(),
            wslenv: set("WSLENV"),
        }
    }

    fn find(&self, name: &str) -> Option<PathBuf> {
        crate::engines::find(&self.path, &[name])
    }

    /// The way copying works here: the first clipboard program the session can use, else OSC 52.
    pub fn clipboard(&self) -> Clipboard {
        let program = if self.wsl.is_some() {
            self.find("powershell.exe").map(Clipboard::PowerShell)
        } else {
            let wayland = self.wayland.as_ref().and_then(|_| self.find("wl-copy"));
            let x11 = |name, make: fn(PathBuf) -> Clipboard| {
                self.x11.as_ref().and_then(|_| self.find(name)).map(make)
            };
            wayland
                .map(Clipboard::WlCopy)
                .or_else(|| x11("xclip", Clipboard::Xclip))
                .or_else(|| x11("xsel", Clipboard::Xsel))
        };
        program
            .or_else(|| {
                self.tmux
                    .then(|| self.find("tmux"))
                    .flatten()
                    .map(Clipboard::Tmux)
            })
            .unwrap_or(Clipboard::Osc52)
    }

    /// Opens `path` (absolute) with its default application.
    pub fn open(&self, path: &Path) -> Result<(), String> {
        let path = existing(path)?;
        if self.wsl.is_some() {
            let win = self.windows_path(&path)?;
            return self.powershell(PS_OPEN, &win);
        }
        let xdg_open = self.xdg_open()?;
        launch(
            &self.path,
            xdg_open.as_os_str(),
            &[path.as_os_str()],
            None,
            &[],
            false,
        )
    }

    /// Shows `path` (absolute) selected in the file manager; a note when only its folder could be
    /// opened. Linux asks the file manager over D-Bus (`org.freedesktop.FileManager1`, which Nautilus,
    /// Dolphin, Nemo, Caja and Thunar answer); WSL starts Explorer with `/select,`.
    pub fn reveal(&self, path: &Path) -> Result<Option<String>, String> {
        let path = existing(path)?;
        if self.wsl.is_some() {
            let win = self.windows_path(&path)?;
            // Explorer parses /select,"PATH" itself: a '"' (legal in Linux names) would end the quotes
            // early and turn the rest of the name into further Explorer arguments.
            if win
                .as_bytes()
                .iter()
                .any(|&b| b == b'"' || b.is_ascii_control())
            {
                return Err(format!(
                    "Explorer cannot select a name containing '\"' or a control character: {}",
                    win.to_string_lossy()
                ));
            }
            return self.powershell(PS_REVEAL, &win).map(|()| None);
        }
        let dbus = match self.find("dbus-send") {
            Some(dbus_send) => {
                let item = format!("array:string:{}", file_uri(&path));
                let args = [
                    "--session",
                    "--print-reply",
                    "--dest=org.freedesktop.FileManager1",
                    "--type=method_call",
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1.ShowItems",
                    &item,
                    "string:",
                ]
                .map(OsStr::new);
                launch(&self.path, dbus_send.as_os_str(), &args, None, &[], true)
            }
            None => Err("dbus-send is not installed".into()),
        };
        let Err(why) = dbus else {
            return Ok(None);
        };
        let folder = path.parent().unwrap_or(&path);
        let xdg_open = self.xdg_open()?;
        launch(
            &self.path,
            xdg_open.as_os_str(),
            &[folder.as_os_str()],
            None,
            &[],
            false,
        )?;
        Ok(Some(format!("Opened the folder ({why})")))
    }

    /// Copies a path: on Linux `shown` as it is, on WSL the Windows form of `path` (absolute).
    pub fn copy_path(&self, shown: &OsStr, path: &Path) -> Result<Clipboard, String> {
        if self.wsl.is_some() {
            let win = self.windows_path(&existing(path)?)?;
            return self.copy_text(win.as_bytes());
        }
        self.copy_text(shown.as_bytes())
    }

    /// Copies `text`; says how.
    pub fn copy_text(&self, text: &[u8]) -> Result<Clipboard, String> {
        let how = self.clipboard();
        let stdin = |program: &Path, args: &[&str], keep_stderr| {
            let args = args.iter().map(OsStr::new).collect::<Vec<_>>();
            launch(
                &self.path,
                program.as_os_str(),
                &args,
                Some(text),
                &[],
                keep_stderr,
            )
        };
        match &how {
            // the three leave a process behind that serves the clipboard and would hold stderr open
            Clipboard::WlCopy(p) => stdin(p, &[], false),
            Clipboard::Xclip(p) => stdin(p, &["-selection", "clipboard"], false),
            Clipboard::Xsel(p) => stdin(p, &["--clipboard", "--input"], false),
            Clipboard::PowerShell(_) => self.powershell(PS_COPY, OsStr::from_bytes(text)),
            Clipboard::Tmux(p) => stdin(p, &["load-buffer", "-w", "-"], true),
            Clipboard::Osc52 => std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/tty")
                .and_then(|mut tty| tty.write_all(&osc52(text)))
                .map_err(|e| format!("No clipboard program, and the terminal: {e}")),
        }?;
        Ok(how)
    }

    fn xdg_open(&self) -> Result<PathBuf, String> {
        self.find("xdg-open")
            .ok_or_else(|| "xdg-open is not installed (package xdg-utils)".to_string())
    }

    /// The Windows form of an absolute path (`C:\…` or `\\wsl.localhost\…`) from `wslpath -w`.
    fn windows_path(&self, path: &Path) -> Result<OsString, String> {
        debug_assert!(
            path.is_absolute(),
            "an absolute path is never taken for an option"
        );
        let wslpath = self
            .find("wslpath")
            .ok_or_else(|| "wslpath not found in PATH".to_string())?;
        let out = Command::new(&wslpath)
            .arg("-w")
            .arg(path)
            .env("PATH", &self.path)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("wslpath: {e}"))?;
        let stdout = out.stdout.trim_ascii_end();
        if !out.status.success() || stdout.is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.lines().map(str::trim).find(|l| !l.is_empty());
            return Err(format!(
                "wslpath cannot convert {}{}",
                path.display(),
                err.map(|e| format!(": {e}")).unwrap_or_default()
            ));
        }
        Ok(OsStr::from_bytes(stdout).to_owned())
    }

    /// Runs one PowerShell statement with `$env:SF_VALUE` = `value`, shared with Windows through
    /// `WSLENV`, so neither cmd.exe nor PowerShell ever parses the value.
    fn powershell(&self, statement: &str, value: &OsStr) -> Result<(), String> {
        let powershell = self.find("powershell.exe").ok_or_else(|| {
            "powershell.exe not found in PATH (WSL interop and appendWindowsPath must be on)"
                .to_string()
        })?;
        let mut wslenv = OsString::from("SF_VALUE");
        if let Some(old) = &self.wslenv {
            wslenv.push(":");
            wslenv.push(old);
        }
        let command = format!("$ErrorActionPreference='Stop'; {statement}");
        let args = ["-NoProfile", "-NonInteractive", "-Command", &command].map(OsStr::new);
        let envs = [("SF_VALUE", value), ("WSLENV", wslenv.as_os_str())];
        launch(&self.path, powershell.as_os_str(), &args, None, &envs, true)
    }
}

impl Clipboard {
    /// What to tell when the text went the OSC 52 way, which cannot confirm it arrived.
    pub fn note(&self) -> Option<&'static str> {
        match self {
            Clipboard::Tmux(_) => Some("Copied through tmux (OSC 52)"),
            Clipboard::Osc52 => Some("Sent to the terminal (OSC 52)"),
            _ => None,
        }
    }
}

/// The names `sf NAME VALUE` takes.
pub const COMMANDS: [&str; 4] = ["open", "reveal", "copy-path", "copy-text"];

/// `sf open|reveal|copy-path PATH` and `sf copy-text TEXT`; a relative PATH is taken in `cwd`.
/// A note to show when it worked, the error when not.
pub fn command(
    name: &str,
    value: &OsStr,
    platform: &Platform,
    cwd: &Path,
) -> Result<Option<String>, String> {
    let note = |how: Clipboard| how.note().map(String::from);
    if name == "copy-text" {
        return platform.copy_text(value.as_bytes()).map(note);
    }
    if value.is_empty() {
        return Err("empty path".into());
    }
    let path = cwd.join(value);
    match name {
        "open" => platform.open(&path).map(|()| None),
        "reveal" => platform.reveal(&path),
        "copy-path" => platform.copy_path(value, &path).map(note),
        _ => Err(format!("unknown command: {name}")),
    }
}

/// `path`, normalized, when it exists.
fn existing(path: &Path) -> Result<PathBuf, String> {
    let path = normalize(path);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("no such file or folder: {}", path.display()))
    }
}

/// `path` with `.` and `..` resolved by name, as `realpath -s` does (symbolic links are kept).
pub fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// The `file://` URI of an absolute path, every byte but `A-Z a-z 0-9 - . _ ~ /` percent-encoded.
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

/// The OSC 52 sequence that asks the terminal to put `text` on the clipboard.
pub fn osc52(text: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = b"\x1b]52;c;".to_vec();
    for chunk in text.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ALPHABET[(n >> (18 - 6 * i) & 63) as usize]
            } else {
                b'='
            });
        }
    }
    out.push(0x07);
    out
}

/// How long a launcher (xdg-open, wl-copy, ...) may take before sf stops waiting for its exit status.
const LAUNCH_WAIT: Duration = Duration::from_secs(3);

/// Runs a launcher with no terminal: stdin from `input` (or nothing), stdout to /dev/null, stderr kept
/// only when `keep_stderr` (a program that leaves a daemon behind, as xdg-open's application or
/// wl-copy's clipboard server, would otherwise hold the pipe open). Its exit status is checked.
pub fn launch(
    path_var: &OsStr,
    program: &OsStr,
    args: &[&OsStr],
    input: Option<&[u8]>,
    envs: &[(&str, &OsStr)],
    keep_stderr: bool,
) -> Result<(), String> {
    let name = Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy()
        .into_owned();
    let mut child = Command::new(program)
        .args(args)
        .env("PATH", path_var)
        .envs(envs.iter().copied())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(if keep_stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{name}: {e}"))?;
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().expect("stdin is piped");
        stdin
            .write_all(bytes)
            .map_err(|e| format!("{name}: writing to it: {e}"))?;
    }
    let mut stderr = child.stderr.take();
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < LAUNCH_WAIT => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                thread::spawn(move || child.wait()); // still running: reap it later, assume it works
                return Ok(());
            }
            Err(e) => return Err(format!("{name}: {e}")),
        }
    };
    if status.success() {
        return Ok(());
    }
    let mut message = String::new();
    if let Some(err) = stderr.as_mut() {
        let _ = std::io::Read::read_to_string(err, &mut message);
    }
    let first = message
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string();
    let code = status
        .code()
        .map_or_else(|| "a signal".to_string(), |c| format!("exit {c}"));
    let what = if first.is_empty() {
        args.last()
            .map(|a| a.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        first
    };
    Err(format!("{name} failed ({code}): {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{calls, envs, stub, temp_dir};
    use std::collections::HashMap;
    use std::os::unix::ffi::OsStringExt;

    fn detect(vars: &[(&str, &str)], proc_version: &str, interop: bool) -> Platform {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        Platform::from_parts(|k| vars.get(k).cloned(), Some(proc_version), interop)
    }

    const DEBIAN: &str =
        "Linux version 6.12.111+deb13-amd64 (debian-kernel@lists.debian.org) #1 SMP";
    const WSL1: &str =
        "Linux version 4.4.0-19041-Microsoft (Microsoft@Microsoft.com) (gcc version 5.4.0)";
    const WSL2: &str =
        "Linux version 5.15.153.1-microsoft-standard-WSL2 (root@941d701f84f1) (gcc 11.2.0)";

    #[test]
    fn wsl_from_the_distribution_name() {
        let p = detect(&[("WSL_DISTRO_NAME", "Ubuntu-24.04")], DEBIAN, false);
        assert_eq!(p.wsl.as_deref(), Some("Ubuntu-24.04"));
    }

    #[test]
    fn wsl_from_interop_variable_file_or_kernel() {
        let interop_var = detect(&[("WSL_INTEROP", "/run/WSL/12_interop")], DEBIAN, false);
        assert_eq!(interop_var.wsl.as_deref(), Some(""));
        assert_eq!(detect(&[], DEBIAN, true).wsl.as_deref(), Some(""));
        assert_eq!(detect(&[], WSL1, false).wsl.as_deref(), Some(""));
        assert_eq!(detect(&[], WSL2, false).wsl.as_deref(), Some(""));
    }

    #[test]
    fn linux_records_the_session() {
        let p = detect(
            &[
                ("WAYLAND_DISPLAY", "wayland-0"),
                ("DISPLAY", ":1"),
                ("XDG_SESSION_TYPE", "wayland"),
                ("XDG_CURRENT_DESKTOP", "KDE"),
                ("TMUX", "/tmp/tmux-1000/default,1,0"),
                ("PATH", "/usr/bin:/bin"),
                ("WSLENV", ""),
                ("WSL_DISTRO_NAME", ""),
            ],
            DEBIAN,
            false,
        );
        assert_eq!(
            p,
            Platform {
                wsl: None,
                wayland: Some("wayland-0".into()),
                x11: Some(":1".into()),
                session_type: Some("wayland".into()),
                desktop: Some("KDE".into()),
                tmux: true,
                path: "/usr/bin:/bin".into(),
                wslenv: None,
            }
        );
    }

    #[test]
    fn nothing_set_is_plain_linux() {
        let p = detect(&[("TMUX", "")], DEBIAN, false);
        assert_eq!(p, Platform::default());
    }

    #[test]
    fn normalize_resolves_dots_by_name() {
        for (input, want) in [
            ("/a/b/../c/./d/", "/a/c/d"),
            ("/a/b/..", "/a"),
            ("/..", "/"),
            ("/a/../../b", "/b"),
            ("/", "/"),
        ] {
            assert_eq!(normalize(Path::new(input)), PathBuf::from(want), "{input}");
        }
    }

    #[test]
    fn file_uri_percent_encodes_all_but_unreserved() {
        assert_eq!(
            file_uri(Path::new("/home/x/a b,c%d/é#?.txt")),
            "file:///home/x/a%20b%2Cc%25d/%C3%A9%23%3F.txt"
        );
        assert_eq!(file_uri(Path::new("/A-z_0.9~")), "file:///A-z_0.9~");
        let latin1 = PathBuf::from(OsString::from_vec(b"/caf\xe9".to_vec()));
        assert_eq!(file_uri(&latin1), "file:///caf%E9");
    }

    #[test]
    fn osc52_is_base64_between_the_markers() {
        // RFC 4648 test vectors
        for (text, b64) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(
                osc52(text.as_bytes()),
                format!("\x1b]52;c;{b64}\x07").into_bytes(),
                "{text}"
            );
        }
        assert_eq!(osc52(&[0xff, 0x00, 0x10]), b"\x1b]52;c;/wAQ\x07");
    }

    /// A temp folder with the stubs `programs` and a platform whose PATH is only that folder.
    fn with(name: &str, programs: &[&str], platform: Platform) -> (PathBuf, Platform) {
        let bin = temp_dir(name);
        for p in programs {
            stub(&bin, p);
        }
        let path = bin.clone().into_os_string();
        (bin, Platform { path, ..platform })
    }

    fn wayland() -> Platform {
        Platform {
            wayland: Some("wayland-0".into()),
            x11: Some(":0".into()),
            ..Platform::default()
        }
    }

    fn x11() -> Platform {
        Platform {
            x11: Some(":0".into()),
            ..Platform::default()
        }
    }

    fn wsl() -> Platform {
        Platform {
            wsl: Some("Ubuntu".into()),
            wayland: Some("wayland-0".into()), // WSLg; the Windows clipboard still wins
            ..Platform::default()
        }
    }

    #[test]
    fn clipboard_is_chosen_by_session_and_programs() {
        let all = ["wl-copy", "xclip", "xsel", "powershell.exe", "tmux"];
        let (bin, p) = with("clip_wayland", &all, wayland());
        assert_eq!(p.clipboard(), Clipboard::WlCopy(bin.join("wl-copy")));
        let (bin, p) = with("clip_xwayland", &["xclip", "xsel"], wayland());
        assert_eq!(
            p.clipboard(),
            Clipboard::Xclip(bin.join("xclip")),
            "X11 programs on XWayland"
        );
        let (bin, p) = with("clip_x11", &all, x11());
        assert_eq!(
            p.clipboard(),
            Clipboard::Xclip(bin.join("xclip")),
            "no wl-copy on X11"
        );
        let (bin, p) = with("clip_xsel", &["xsel"], x11());
        assert_eq!(p.clipboard(), Clipboard::Xsel(bin.join("xsel")));
        let (bin, p) = with("clip_wsl", &all, wsl());
        assert_eq!(
            p.clipboard(),
            Clipboard::PowerShell(bin.join("powershell.exe"))
        );
    }

    #[test]
    fn clipboard_without_a_program_is_osc52() {
        let tmux = Platform {
            tmux: true,
            ..Platform::default()
        };
        let (bin, p) = with("clip_tmux", &["tmux", "wl-copy", "xclip"], tmux);
        assert_eq!(
            p.clipboard(),
            Clipboard::Tmux(bin.join("tmux")),
            "no display"
        );
        let (_, p) = with("clip_none", &["tmux"], wayland());
        assert_eq!(
            p.clipboard(),
            Clipboard::Osc52,
            "tmux there, but sf is not inside it"
        );
        let (bin, p) = with(
            "clip_wsl_none",
            &["tmux", "wl-copy"],
            Platform {
                tmux: true,
                ..wsl()
            },
        );
        assert_eq!(
            p.clipboard(),
            Clipboard::Tmux(bin.join("tmux")),
            "WSL without interop"
        );
    }

    #[test]
    fn copy_sends_the_text_on_stdin() {
        for (program, platform, args) in [
            ("wl-copy", wayland(), vec![]),
            ("xclip", x11(), vec!["-selection", "clipboard"]),
            ("xsel", x11(), vec!["--clipboard", "--input"]),
            (
                "tmux",
                Platform {
                    tmux: true,
                    ..Platform::default()
                },
                vec!["load-buffer", "-w", "-"],
            ),
        ] {
            let (bin, p) = with(&format!("copy_{program}"), &[program], platform);
            p.copy_text(b"a b\nc").unwrap();
            assert_eq!(calls(&bin, program), [args], "{program}");
            assert_eq!(
                std::fs::read(bin.join(format!("{program}.stdin"))).unwrap(),
                b"a b\nc",
                "{program}"
            );
        }
    }

    #[test]
    fn copy_path_on_linux_is_the_path_as_shown() {
        let (bin, p) = with("copy_path_linux", &["wl-copy"], wayland());
        let got = p.copy_path("src/a b.txt".as_ref(), Path::new("/r/src/a b.txt"));
        assert_eq!(got, Ok(Clipboard::WlCopy(bin.join("wl-copy"))));
        assert_eq!(
            std::fs::read(bin.join("wl-copy.stdin")).unwrap(),
            b"src/a b.txt"
        );
    }

    #[test]
    fn copy_failure_is_reported() {
        let (bin, p) = with("copy_fail", &["wl-copy"], wayland());
        std::fs::write(bin.join("wl-copy.exit"), "1").unwrap();
        let err = p.copy_text(b"x").unwrap_err();
        assert!(err.starts_with("wl-copy failed (exit 1)"), "{err}");
    }

    const PS_START: [&str; 3] = ["-NoProfile", "-NonInteractive", "-Command"];

    /// WSL stubs; wslpath answers `win` for every path.
    fn wsl_bin(name: &str, win: &str, platform: Platform) -> (PathBuf, Platform) {
        let (bin, p) = with(name, &["powershell.exe", "wslpath"], platform);
        std::fs::write(bin.join("wslpath.out"), format!("{win}\n")).unwrap();
        (bin, p)
    }

    #[test]
    fn wsl_copy_path_is_the_windows_path_in_sf_value() {
        let (bin, p) = wsl_bin("wsl_copy_path", r"C:\Users\b\a b.txt", wsl());
        let file = bin.join("a b.txt");
        std::fs::write(&file, "").unwrap();
        let got = p.copy_path("a b.txt".as_ref(), &file);
        assert_eq!(got, Ok(Clipboard::PowerShell(bin.join("powershell.exe"))));
        assert_eq!(calls(&bin, "wslpath"), [["-w", file.to_str().unwrap()]]);
        let mut want: Vec<String> = PS_START.map(String::from).into();
        want.push("$ErrorActionPreference='Stop'; Set-Clipboard -Value $env:SF_VALUE".into());
        assert_eq!(calls(&bin, "powershell.exe"), [want]);
        assert_eq!(
            envs(&bin, "powershell.exe"),
            [[r"SF_VALUE=C:\Users\b\a b.txt", "WSLENV=SF_VALUE"]]
        );
    }

    #[test]
    fn wsl_copy_text_keeps_wslenv() {
        let platform = Platform {
            wslenv: Some("USERPROFILE/p".into()),
            ..wsl()
        };
        let (bin, p) = wsl_bin("wsl_copy_text", "unused", platform);
        p.copy_text(b"abcdef1").unwrap();
        assert!(calls(&bin, "wslpath").is_empty());
        assert_eq!(
            envs(&bin, "powershell.exe"),
            [["SF_VALUE=abcdef1", "WSLENV=SF_VALUE:USERPROFILE/p"]]
        );
    }

    #[test]
    fn wsl_open_and_reveal_run_powershell() {
        let (bin, p) = wsl_bin("wsl_open", r"\\wsl.localhost\Ubuntu\home\b\x", wsl());
        let file = bin.join("x");
        std::fs::write(&file, "").unwrap();
        p.open(&file).unwrap();
        assert_eq!(p.reveal(&file), Ok(None));
        let scripts: Vec<String> = calls(&bin, "powershell.exe")
            .into_iter()
            .map(|c| {
                assert_eq!(c[..3], PS_START);
                c[3].clone()
            })
            .collect();
        assert_eq!(
            scripts,
            [
                "$ErrorActionPreference='Stop'; Invoke-Item -LiteralPath $env:SF_VALUE",
                "$ErrorActionPreference='Stop'; Start-Process -FilePath explorer.exe \
                 -ArgumentList ('/select,' + [char]34 + $env:SF_VALUE + [char]34)"
            ]
        );
        // WSL quotes each argument for Windows: a '"' in a statement would change it
        assert!(scripts.iter().all(|s| !s.contains('"')));
        for env in envs(&bin, "powershell.exe") {
            assert_eq!(env[0], r"SF_VALUE=\\wsl.localhost\Ubuntu\home\b\x");
        }
    }

    #[test]
    fn wsl_reveal_refuses_quotes_and_control_characters() {
        for win in [r#"C:\a"b"#, "C:\\a\tb"] {
            let (bin, p) = wsl_bin("wsl_reveal_quote", win, wsl());
            let err = p.reveal(&bin).unwrap_err();
            assert!(err.contains("Explorer cannot select"), "{err}");
            assert!(calls(&bin, "powershell.exe").is_empty());
        }
    }

    #[test]
    fn wsl_without_powershell_says_why() {
        let (bin, p) = with("wsl_no_ps", &["wslpath"], wsl());
        std::fs::write(bin.join("wslpath.out"), "C:\\x\n").unwrap();
        let err = p.open(&bin).unwrap_err();
        assert!(
            err.contains("powershell.exe") && err.contains("interop"),
            "{err}"
        );
    }

    #[test]
    fn wsl_wslpath_failure_is_reported() {
        let (bin, p) = wsl_bin("wsl_wslpath_fail", "x", wsl());
        std::fs::write(bin.join("wslpath.exit"), "1").unwrap();
        std::fs::write(bin.join("wslpath.err"), "wslpath: bad path\n").unwrap();
        let err = p.open(&bin).unwrap_err();
        assert!(err.contains("wslpath") && err.contains("bad path"), "{err}");
        assert!(calls(&bin, "powershell.exe").is_empty());
    }

    #[test]
    fn open_on_linux_is_xdg_open_on_the_normalized_path() {
        let (bin, p) = with("linux_open", &["xdg-open"], wayland());
        std::fs::create_dir(bin.join("d")).unwrap();
        std::fs::write(bin.join("-a b"), "").unwrap();
        p.open(&bin.join("d/../-a b")).unwrap();
        assert_eq!(
            calls(&bin, "xdg-open"),
            [[bin.join("-a b").to_str().unwrap()]]
        );
    }

    #[test]
    fn open_refuses_a_missing_path_and_a_missing_xdg_open() {
        let (bin, p) = with("linux_open_missing", &["xdg-open"], wayland());
        let err = p.open(&bin.join("gone")).unwrap_err();
        assert!(err.contains("no such file or folder"), "{err}");
        assert!(calls(&bin, "xdg-open").is_empty());
        let (bin, p) = with("linux_no_xdg_open", &[], wayland());
        let err = p.open(&bin).unwrap_err();
        assert!(err.contains("xdg-open"), "{err}");
    }

    #[test]
    fn reveal_on_linux_asks_the_file_manager_over_dbus() {
        let (bin, p) = with("linux_reveal", &["dbus-send", "xdg-open"], wayland());
        let file = bin.join("a b,c.txt");
        std::fs::write(&file, "").unwrap();
        assert_eq!(p.reveal(&file), Ok(None));
        let uri = format!("file://{}/a%20b%2Cc.txt", bin.to_str().unwrap());
        assert_eq!(
            calls(&bin, "dbus-send"),
            [vec![
                "--session".to_string(),
                "--print-reply".into(),
                "--dest=org.freedesktop.FileManager1".into(),
                "--type=method_call".into(),
                "/org/freedesktop/FileManager1".into(),
                "org.freedesktop.FileManager1.ShowItems".into(),
                format!("array:string:{uri}"),
                "string:".into(),
            ]]
        );
        assert!(calls(&bin, "xdg-open").is_empty());
    }

    #[test]
    fn reveal_on_linux_falls_back_to_the_folder() {
        let (bin, p) = with(
            "linux_reveal_fallback",
            &["dbus-send", "xdg-open"],
            wayland(),
        );
        std::fs::write(bin.join("dbus-send.exit"), "1").unwrap();
        std::fs::write(
            bin.join("dbus-send.err"),
            "Error org.freedesktop.DBus.Error.ServiceUnknown: no FileManager1\n",
        )
        .unwrap();
        std::fs::create_dir(bin.join("sub")).unwrap();
        let note = p.reveal(&bin.join("sub")).unwrap().unwrap();
        assert!(
            note.starts_with("Opened the folder") && note.contains("ServiceUnknown"),
            "{note}"
        );
        assert_eq!(calls(&bin, "xdg-open"), [[bin.to_str().unwrap()]]);

        let (bin, p) = with("linux_reveal_no_dbus", &["xdg-open"], wayland());
        std::fs::write(bin.join("f"), "").unwrap();
        let note = p.reveal(&bin.join("f")).unwrap().unwrap();
        assert!(note.contains("dbus-send"), "{note}");
        assert_eq!(calls(&bin, "xdg-open"), [[bin.to_str().unwrap()]]);
    }

    #[test]
    fn command_resolves_a_relative_path_against_the_folder() {
        let (bin, p) = with("cmd_open", &["xdg-open", "dbus-send"], wayland());
        std::fs::create_dir(bin.join("d")).unwrap();
        std::fs::write(bin.join("a b"), "").unwrap();
        assert_eq!(command("open", "d/../a b".as_ref(), &p, &bin), Ok(None));
        assert_eq!(
            calls(&bin, "xdg-open"),
            [[bin.join("a b").to_str().unwrap()]]
        );
        assert_eq!(command("reveal", "/".as_ref(), &p, &bin), Ok(None));
        assert_eq!(calls(&bin, "dbus-send")[0][6], "array:string:file:///");
    }

    #[test]
    fn command_copies_the_path_as_typed_on_linux() {
        let (bin, p) = with("cmd_copy", &["wl-copy"], wayland());
        assert_eq!(command("copy-path", "./x y".as_ref(), &p, &bin), Ok(None));
        assert_eq!(std::fs::read(bin.join("wl-copy.stdin")).unwrap(), b"./x y");
        assert_eq!(command("copy-text", "".as_ref(), &p, &bin), Ok(None));
        assert_eq!(std::fs::read(bin.join("wl-copy.stdin")).unwrap(), b"");
    }

    #[test]
    fn command_notes_osc52_through_tmux() {
        let tmux = Platform {
            tmux: true,
            ..Platform::default()
        };
        let (bin, p) = with("cmd_copy_tmux", &["tmux"], tmux);
        let note = command("copy-text", "abc".as_ref(), &p, &bin)
            .unwrap()
            .unwrap();
        assert!(note.contains("OSC 52"), "{note}");
    }

    #[test]
    fn command_refuses_an_empty_path_and_an_unknown_name() {
        let (bin, p) = with("cmd_bad", &["xdg-open", "wl-copy"], wayland());
        for name in ["open", "reveal", "copy-path"] {
            let err = command(name, "".as_ref(), &p, &bin).unwrap_err();
            assert!(err.contains("empty path"), "{name}: {err}");
        }
        assert!(command("delete", "x".as_ref(), &p, &bin).is_err());
        assert!(calls(&bin, "xdg-open").is_empty() && calls(&bin, "wl-copy").is_empty());
    }

    #[test]
    fn launch_reports_a_missing_program() {
        let err = launch(
            temp_dir("launch_missing").as_os_str(),
            "no-such-program".as_ref(),
            &[],
            None,
            &[],
            true,
        )
        .unwrap_err();
        assert!(err.starts_with("no-such-program: "), "{err}");
    }
}
