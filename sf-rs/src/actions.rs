//! What the keys on a selected item do. Every program gets its arguments as a list (no shell); paths
//! come after `--`; the Search pattern reaches nvim through the environment, never as an Ex command.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::commands::{Cmd, is_hash};
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
    /// Ctrl-T: search inside the file (sf narrows Content to it).
    InFile,
    /// Ctrl-U: the file's history with its changes (a view of sf).
    DiffSplit,
    /// Ctrl-G: the file against HEAD (a view of sf).
    Diff,
    /// Alt-G: the same, shorter (a view of sf).
    DiffShort,
    /// Alt-U: the file's log (a view of sf).
    Log,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Done; the message (if any) is shown for a moment.
    Done(Option<String>),
    /// A terminal program to run in the foreground (sf steps aside); a pipeline of one or more.
    Foreground(Vec<Cmd>),
    Failed(String),
}

/// The editor Ctrl-O opens.
#[derive(Clone, Debug, PartialEq)]
pub struct Editor {
    /// As found: a name in `$PATH` or `$EDITOR`'s first word.
    pub program: OsString,
    /// `$EDITOR`'s other words.
    pub args: Vec<OsString>,
    /// vi or vim: takes `+LINE`, `-c` commands and `--`.
    pub vim: bool,
}

/// nvim, else `$VISUAL`, `$EDITOR`, vim, vi: the first that is there.
pub fn find_editor(
    path_var: &std::ffi::OsStr,
    visual: Option<OsString>,
    editor: Option<OsString>,
) -> Option<Editor> {
    let there = |program: &std::ffi::OsStr| {
        let path = Path::new(program);
        if path.components().count() > 1 {
            path.is_file()
        } else {
            program
                .to_str()
                .is_some_and(|name| crate::engines::find(path_var, &[name]).is_some())
        }
    };
    let named = |name: &str| Some(OsString::from(name));
    [named("nvim"), visual, editor, named("vim"), named("vi")]
        .into_iter()
        .flatten()
        .find_map(|value| {
            // `$EDITOR` may hold words, as "code --wait"
            let value = value.to_str()?.to_string();
            let mut words = value.split_whitespace().map(OsString::from);
            let program = words.next()?;
            if !there(&program) {
                return None;
            }
            let name = Path::new(&program)
                .file_name()?
                .to_string_lossy()
                .into_owned();
            Some(Editor {
                vim: name == "vi" || name.contains("vim"),
                program,
                args: words.collect(),
            })
        })
}

/// Where sf runs.
#[derive(Clone, Debug)]
pub struct Env {
    /// Linux or WSL, the session, tmux, `$PATH`.
    pub platform: Platform,
    /// Ctrl-O's editor; None when there is none.
    pub editor: Option<Editor>,
    /// sf's own program, for Ctrl-T's tmux window (`sf in FILE`).
    pub sf: Option<PathBuf>,
    /// What sf in that window must see as this one does (`WINDOW_ENV`): tmux gives a new window its
    /// server's environment, not sf's.
    pub window_env: Vec<(String, String)>,
}

/// The variables sf reads: its config, bat's theme, the editor, the desktop session (open, reveal,
/// copy). Only these go to the window: its command line is visible to `ps`, the rest of the
/// environment may hold secrets.
const WINDOW_ENV: [&str; 8] = [
    "XDG_CONFIG_HOME",
    "BAT_THEME",
    "VISUAL",
    "EDITOR",
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
];

impl Env {
    pub fn detect() -> Env {
        let platform = Platform::detect();
        let editor = find_editor(
            &platform.path,
            std::env::var_os("VISUAL"),
            std::env::var_os("EDITOR"),
        );
        let window_env = WINDOW_ENV
            .iter()
            .filter_map(|&name| Some((name.to_string(), std::env::var(name).ok()?)))
            .collect();
        Env {
            platform,
            editor,
            sf: std::env::current_exe().ok(),
            window_env,
        }
    }
}

/// `sh -c CD_AND_EXEC sf DIR PROGRAM ARGS...`: runs PROGRAM in DIR; the script itself holds no data.
const CD_AND_EXEC: &str = "cd -- \"$1\" && shift && exec \"$@\"";

pub fn perform(
    kind: ActionKind,
    tab: Tab,
    item: &Item,
    query: &str,
    root: &Path,
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
        ActionKind::Editor => editor_window(tab, item, query, root, &path, env),
        ActionKind::InFile => in_file_window(item, query, root, env),
        // sf answers these itself (a view, Content narrowed to the file); here they mean nothing
        _ => Outcome::Failed("Not available here".into()),
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

/// Ctrl-O: the editor in a tmux window of its own, else in the foreground.
fn editor_window(
    tab: Tab,
    item: &Item,
    query: &str,
    root: &Path,
    path: &Path,
    env: &Env,
) -> Outcome {
    let Some(e) = &env.editor else {
        return Outcome::Failed("No editor: install nvim or set $EDITOR".into());
    };
    // the window is named after the editor, "nvim" as the fzf functions name it
    let window = Path::new(&e.program)
        .file_name()
        .unwrap_or(&e.program)
        .to_string_lossy()
        .into_owned();
    let cmd = editor(e, tab, item, query, path);
    if !env.platform.tmux {
        return Outcome::Foreground(vec![cmd]);
    }
    tmux_window(
        &window,
        "editor",
        root,
        &cmd.env,
        cmd.program.as_os_str(),
        &cmd.args,
        env,
    )
}

/// Ctrl-T in tmux (Bilal): sf inside the file (`sf in FILE WORDS`) in a window of its own, named
/// "infile" as the fzf functions named theirs.
fn in_file_window(item: &Item, query: &str, root: &Path, env: &Env) -> Outcome {
    if !env.platform.tmux {
        return Outcome::Failed("Not available here".into()); // sf narrows itself to the file
    }
    let Some(sf) = &env.sf else {
        return Outcome::Failed("sf cannot find its own program".into());
    };
    let mut args = vec![OsString::from("in"), item.key.as_os_str().to_os_string()];
    if !query.is_empty() {
        args.push(query.into()); // one word: the query as typed, spaces and quotes kept
    }
    tmux_window(
        "infile",
        "file-content",
        root,
        &env.window_env,
        sf.as_os_str(),
        &args,
        env,
    )
}

/// `program args` in a new tmux window named `window` (its pane titled `title`), in `root`, with sf's
/// `$PATH` and `vars`.
fn tmux_window(
    window: &str,
    title: &str,
    root: &Path,
    vars: &[(String, String)],
    program: &std::ffi::OsStr,
    program_args: &[OsString],
    env: &Env,
) -> Outcome {
    // More than one argument after the options: tmux runs the program itself, no shell. The folder
    // is never given with `-c`, which tmux expands as a format (a folder named `#(cmd)` would run
    // cmd); a fixed script changes into it, the folder being one of its arguments.
    let mut args: Vec<OsString> = ["new-window", "-n", window].map(OsString::from).into();
    // the window gets sf's PATH (with a release package's engines), not the tmux server's
    let mut path = OsString::from("PATH=");
    path.push(&env.platform.path);
    args.extend(["-e".into(), path]);
    for (k, v) in vars {
        args.push("-e".into()); // -e values are not expanded
        args.push(format!("{k}={v}").into());
    }
    args.extend(["sh", "-c", CD_AND_EXEC, "sf"].map(OsString::from));
    args.push(root.into());
    args.push(program.into());
    args.extend(program_args.iter().cloned());
    let args: Vec<&std::ffi::OsStr> = args.iter().map(OsString::as_os_str).collect();
    let tmux = std::ffi::OsStr::new("tmux");
    let path = &env.platform.path;
    let result = launch(path, tmux, &args, None, &[], true).and_then(|()| {
        let select = ["select-pane", "-T", title].map(std::ffi::OsStr::new);
        launch(path, tmux, &select, None, &[], true)
    });
    done(result.map(|()| None))
}

/// The editor at the matching line, or (vi and vim) searching for Content's or Deep's pattern. The pattern goes
/// in $SF_QUERY and vim reads it with `let @/ = $SF_QUERY`: as an Ex command (`-c "/pattern"`) a pattern
/// with `/|` would run whatever follows. Very magic (`\v`) is closest to ugrep's syntax; `\c` gives
/// smart case. Another editor gets only `+LINE` and the path, which is absolute and so never an option.
fn editor(e: &Editor, tab: Tab, item: &Item, query: &str, path: &Path) -> Cmd {
    let mut cmd = Cmd::new(&e.program).args(&e.args);
    if tab.searches() {
        if let Some(line) = item.line {
            cmd = cmd.arg(format!("+{line}"));
        } else if !query.is_empty() && e.vim {
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
    if e.vim {
        cmd = cmd.arg("--");
    }
    cmd.arg(path)
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
    use std::path::PathBuf;

    struct Fixture {
        bin: PathBuf,
        env: Env,
        root: PathBuf,
    }

    fn fixture(name: &str, wsl: bool, tmux: bool) -> Fixture {
        let base = temp_dir(name);
        let (bin, root) = (base.join("bin"), base.join("root dir"));
        for d in [&bin, &root] {
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
            env: Env {
                platform,
                editor: Some(Editor {
                    program: "nvim".into(),
                    args: Vec::new(),
                    vim: true,
                }),
                sf: Some("/opt/sf/bin/sf".into()),
                window_env: vec![("XDG_CONFIG_HOME".into(), "/tmp/e2e config".into())],
            },
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

    fn file(key: &str) -> Item {
        parse(ParseKind::Files, key.as_bytes(), 0).unwrap()
    }

    fn act(f: &Fixture, kind: ActionKind, tab: Tab, item: &Item, query: &str) -> Outcome {
        perform(kind, tab, item, query, &f.root, &f.env)
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
        let out = act(&f, ActionKind::Editor, Tab::Deep, &file("a.txt"), "needle");
        let Outcome::Foreground(cmds) = out else {
            panic!()
        };
        assert_eq!(
            cmds[0].env,
            vec![("SF_QUERY".to_string(), "\\c\\vneedle".to_string())],
            "Deep's pattern too"
        );
    }

    #[test]
    fn editor_is_nvim_then_visual_editor_vim_vi() {
        let bin = temp_dir("editor_find");
        let found = |path: &Path, visual: Option<&str>, editor: Option<&str>| {
            find_editor(
                path.as_os_str(),
                visual.map(Into::into),
                editor.map(Into::into),
            )
        };
        assert_eq!(found(&bin, None, None), None);
        for p in ["vi", "vim", "nano", "code"] {
            stub(&bin, p);
        }
        let vi = found(&bin, None, None).unwrap();
        assert_eq!((vi.program, vi.vim), ("vim".into(), true), "vim before vi");
        let nano = found(&bin, Some("missing-editor"), Some("nano")).unwrap();
        assert_eq!(
            (nano.program, nano.vim),
            ("nano".into(), false),
            "a missing $VISUAL is skipped"
        );
        let code = found(&bin, Some("code --wait"), Some("nano")).unwrap();
        assert_eq!(
            code.args,
            vec![OsString::from("--wait")],
            "$VISUAL with its words"
        );
        stub(&bin, "nvim");
        assert_eq!(
            found(&bin, Some("code"), None).unwrap().program,
            "nvim",
            "nvim first"
        );
    }

    #[test]
    fn another_editor_gets_only_the_line_and_the_path() {
        let mut f = fixture("act_editor_nano", false, false);
        f.env.editor = Some(Editor {
            program: "nano".into(),
            args: vec!["-l".into()],
            vim: false,
        });
        let item = parse(ParseKind::SearchLines, b"a.c\x0012\x1fneedle", 0).unwrap();
        let path = f.root.join("a.c");
        assert_eq!(
            act(&f, ActionKind::Editor, Tab::Content, &item, "needle"),
            Outcome::Foreground(vec![Cmd::new("nano").arg("-l").arg("+12").arg(&path)])
        );
        assert_eq!(
            act(&f, ActionKind::Editor, Tab::Content, &file("a.c"), "needle"),
            Outcome::Foreground(vec![Cmd::new("nano").arg("-l").arg(&path)]),
            "no vim commands for the pattern"
        );
        f.env.editor = None;
        let Outcome::Failed(msg) = act(&f, ActionKind::Editor, Tab::Files, &file("a.c"), "") else {
            panic!()
        };
        assert!(msg.contains("$EDITOR"), "{msg}");
    }

    #[test]
    fn the_keys_sf_answers_itself_are_no_actions_here() {
        // sf opens Ctrl-T, Ctrl-U, Ctrl-G, Alt-G and Alt-U itself; what reaches actions is a tab without them
        let f = fixture("act_not_here", false, false);
        for kind in [
            ActionKind::InFile,
            ActionKind::Diff,
            ActionKind::DiffShort,
            ActionKind::DiffSplit,
            ActionKind::Log,
        ] {
            assert_eq!(
                act(&f, kind, Tab::Folders, &file("src"), ""),
                Outcome::Failed("Not available here".into()),
                "{kind:?}"
            );
        }
        assert_eq!(
            act(&f, ActionKind::Open, Tab::Git, &git_item("abcdef1"), ""),
            Outcome::Failed("Not available in Git".into()),
            "sf shows a commit itself"
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
    fn in_file_in_tmux_runs_sf_in_a_window() {
        let f = fixture("act_in_file_tmux", false, true);
        assert_eq!(
            act(
                &f,
                ActionKind::InFile,
                Tab::Content,
                &file("src/a b.c"),
                "ankara izmir"
            ),
            Outcome::Done(None)
        );
        let root = f.root.to_str().unwrap();
        let path_var = format!("PATH={}", f.env.platform.path.to_str().unwrap());
        assert_eq!(
            calls(&f.bin, "tmux"),
            [
                vec![
                    "new-window",
                    "-n",
                    "infile",
                    "-e",
                    &path_var,
                    "-e",
                    "XDG_CONFIG_HOME=/tmp/e2e config",
                    "sh",
                    "-c",
                    "cd -- \"$1\" && shift && exec \"$@\"",
                    "sf",
                    root,
                    "/opt/sf/bin/sf",
                    "in",
                    "src/a b.c",
                    "ankara izmir"
                ],
                vec!["select-pane", "-T", "file-content"]
            ]
        );
        let f = fixture("act_in_file_no_words", false, true);
        act(&f, ActionKind::InFile, Tab::Files, &file("a.c"), "");
        assert_eq!(
            calls(&f.bin, "tmux")[0].last().map(String::as_str),
            Some("a.c"),
            "no empty word"
        );
    }

    #[test]
    fn tmux_never_gets_the_folder_as_an_option() {
        // tmux expands formats in `new-window -c DIR`: a folder named `#(cmd)` would run cmd
        // (verified with tmux 3.5a); program arguments are not expanded
        let mut f = fixture("act_tmux_format", false, true);
        f.root = f.root.join("a#(touch PWNED)b");
        std::fs::create_dir_all(&f.root).unwrap();
        act(&f, ActionKind::Editor, Tab::Files, &file("x"), "");
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
                "nvim",
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
    fn tmux_failure_is_reported() {
        let f = fixture("act_tmux_fail", false, true);
        std::fs::write(f.bin.join("tmux.exit"), "1").unwrap();
        assert!(matches!(
            act(&f, ActionKind::Editor, Tab::Files, &file("a"), ""),
            Outcome::Failed(_)
        ));
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
    fn git_other_keys_not_available() {
        let f = fixture("act_git_other", false, false);
        let out = act(&f, ActionKind::Editor, Tab::Git, &git_item("abcdef1"), "");
        assert_eq!(out, Outcome::Failed("Not available in Git".into()));
    }
}
