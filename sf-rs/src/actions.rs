//! What the keys on a selected item do. Every program gets its arguments as a list (no shell); paths
//! come after `--`; the Search pattern reaches nvim through the environment, never as an Ex command.

use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::commands::{Cmd, delta_args, is_hash};
use crate::engines::Engines;
use crate::items::Item;
use crate::model::Tab;

/// One action on the selected item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// Enter: the default application.
    Open,
    /// Ctrl-E: the file manager.
    Reveal,
    /// Ctrl-C: the path (Git: the hash) to the clipboard.
    Copy,
    /// Ctrl-O: nvim.
    Editor,
    /// Ctrl-T: search inside the file.
    InFile,
    /// Ctrl-U: git diff side by side.
    DiffSplit,
    /// Ctrl-G: git diff through delta.
    Diff,
    /// Alt-G: short git diff.
    DiffShort,
    /// Alt-U: git log of the file.
    Log,
    /// Enter in Git: the whole commit, paged.
    ShowCommit,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Done; the message (if any) is shown for a moment.
    Done(Option<String>),
    /// A terminal program to run in the foreground (sf steps aside); a pipeline of one or more.
    Foreground(Vec<Cmd>),
    Failed(String),
}

/// Where sf runs.
#[derive(Clone, Debug)]
pub struct Env {
    /// WSL with `windows_action` from the WSL edition of the search functions.
    pub wsl: bool,
    pub tmux: bool,
    /// The sub-functions folder (`$SF_SUBFUNCTIONS`, else `~/.SearchFunctions/sub-functions`).
    pub sub: PathBuf,
    /// The `$PATH` the programs are looked up in.
    pub path: OsString,
}

impl Env {
    pub fn detect() -> Env {
        let sub = std::env::var_os("SF_SUBFUNCTIONS")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".SearchFunctions/sub-functions")))
            .unwrap_or_else(|| PathBuf::from("sub-functions"));
        let windows_action = sub
            .join("windows_action")
            .metadata()
            .is_ok_and(|m| m.is_file());
        let microsoft = std::fs::read_to_string("/proc/version")
            .is_ok_and(|v| v.to_ascii_lowercase().contains("microsoft"));
        Env {
            wsl: windows_action && microsoft,
            tmux: std::env::var_os("TMUX").is_some_and(|t| !t.is_empty()),
            sub,
            path: std::env::var_os("PATH").unwrap_or_default(),
        }
    }
}

const COPIED: &str = "Copied to the clipboard";
/// `sh -c CD_AND_EXEC sf DIR PROGRAM ARGS...`: runs PROGRAM in DIR; the script itself holds no data.
const CD_AND_EXEC: &str = "cd -- \"$1\" && shift && exec \"$@\"";
/// How long a launcher (xdg-open, wl-copy, ...) may take before sf stops waiting for its exit status.
const LAUNCH_WAIT: Duration = Duration::from_secs(3);

pub fn perform(
    kind: ActionKind,
    tab: Tab,
    item: &Item,
    query: &str,
    root: &Path,
    e: &Engines,
    env: &Env,
) -> Outcome {
    if !item.selectable {
        return Outcome::Failed("Nothing selected".into());
    }
    if tab == Tab::Git {
        let Some(hash) = item.key.to_str().filter(|h| is_hash(h)) else {
            return Outcome::Failed("Not a commit".into());
        };
        return match kind {
            ActionKind::ShowCommit | ActionKind::Open => show_commit(hash, e),
            ActionKind::Copy if env.wsl => {
                done(windows(env, "copy-text", hash.as_ref()), Some(COPIED))
            }
            ActionKind::Copy => done(
                launch(env, "wl-copy".as_ref(), &[], Some(hash.as_bytes()), false),
                Some(COPIED),
            ),
            _ => Outcome::Failed("Not available in Git".into()),
        };
    }
    let path = root.join(&item.key);
    match kind {
        ActionKind::Open if env.wsl => done(windows(env, "open", path.as_os_str()), None),
        ActionKind::Open => done(
            launch(env, "xdg-open".as_ref(), &[path.as_os_str()], None, false),
            None,
        ),
        ActionKind::Reveal if env.wsl => done(windows(env, "reveal", path.as_os_str()), None),
        ActionKind::Reveal => {
            let fm = env.sub.join("open_in_file_manager");
            done(
                launch(env, fm.as_os_str(), &[path.as_os_str()], None, true),
                None,
            )
        }
        // Linux: the path as listed (relative to the root); WSL: its Windows form
        ActionKind::Copy if env.wsl => {
            done(windows(env, "copy-path", path.as_os_str()), Some(COPIED))
        }
        ActionKind::Copy => done(
            launch(
                env,
                "wl-copy".as_ref(),
                &[],
                Some(item.key.as_bytes()),
                false,
            ),
            Some(COPIED),
        ),
        ActionKind::ShowCommit => Outcome::Failed("Not available here".into()),
        _ => terminal(kind, tab, item, query, root, &path, env),
    }
}

fn done(result: Result<(), String>, message: Option<&str>) -> Outcome {
    match result {
        Ok(()) => Outcome::Done(message.map(String::from)),
        Err(e) => Outcome::Failed(e),
    }
}

fn windows(env: &Env, action: &str, value: &std::ffi::OsStr) -> Result<(), String> {
    let program = env.sub.join("windows_action");
    launch(
        env,
        program.as_os_str(),
        &[action.as_ref(), value],
        None,
        true,
    )
}

/// Runs a launcher with no terminal: stdin from `input` (or nothing), stdout to /dev/null, stderr kept
/// only when `keep_stderr` (a program that leaves a daemon behind, as xdg-open's application or
/// wl-copy's clipboard server, would otherwise hold the pipe open). Its exit status is checked.
fn launch(
    env: &Env,
    program: &std::ffi::OsStr,
    args: &[&std::ffi::OsStr],
    input: Option<&[u8]>,
    keep_stderr: bool,
) -> Result<(), String> {
    let name = Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy()
        .into_owned();
    let mut child = Command::new(program)
        .args(args)
        .env("PATH", &env.path)
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

fn show_commit(hash: &str, e: &Engines) -> Outcome {
    let Some(git) = &e.git else {
        return Outcome::Failed("git is not installed".into());
    };
    match &e.delta {
        Some(delta) => Outcome::Foreground(vec![
            Cmd::new(git).args(["show", "--color=always", "--end-of-options", hash]),
            Cmd::new(delta)
                .arg("--paging=always")
                .args(delta_args(&e.theme)),
        ]),
        None => Outcome::Foreground(vec![Cmd::new(git).args(["show", "--end-of-options", hash])]),
    }
}

/// The programs that need a terminal: a tmux window of their own, else the foreground.
fn terminal(
    kind: ActionKind,
    tab: Tab,
    item: &Item,
    query: &str,
    root: &Path,
    path: &Path,
    env: &Env,
) -> Outcome {
    let sub = |name: &str| Cmd::new(env.sub.join(name)).arg(path);
    let (window, title, cmd) = match kind {
        ActionKind::Editor => ("nvim", "editor", editor(tab, item, query, path)),
        ActionKind::InFile => {
            let mut cmd = sub("search_in_single_file");
            if tab == Tab::Content && !query.is_empty() {
                cmd = cmd.arg(query);
            }
            ("infile", "file-content", cmd)
        }
        ActionKind::DiffSplit => ("diff", "git", sub("show_diffs")),
        ActionKind::Diff => ("diff", "git", sub("git_diff")),
        ActionKind::DiffShort => ("diff", "git", sub("git_diff_short")),
        ActionKind::Log => ("logs", "git", sub("show_logs")),
        _ => return Outcome::Failed(format!("{kind:?} needs no terminal")),
    };
    if !env.tmux {
        return Outcome::Foreground(vec![cmd]);
    }
    // More than one argument after the options: tmux runs the program itself, no shell. The folder
    // is never given with `-c`, which tmux expands as a format (a folder named `#(cmd)` would run
    // cmd); a fixed script changes into it, the folder being one of its arguments.
    let mut args: Vec<OsString> = ["new-window", "-n", window].map(OsString::from).into();
    for (k, v) in &cmd.env {
        args.push("-e".into()); // -e values are not expanded
        args.push(format!("{k}={v}").into());
    }
    args.extend(["sh", "-c", CD_AND_EXEC, "sf"].map(OsString::from));
    args.push(root.into());
    args.push(cmd.program.into());
    args.extend(cmd.args);
    let args: Vec<&std::ffi::OsStr> = args.iter().map(OsString::as_os_str).collect();
    let tmux = std::ffi::OsStr::new("tmux");
    let result = launch(env, tmux, &args, None, true).and_then(|()| {
        launch(
            env,
            tmux,
            &["select-pane".as_ref(), "-T".as_ref(), title.as_ref()],
            None,
            true,
        )
    });
    done(result, None)
}

/// nvim at the matching line, or searching for the Search pattern. The pattern goes in $SF_QUERY and
/// nvim reads it with `let @/ = $SF_QUERY`: as an Ex command (`-c "/pattern"`) a pattern with `/|`
/// would run whatever follows. Very magic (`\v`) is closest to ugrep's syntax; `\c` gives smart case.
fn editor(tab: Tab, item: &Item, query: &str, path: &Path) -> Cmd {
    let mut cmd = Cmd::new("nvim");
    if tab == Tab::Content {
        if let Some(line) = item.line {
            cmd = cmd.arg(format!("+{line}"));
        } else if !query.is_empty() {
            let case = if query.chars().any(char::is_uppercase) {
                ""
            } else {
                "\\c"
            };
            cmd = cmd
                .args([
                    "-c",
                    "let @/ = $SF_QUERY",
                    "-c",
                    "set hlsearch",
                    "-c",
                    "silent! call search(@/, 'c')",
                ])
                .env("SF_QUERY", &format!("{case}\\v{query}"));
        }
    }
    cmd.arg("--").arg(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ParseKind;
    use crate::items::parse;
    use crate::testutil::{executable, temp_dir};
    use std::os::unix::ffi::OsStringExt;

    /// A stub that writes its arguments (NUL-terminated) to NAME.args, appending, and its stdin to
    /// NAME.stdin; it exits with the code in the file NAME.exit, if there is one.
    fn stub(dir: &Path, name: &str) {
        let script = format!(
            "#!/bin/sh\nd='{d}'\nfor a in \"$@\"; do printf '%s\\0' \"$a\" >> \"$d/{name}.args\"; done\n\
             printf '\\n' >> \"$d/{name}.args\"\ncat > \"$d/{name}.stdin\"\n\
             [ -f \"$d/{name}.exit\" ] && exit \"$(cat \"$d/{name}.exit\")\"\nexit 0\n",
            d = dir.display()
        );
        executable(&dir.join(name), &script);
    }

    /// The argument lists of every call of the stub NAME.
    fn calls(dir: &Path, name: &str) -> Vec<Vec<String>> {
        let Ok(log) = std::fs::read(dir.join(format!("{name}.args"))) else {
            return Vec::new();
        };
        let calls = log.strip_suffix(b"\n").unwrap_or(&log);
        calls
            .split(|&b| b == b'\n')
            .map(|l| {
                l.split(|&b| b == 0)
                    .filter(|a| !a.is_empty())
                    .map(|a| String::from_utf8_lossy(a).into_owned())
                    .collect()
            })
            .collect()
    }

    struct Fixture {
        bin: PathBuf,
        env: Env,
        root: PathBuf,
    }

    fn fixture(name: &str, wsl: bool, tmux: bool) -> Fixture {
        let base = temp_dir(name);
        let (bin, sub, root) = (base.join("bin"), base.join("sub"), base.join("root dir"));
        for d in [&bin, &sub, &root] {
            std::fs::create_dir_all(d).unwrap();
        }
        for p in ["xdg-open", "wl-copy", "tmux", "nvim"] {
            stub(&bin, p);
        }
        for p in [
            "open_in_file_manager",
            "windows_action",
            "search_in_single_file",
            "show_diffs",
            "git_diff",
            "git_diff_short",
            "show_logs",
        ] {
            stub(&sub, p);
        }
        let path = std::env::join_paths([
            bin.clone(),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ])
        .unwrap();
        Fixture {
            bin,
            env: Env {
                wsl,
                tmux,
                sub,
                path,
            },
            root,
        }
    }

    fn engines() -> Engines {
        Engines {
            git: Some("/usr/bin/git".into()),
            delta: Some("/usr/bin/delta".into()),
            theme: "base16".into(),
            ..Engines::default()
        }
    }

    fn file(key: &str) -> Item {
        parse(ParseKind::Files, key.as_bytes(), 0).unwrap()
    }

    fn act(f: &Fixture, kind: ActionKind, tab: Tab, item: &Item, query: &str) -> Outcome {
        perform(kind, tab, item, query, &f.root, &engines(), &f.env)
    }

    #[test]
    fn open_runs_xdg_open_with_real_path() {
        let f = fixture("act_open", false, false);
        let key = "-a b&c'd $(x).txt";
        assert_eq!(
            act(&f, ActionKind::Open, Tab::Files, &file(key), ""),
            Outcome::Done(None)
        );
        assert_eq!(
            calls(&f.bin, "xdg-open"),
            [[f.root.join(key).to_str().unwrap()]]
        );
    }

    #[test]
    fn open_keeps_non_utf8_bytes() {
        let f = fixture("act_open_bytes", false, false);
        let item = parse(ParseKind::Files, b"caf\xe9.txt", 0).unwrap();
        act(&f, ActionKind::Open, Tab::Files, &item, "");
        let log = std::fs::read(f.bin.join("xdg-open.args")).unwrap();
        let mut want = f
            .root
            .join(OsString::from_vec(b"caf\xe9.txt".to_vec()))
            .into_os_string()
            .into_vec();
        want.extend_from_slice(b"\0\n");
        assert_eq!(log, want);
    }

    #[test]
    fn xdg_open_failure_is_reported() {
        let f = fixture("act_open_fail", false, false);
        std::fs::write(f.bin.join("xdg-open.exit"), "4").unwrap();
        let Outcome::Failed(msg) = act(&f, ActionKind::Open, Tab::Files, &file("a"), "") else {
            panic!()
        };
        assert!(msg.starts_with("xdg-open failed (exit 4)"), "{msg}");
    }

    #[test]
    fn missing_program_is_reported() {
        let mut f = fixture("act_missing", false, false);
        f.env.path = f.root.clone().into_os_string();
        let Outcome::Failed(msg) = act(&f, ActionKind::Copy, Tab::Files, &file("a"), "") else {
            panic!()
        };
        assert!(msg.starts_with("wl-copy: "), "{msg}");
    }

    #[test]
    fn copy_sends_relative_path_on_stdin() {
        let f = fixture("act_copy", false, false);
        let out = act(&f, ActionKind::Copy, Tab::Files, &file("src/a b.txt"), "");
        assert_eq!(out, Outcome::Done(Some("Copied to the clipboard".into())));
        assert_eq!(
            std::fs::read_to_string(f.bin.join("wl-copy.stdin")).unwrap(),
            "src/a b.txt"
        );
        assert_eq!(calls(&f.bin, "wl-copy"), [Vec::<String>::new()]);
    }

    #[test]
    fn reveal_runs_open_in_file_manager() {
        let f = fixture("act_reveal", false, false);
        assert_eq!(
            act(&f, ActionKind::Reveal, Tab::Folders, &file("src"), ""),
            Outcome::Done(None)
        );
        assert_eq!(
            calls(&f.env.sub, "open_in_file_manager"),
            [[f.root.join("src").to_str().unwrap()]]
        );
    }

    #[test]
    fn wsl_uses_windows_action() {
        let f = fixture("act_wsl", true, false);
        let abs = f.root.join("a.txt").to_str().unwrap().to_string();
        act(&f, ActionKind::Open, Tab::Files, &file("a.txt"), "");
        act(&f, ActionKind::Reveal, Tab::Files, &file("a.txt"), "");
        assert_eq!(
            act(&f, ActionKind::Copy, Tab::Files, &file("a.txt"), ""),
            Outcome::Done(Some("Copied to the clipboard".into()))
        );
        let hash = "abcdef1";
        act(&f, ActionKind::Copy, Tab::Git, &git_item(hash), "");
        assert_eq!(
            calls(&f.env.sub, "windows_action"),
            [
                vec!["open".to_string(), abs.clone()],
                vec!["reveal".to_string(), abs.clone()],
                vec!["copy-path".to_string(), abs],
                vec!["copy-text".to_string(), hash.to_string()]
            ]
        );
        assert!(calls(&f.bin, "xdg-open").is_empty() && calls(&f.bin, "wl-copy").is_empty());
    }

    #[test]
    fn editor_outside_tmux_is_foreground_with_line() {
        let f = fixture("act_editor_fg", false, false);
        let item = parse(ParseKind::SearchLines, b"src/a.c\x0012\x1ftext", 0).unwrap();
        let out = act(&f, ActionKind::Editor, Tab::Content, &item, "text");
        let path = f.root.join("src/a.c");
        assert_eq!(
            out,
            Outcome::Foreground(vec![Cmd::new("nvim").arg("+12").arg("--").arg(&path)])
        );
    }

    #[test]
    fn editor_with_a_search_pattern_gets_it_through_the_environment() {
        let f = fixture("act_editor_query", false, false);
        let out = act(
            &f,
            ActionKind::Editor,
            Tab::Content,
            &file("a.txt"),
            "x/|!rm",
        );
        let Outcome::Foreground(cmds) = out else {
            panic!("{out:?}")
        };
        let cmd = &cmds[0];
        assert!(
            cmd.args.iter().all(|a| !a.to_string_lossy().contains("rm")),
            "{:?}",
            cmd.args
        );
        assert!(cmd.args.contains(&"let @/ = $SF_QUERY".into()));
        assert_eq!(
            cmd.env,
            vec![("SF_QUERY".to_string(), "\\c\\vx/|!rm".to_string())]
        );
        let out = act(
            &f,
            ActionKind::Editor,
            Tab::Content,
            &file("a.txt"),
            "Upper",
        );
        let Outcome::Foreground(cmds) = out else {
            panic!()
        };
        assert_eq!(
            cmds[0].env,
            vec![("SF_QUERY".to_string(), "\\vUpper".to_string())],
            "smart case"
        );
    }

    #[test]
    fn editor_in_files_has_no_pattern() {
        let f = fixture("act_editor_files", false, false);
        let out = act(&f, ActionKind::Editor, Tab::Files, &file("a.txt"), "a.t");
        assert_eq!(
            out,
            Outcome::Foreground(vec![Cmd::new("nvim").arg("--").arg(f.root.join("a.txt"))])
        );
    }

    #[test]
    fn editor_in_tmux_opens_window() {
        let f = fixture("act_editor_tmux", false, true);
        let item = parse(ParseKind::SearchLines, b"a.c\x007\x1fx", 0).unwrap();
        assert_eq!(
            act(&f, ActionKind::Editor, Tab::Content, &item, "x"),
            Outcome::Done(None)
        );
        let root = f.root.to_str().unwrap();
        let path = f.root.join("a.c");
        assert_eq!(
            calls(&f.bin, "tmux"),
            [
                vec![
                    "new-window",
                    "-n",
                    "nvim",
                    "sh",
                    "-c",
                    "cd -- \"$1\" && shift && exec \"$@\"",
                    "sf",
                    root,
                    "nvim",
                    "+7",
                    "--",
                    path.to_str().unwrap()
                ],
                vec!["select-pane", "-T", "editor"]
            ]
        );
    }

    #[test]
    fn tmux_never_gets_the_folder_as_an_option() {
        // tmux expands formats in `new-window -c DIR`: a folder named `#(cmd)` would run cmd
        // (verified with tmux 3.5a); program arguments are not expanded
        let mut f = fixture("act_tmux_format", false, true);
        f.root = f.root.join("a#(touch PWNED)b");
        std::fs::create_dir_all(&f.root).unwrap();
        act(&f, ActionKind::Log, Tab::Files, &file("x"), "");
        let first = &calls(&f.bin, "tmux")[0];
        let at = first.iter().position(|a| a == "sf").unwrap();
        assert_eq!(
            first[at + 1],
            f.root.to_str().unwrap(),
            "the folder is an argument of the wrapper"
        );
        assert_eq!(
            &first[..at],
            [
                "new-window",
                "-n",
                "logs",
                "sh",
                "-c",
                "cd -- \"$1\" && shift && exec \"$@\""
            ]
        );
    }

    #[test]
    fn tmux_window_with_env_passes_it_with_e() {
        let f = fixture("act_tmux_env", false, true);
        act(&f, ActionKind::Editor, Tab::Content, &file("a"), "q");
        let first = &calls(&f.bin, "tmux")[0];
        let at = first.iter().position(|a| a == "-e").unwrap();
        assert_eq!(first[at + 1], "SF_QUERY=\\c\\vq");
    }

    #[test]
    fn tmux_failure_is_reported() {
        let f = fixture("act_tmux_fail", false, true);
        std::fs::write(f.bin.join("tmux.exit"), "1").unwrap();
        assert!(matches!(
            act(&f, ActionKind::Log, Tab::Files, &file("a"), ""),
            Outcome::Failed(_)
        ));
    }

    #[test]
    fn infile_passes_query_in_search_only() {
        let f = fixture("act_infile", false, false);
        let script = f.env.sub.join("search_in_single_file");
        let path = f.root.join("a");
        let out = act(&f, ActionKind::InFile, Tab::Content, &file("a"), "nee dle");
        assert_eq!(
            out,
            Outcome::Foreground(vec![Cmd::new(&script).arg(&path).arg("nee dle")])
        );
        let out = act(&f, ActionKind::InFile, Tab::Files, &file("a"), "nee");
        assert_eq!(out, Outcome::Foreground(vec![Cmd::new(&script).arg(&path)]));
    }

    #[test]
    fn git_keys_on_a_file_use_the_sub_functions() {
        let f = fixture("act_git_file", false, false);
        let path = f.root.join("a");
        for (kind, name) in [
            (ActionKind::DiffSplit, "show_diffs"),
            (ActionKind::Diff, "git_diff"),
            (ActionKind::DiffShort, "git_diff_short"),
            (ActionKind::Log, "show_logs"),
        ] {
            let out = act(&f, kind, Tab::Files, &file("a"), "");
            assert_eq!(
                out,
                Outcome::Foreground(vec![Cmd::new(f.env.sub.join(name)).arg(&path)]),
                "{kind:?}"
            );
        }
    }

    fn git_item(hash: &str) -> Item {
        parse(ParseKind::Git, format!("* x\x1f{hash}").as_bytes(), 1).unwrap()
    }

    #[test]
    fn git_copy_is_hash() {
        let f = fixture("act_git_copy", false, false);
        let out = act(&f, ActionKind::Copy, Tab::Git, &git_item("abcdef1"), "");
        assert_eq!(out, Outcome::Done(Some("Copied to the clipboard".into())));
        assert_eq!(
            std::fs::read_to_string(f.bin.join("wl-copy.stdin")).unwrap(),
            "abcdef1"
        );
    }

    #[test]
    fn git_show_commit_is_paged_through_delta() {
        let f = fixture("act_git_show", false, true);
        let Outcome::Foreground(cmds) = act(
            &f,
            ActionKind::ShowCommit,
            Tab::Git,
            &git_item("abcdef1"),
            "",
        ) else {
            panic!()
        };
        assert_eq!(cmds.len(), 2);
        assert_eq!(
            cmds[0].args,
            ["show", "--color=always", "--end-of-options", "abcdef1"].map(OsString::from)
        );
        assert_eq!(cmds[1].program, PathBuf::from("/usr/bin/delta"));
        assert!(cmds[1].args.contains(&"--paging=always".into()));
    }

    #[test]
    fn git_other_keys_not_available() {
        let f = fixture("act_git_other", false, false);
        let out = act(&f, ActionKind::Editor, Tab::Git, &git_item("abcdef1"), "");
        assert_eq!(out, Outcome::Failed("Not available in Git".into()));
    }

    #[test]
    fn detect_reads_sub_functions_override() {
        // Env::detect reads the process environment; only the override is checked here.
        let env = Env::detect();
        assert!(
            env.sub.ends_with("sub-functions") || std::env::var_os("SF_SUBFUNCTIONS").is_some()
        );
    }
}
