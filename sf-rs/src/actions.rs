//! What the keys on a selected item do. Every program gets its arguments as a list (no shell); paths
//! come after `--`; the Search pattern reaches nvim through the environment, never as an Ex command.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::commands::{Cmd, delta_args, is_hash};
use crate::engines::Engines;
use crate::items::Item;
use crate::model::Tab;
use crate::platform::{Clipboard, Platform, launch};

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
    /// Linux or WSL, the session, tmux, `$PATH`.
    pub platform: Platform,
    /// The sub-functions folder (see `sub_functions`).
    pub sub: PathBuf,
}

impl Env {
    /// `bundle`: the release package's `lib/sf`, if sf runs from one.
    pub fn detect(bundle: Option<&Path>) -> Env {
        let home = dirs::home_dir();
        Env {
            platform: Platform::detect(),
            sub: sub_functions(std::env::var_os("SF_SUBFUNCTIONS"), bundle, home),
        }
    }
}

/// The sub-functions folder: `$SF_SUBFUNCTIONS`, else the bundle's, else `~/.SearchFunctions/sub-functions`.
fn sub_functions(var: Option<OsString>, bundle: Option<&Path>, home: Option<PathBuf>) -> PathBuf {
    var.filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            bundle
                .map(|b| b.join("sub-functions"))
                .filter(|s| s.is_dir())
        })
        .or_else(|| home.map(|h| h.join(".SearchFunctions/sub-functions")))
        .unwrap_or_else(|| PathBuf::from("sub-functions"))
}

/// `sh -c CD_AND_EXEC sf DIR PROGRAM ARGS...`: runs PROGRAM in DIR; the script itself holds no data.
const CD_AND_EXEC: &str = "cd -- \"$1\" && shift && exec \"$@\"";

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
    let platform = &env.platform;
    if tab == Tab::Git {
        let Some(hash) = item.key.to_str().filter(|h| is_hash(h)) else {
            return Outcome::Failed("Not a commit".into());
        };
        return match kind {
            ActionKind::ShowCommit | ActionKind::Open => show_commit(hash, e),
            ActionKind::Copy => copied(platform.copy_text(hash.as_bytes())),
            _ => Outcome::Failed("Not available in Git".into()),
        };
    }
    let path = root.join(&item.key);
    match kind {
        ActionKind::Open => done(platform.open(&path).map(|()| None)),
        ActionKind::Reveal => done(platform.reveal(&path)),
        // Linux: the path as listed (relative to the root); WSL: its Windows form
        ActionKind::Copy => copied(platform.copy_path(item.key.as_os_str(), &path)),
        ActionKind::ShowCommit => Outcome::Failed("Not available here".into()),
        _ => terminal(kind, tab, item, query, root, &path, env),
    }
}

fn done(result: Result<Option<String>, String>) -> Outcome {
    match result {
        Ok(note) => Outcome::Done(note),
        Err(e) => Outcome::Failed(e),
    }
}

/// What the status says after copying: how, when it was no clipboard program.
fn copied(result: Result<Clipboard, String>) -> Outcome {
    done(result.map(|how| Some(how.note().unwrap_or("Copied to the clipboard").to_string())))
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
    if !env.platform.tmux {
        return Outcome::Foreground(vec![cmd]);
    }
    // More than one argument after the options: tmux runs the program itself, no shell. The folder
    // is never given with `-c`, which tmux expands as a format (a folder named `#(cmd)` would run
    // cmd); a fixed script changes into it, the folder being one of its arguments.
    let mut args: Vec<OsString> = ["new-window", "-n", window].map(OsString::from).into();
    // the window gets sf's PATH (with a release package's engines), not the tmux server's
    let mut path = OsString::from("PATH=");
    path.push(&env.platform.path);
    args.extend(["-e".into(), path]);
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
    let path = &env.platform.path;
    let result = launch(path, tmux, &args, None, &[], true).and_then(|()| {
        let select = ["select-pane", "-T", title].map(std::ffi::OsStr::new);
        launch(path, tmux, &select, None, &[], true)
    });
    done(result.map(|()| None))
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
    use crate::platform::Platform;
    use crate::testutil::{calls, envs, stub, temp_dir};
    use std::ffi::OsStr;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

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
        for p in [
            "xdg-open",
            "wl-copy",
            "dbus-send",
            "tmux",
            "nvim",
            "powershell.exe",
            "wslpath",
        ] {
            stub(&bin, p);
        }
        std::fs::write(bin.join("wslpath.out"), "C:\\w\\a.txt\n").unwrap();
        for p in [
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
        let platform = Platform {
            wsl: wsl.then(|| "Ubuntu".to_string()),
            wayland: Some("wayland-0".into()),
            tmux,
            path,
            ..Platform::default()
        };
        Fixture {
            bin,
            env: Env { platform, sub },
            root,
        }
    }

    /// The item for `key`, which is created as a file under the fixture's root.
    fn existing(f: &Fixture, key: &[u8]) -> Item {
        let path = f.root.join(OsStr::from_bytes(key));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        parse(ParseKind::Files, key, 0).unwrap()
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
            act(
                &f,
                ActionKind::Open,
                Tab::Files,
                &existing(&f, key.as_bytes()),
                ""
            ),
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
        let item = existing(&f, b"caf\xe9.txt");
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
        let item = existing(&f, b"a");
        let Outcome::Failed(msg) = act(&f, ActionKind::Open, Tab::Files, &item, "") else {
            panic!()
        };
        assert!(msg.starts_with("xdg-open failed (exit 4)"), "{msg}");
    }

    #[test]
    fn missing_program_is_reported() {
        let mut f = fixture("act_missing", false, false);
        f.env.platform.path = f.root.clone().into_os_string();
        let item = existing(&f, b"a");
        let Outcome::Failed(msg) = act(&f, ActionKind::Open, Tab::Files, &item, "") else {
            panic!()
        };
        assert!(msg.starts_with("xdg-open is not installed"), "{msg}");
    }

    #[test]
    fn missing_file_is_reported() {
        let f = fixture("act_gone", false, false);
        let Outcome::Failed(msg) = act(&f, ActionKind::Open, Tab::Files, &file("gone"), "") else {
            panic!()
        };
        assert!(msg.starts_with("no such file or folder"), "{msg}");
        assert!(calls(&f.bin, "xdg-open").is_empty());
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
    fn reveal_asks_the_file_manager_over_dbus() {
        let f = fixture("act_reveal", false, false);
        std::fs::create_dir(f.root.join("src")).unwrap();
        let src = parse(ParseKind::Files, b"src", 0).unwrap();
        assert_eq!(
            act(&f, ActionKind::Reveal, Tab::Folders, &src, ""),
            Outcome::Done(None)
        );
        let calls = calls(&f.bin, "dbus-send");
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0].last().unwrap() == "string:"
                && calls[0].iter().any(|a| a.ends_with("/root%20dir/src")),
            "{calls:?}"
        );
    }

    #[test]
    fn reveal_fallback_says_so() {
        let f = fixture("act_reveal_fallback", false, false);
        std::fs::write(f.bin.join("dbus-send.exit"), "1").unwrap();
        let item = existing(&f, b"a.txt");
        let Outcome::Done(Some(note)) = act(&f, ActionKind::Reveal, Tab::Files, &item, "") else {
            panic!()
        };
        assert!(note.starts_with("Opened the folder"), "{note}");
        assert_eq!(calls(&f.bin, "xdg-open"), [[f.root.to_str().unwrap()]]);
    }

    #[test]
    fn wsl_goes_through_powershell() {
        let f = fixture("act_wsl", true, false);
        let item = existing(&f, b"a.txt");
        let abs = f.root.join("a.txt").to_str().unwrap().to_string();
        act(&f, ActionKind::Open, Tab::Files, &item, "");
        act(&f, ActionKind::Reveal, Tab::Files, &item, "");
        assert_eq!(
            act(&f, ActionKind::Copy, Tab::Files, &item, ""),
            Outcome::Done(Some("Copied to the clipboard".into()))
        );
        let hash = "abcdef1";
        act(&f, ActionKind::Copy, Tab::Git, &git_item(hash), "");
        assert_eq!(
            calls(&f.bin, "wslpath"),
            vec![vec!["-w".to_string(), abs]; 3]
        );
        let values: Vec<String> = envs(&f.bin, "powershell.exe")
            .into_iter()
            .map(|e| e[0].clone())
            .collect();
        let win = "SF_VALUE=C:\\w\\a.txt";
        assert_eq!(values, [win, win, win, "SF_VALUE=abcdef1"]);
        assert!(calls(&f.bin, "xdg-open").is_empty() && calls(&f.bin, "wl-copy").is_empty());
    }

    #[test]
    fn copy_through_tmux_says_how() {
        let mut f = fixture("act_copy_tmux", false, true);
        f.env.platform.wayland = None;
        let out = act(&f, ActionKind::Copy, Tab::Files, &file("a b"), "");
        assert_eq!(
            out,
            Outcome::Done(Some("Copied through tmux (OSC 52)".into()))
        );
        assert_eq!(calls(&f.bin, "tmux"), [["load-buffer", "-w", "-"]]);
        assert_eq!(std::fs::read(f.bin.join("tmux.stdin")).unwrap(), b"a b");
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
        let path_var = format!("PATH={}", f.env.platform.path.to_str().unwrap());
        assert_eq!(
            calls(&f.bin, "tmux"),
            [
                vec![
                    "new-window",
                    "-n",
                    "nvim",
                    "-e",
                    &path_var,
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
        let path_var = format!("PATH={}", f.env.platform.path.to_str().unwrap());
        assert_eq!(
            &first[..at],
            [
                "new-window",
                "-n",
                "logs",
                "-e",
                &path_var,
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
        let values: Vec<&String> = first
            .iter()
            .zip(&first[1..])
            .filter(|(flag, _)| *flag == "-e")
            .map(|(_, value)| value)
            .collect();
        assert_eq!(values.len(), 2, "{first:?}");
        assert!(values[0].starts_with("PATH="), "sf's PATH, with its bundle");
        assert_eq!(values[1], "SF_QUERY=\\c\\vq");
    }

    #[test]
    fn sub_functions_come_from_override_bundle_or_home() {
        let bundle = temp_dir("sub_bundle");
        let home = PathBuf::from("/home/u");
        let repo = PathBuf::from("/home/u/.SearchFunctions/sub-functions");
        assert_eq!(
            sub_functions(Some("/x".into()), Some(&bundle), Some(home.clone())),
            PathBuf::from("/x")
        );
        assert_eq!(
            sub_functions(Some("".into()), None, Some(home.clone())),
            repo,
            "empty: unset"
        );
        assert_eq!(
            sub_functions(None, Some(&bundle), Some(home.clone())),
            repo,
            "bundle without sub-functions"
        );
        std::fs::create_dir(bundle.join("sub-functions")).unwrap();
        assert_eq!(
            sub_functions(None, Some(&bundle), Some(home)),
            bundle.join("sub-functions")
        );
        assert_eq!(
            sub_functions(None, None, None),
            PathBuf::from("sub-functions")
        );
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
}
