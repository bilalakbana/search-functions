//! The full-screen views of sf: a file's diff against HEAD (Ctrl-G, Alt-G short), its history with
//! the changes (Ctrl-U), its log (Alt-U) and a commit (Enter in Git), drawn inside sf. They are made by
//! git (run in the file's folder with `-C`) and shown through delta, side by side for diffs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::commands::{Cmd, GIT_SHOW_FORMAT, GREP_PALETTE, PreviewPlan, delta_args};
use crate::engines::Engines;

/// The output of `git config -z --show-scope --name-only --get-regexp '^(filter|hook)\.'` in
/// `folder`: scope NUL key NUL, one pair per setting (exit 1: none). Err holds git's first error line:
/// a git before 2.26 (no --show-scope) or a config git cannot read; the diff view then runs no diff.
fn program_config(git: &Path, folder: &Path) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new(git)
        .args(crate::engines::GIT_RUNS_NO_PROGRAMS)
        .envs(crate::engines::GIT_ENV)
        .arg("-C")
        .arg(folder)
        .args([
            "config",
            "-z",
            "--show-scope",
            "--name-only",
            "--get-regexp",
            r"^(filter|hook)\.",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if matches!(out.status.code(), Some(0 | 1)) {
        return Ok(out.stdout);
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err
        .lines()
        .next()
        .unwrap_or("git config failed")
        .to_string())
}

/// `-c` settings that turn off the programs of the config that git diff HEAD would run: each content
/// filter (clean, process) of the repository's own config (local or worktree scope), which runs on
/// the working tree's file, and each hook defined in config (git 2.54 and later, any scope), which
/// `core.hooksPath` does not reach: writing the index runs `post-index-change`. The user's own
/// filters (git-lfs) stay. None when a name holds `=`, which `-c` cannot write.
fn programs_off(scoped: &[u8]) -> Option<Vec<OsString>> {
    use std::os::unix::ffi::OsStrExt;
    let fields: Vec<&[u8]> = scoped.split(|&b| b == 0).collect();
    let mut off: Vec<Vec<u8>> = Vec::new();
    for &[scope, key] in fields.as_chunks::<2>().0 {
        let (section, settings): (&[u8], &[&[u8]]) = if key.starts_with(b"hook.") {
            (b"hook.", &[b"enabled=false"])
        } else if key.starts_with(b"filter.") && matches!(scope, b"local" | b"worktree") {
            (b"filter.", &[b"clean=", b"process=", b"required=false"])
        } else {
            continue;
        };
        // the name is the subsection: from the section to the last dot (a name may hold dots)
        let Some(name) = key[section.len()..]
            .iter()
            .rposition(|&b| b == b'.')
            .map(|dot| &key[section.len()..section.len() + dot])
        else {
            continue;
        };
        if name.contains(&b'=') {
            return None;
        }
        for setting in settings {
            let value = [section, name, b".", setting].concat();
            if !off.contains(&value) {
                off.push(value);
            }
        }
    }
    Some(
        off.iter()
            .flat_map(|v| {
                [
                    OsString::from("-c"),
                    std::ffi::OsStr::from_bytes(v).to_owned(),
                ]
            })
            .collect(),
    )
}

/// What a view shows.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewKind {
    /// The working tree against HEAD: the whole file (`full`, Ctrl-G) or 10 lines around changes (Alt-G).
    Diff { path: PathBuf, full: bool },
    /// Every commit of the file with its changes (Ctrl-U); `whole`: the file whole in each (Git's
    /// Whole files).
    History { path: PathBuf, whole: bool },
    /// Every commit of the file, the files each changed (Alt-U).
    Log { path: PathBuf },
    /// One commit (Enter in Git); `whole`: the changed files whole.
    Commit {
        root: PathBuf,
        hash: String,
        whole: bool,
    },
    /// A file as it was in a version (Git, Ctrl-Shift-Alt-F, Enter): `line` is the one found.
    Version {
        root: PathBuf,
        rev: String,
        path: String,
        line: u32,
    },
}

impl ViewKind {
    /// The view's title, for its border.
    pub fn title(&self) -> String {
        let name = |p: &Path| {
            p.file_name()
                .unwrap_or(p.as_os_str())
                .to_string_lossy()
                .into_owned()
        };
        match self {
            ViewKind::Diff { path, full: true } => format!("{} · changes against HEAD", name(path)),
            ViewKind::Diff { path, full: false } => {
                format!("{} · changes against HEAD, short", name(path))
            }
            ViewKind::History { path, .. } => format!("{} · history with changes", name(path)),
            ViewKind::Log { path } => format!("{} · log", name(path)),
            ViewKind::Commit { hash, .. } => format!("commit {hash}"),
            ViewKind::Version { rev, path, .. } => {
                format!("{path} @ {}", rev.chars().take(7).collect::<String>())
            }
        }
    }

    /// What the view says when git printed nothing.
    pub fn empty_note(&self) -> String {
        match self {
            ViewKind::Diff { path, .. } => {
                // the diff does not look into submodules (--ignore-submodules=dirty)
                let sub = if path.is_dir() {
                    "; a submodule's own changes show in its folder"
                } else {
                    ""
                };
                format!(
                    "No local changes in {}, or git does not track it{sub}",
                    path.display()
                )
            }
            ViewKind::History { path, .. } | ViewKind::Log { path } => {
                format!("No commits of {}", path.display())
            }
            ViewKind::Commit { hash, .. } => format!("Nothing to show for {hash}"),
            ViewKind::Version { rev, path, .. } => format!("{path} is empty in {rev}"),
        }
    }
}

/// The folder git runs in and the path it is given: a file's folder and name, a folder itself and ".".
fn place(path: &Path) -> (PathBuf, OsString) {
    if path.is_dir() {
        return (path.to_path_buf(), ".".into());
    }
    let folder = path.parent().filter(|p| !p.as_os_str().is_empty());
    let name = path.file_name().map_or_else(|| ".".into(), OsString::from);
    (folder.unwrap_or(Path::new("/")).to_path_buf(), name)
}

/// After every git command that prints changes or a file: a repository's own config (a downloaded tree
/// with its .git/, a bare repository inside a clone) can name a diff program or a textconv program,
/// which git ran as soon as the Git tab showed a commit. sf shows git's own diff, through delta.
const NO_PROGRAMS: [&str; 2] = ["--no-ext-diff", "--no-textconv"];

/// The commands of a view `width` columns wide.
/// git's context for a whole file: more lines than any file has.
const WHOLE: &str = "--unified=99999999";

pub fn view_plan(kind: &ViewKind, width: u16, e: &Engines) -> PreviewPlan {
    let Some(git_path) = &e.git else {
        return PreviewPlan::Text("git is not installed".into());
    };
    let git = |folder: &Path| {
        Cmd::new(git_path)
            .args(crate::engines::GIT_RUNS_NO_PROGRAMS)
            .envs(&crate::engines::GIT_ENV)
            .arg("-C")
            .arg(folder)
    };
    let (cmd, side_by_side) = match kind {
        ViewKind::Diff { path, full } => {
            let (folder, name) = place(path);
            let context = if *full { WHOLE } else { "--unified=10" };
            let scoped = match program_config(git_path, &folder) {
                Ok(scoped) => scoped,
                Err(e) => {
                    let e = crate::items::printable(&e);
                    return PreviewPlan::Text(format!(
                        "sf cannot read the repository's filters and hooks, so it shows no diff: {e}"
                    ));
                }
            };
            let Some(off) = programs_off(&scoped) else {
                return PreviewPlan::Text(
                    "The repository's config names a filter or hook that sf cannot turn off".into(),
                );
            };
            // a submodule's own config (its filters, its hooks) would run in the git status that
            // git diff starts in it to see whether it holds changes; a new commit in it still shows
            let cmd = git(&folder)
                .args(off)
                .arg("diff")
                .args(NO_PROGRAMS)
                .args([
                    "--ignore-submodules=dirty",
                    "--color=always",
                    "HEAD",
                    context,
                    "--",
                ])
                .arg(name);
            (cmd, true)
        }
        ViewKind::History { path, whole } => {
            let (folder, name) = place(path);
            let mut cmd = git(&folder)
                .arg("log")
                .args(NO_PROGRAMS)
                .args(["--color=always", "-p"]);
            if *whole {
                cmd = cmd.arg(WHOLE);
            }
            (cmd.arg("--").arg(name), true)
        }
        ViewKind::Log { path } => {
            let (folder, name) = place(path);
            let rule = "─".repeat(width as usize);
            let format = format!(
                "--format=%C(dim)%n{rule}%C(reset)%n%C(red)<%an>%C(reset) %C(green)(%ar)%C(reset) \
                 %C(cyan)%ad%C(reset) - %C(yellow)%h%C(reset)%n%C(white)%s%C(reset)%n%C(magenta)%b%C(reset)"
            );
            let cmd = git(&folder)
                .args(["log", "--color=always", "--date=format:%Y-%m-%d %H:%M:%S"])
                .arg(format)
                .args(["--name-status", "--"])
                .arg(name);
            return PreviewPlan::Run(cmd); // already coloured, nothing for delta to do
        }
        ViewKind::Commit { root, hash, whole } => {
            let mut cmd = git(root).arg("show").args(NO_PROGRAMS).args([
                "--color=always",
                "--date=format:%Y-%m-%d %H:%M:%S",
                GIT_SHOW_FORMAT,
                "--patch-with-stat",
            ]);
            // the --stat bars fit the view (git's own width is the terminal's, or 80)
            cmd = cmd.arg(format!("--stat-width={width}"));
            if *whole {
                cmd = cmd.arg(WHOLE);
            }
            (cmd.args(["--end-of-options", hash]), false)
        }
        ViewKind::Version {
            root,
            rev,
            path,
            line,
        } => return version_plan(git(root), rev, path, *line, width, e),
    };
    let Some(delta_path) = &e.delta else {
        return PreviewPlan::Run(cmd);
    };
    let mut delta = Cmd::new(delta_path).args([
        "--line-numbers".to_string(),
        "--paging=never".into(),
        format!("--width={width}"),
    ]);
    if side_by_side {
        delta = delta.arg("--side-by-side");
    }
    PreviewPlan::Pipe(cmd, delta.args(delta_args(&e.theme)))
}

/// A file as it was in version `rev`: a text through bat, its line marked; a document or archive as
/// ugrep reads it (every line of its text), straight from git, never copied to disk.
fn version_plan(
    git: Cmd,
    rev: &str,
    path: &str,
    line: u32,
    width: u16,
    e: &Engines,
) -> PreviewPlan {
    let object = format!("{rev}:{path}");
    if let Some(doc) = crate::versions::doc_kind(path) {
        let Some(ugrep) = &e.ugrep else {
            return PreviewPlan::Text("ugrep is not installed".into());
        };
        let mut ug = Cmd::new(ugrep).args(["-n", "--color=always"]);
        ug = ug.arg(format!("--label={path}"));
        ug = match doc {
            crate::versions::Doc::Archive => ug.arg("-z"),
            crate::versions::Doc::Filtered => {
                let filters =
                    crate::versions::filters(&std::env::var_os("PATH").unwrap_or_default());
                let ug = ug.arg("-h");
                if filters.is_empty() {
                    ug
                } else {
                    ug.arg(format!("--filter={filters}"))
                }
            }
        };
        let ug = ug.args(["-e", ""]).env("GREP_COLORS", GREP_PALETTE);
        return PreviewPlan::Pipe(git.args(["cat-file", "blob"]).arg(object), ug);
    }
    let show = git.arg("show").args(NO_PROGRAMS).arg(object);
    let Some(bat) = &e.bat else {
        return PreviewPlan::Run(show);
    };
    let bat = Cmd::new(bat).args([
        "--color=always".to_string(),
        "--style=numbers".into(),
        format!("--theme={}", e.theme),
        format!("--terminal-width={width}"),
        format!("--file-name={path}"),
    ]);
    // line 0: the file itself (every file's history), no line to mark
    let bat = if line > 0 {
        bat.arg(format!("--highlight-line={line}"))
    } else {
        bat
    };
    PreviewPlan::Pipe(show, bat.arg("-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{calls, stub, temp_dir};

    fn engines() -> Engines {
        Engines {
            // the diff view asks git for the repository's filters: true answers "none"
            git: Some("/bin/true".into()),
            delta: Some("/bin/delta".into()),
            theme: "base16".into(),
            ..Engines::default()
        }
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// git | its viewer, git's arguments after the ones every git gets.
    fn pipe(plan: PreviewPlan) -> (Cmd, Cmd) {
        match plan {
            PreviewPlan::Pipe(git, delta) => (without_guard(git), delta),
            other => panic!("expected git | delta, got {other:?}"),
        }
    }

    /// `git` without its first arguments and its environment, which turn off the programs a
    /// repository's config names.
    fn without_guard(mut git: Cmd) -> Cmd {
        let n = crate::engines::GIT_RUNS_NO_PROGRAMS.len();
        assert_eq!(
            git.args[..n],
            crate::engines::GIT_RUNS_NO_PROGRAMS.map(OsString::from)
        );
        git.args.drain(..n);
        assert_eq!(
            git.env,
            crate::engines::GIT_ENV.map(|(k, v)| (k.to_string(), v.to_string()))
        );
        git.env.clear();
        git
    }

    fn version(path: &str) -> ViewKind {
        ViewKind::Version {
            root: "/r".into(),
            rev: "0123456789abcdef0123456789abcdef01234567".into(),
            path: path.into(),
            line: 7,
        }
    }

    #[test]
    fn a_file_in_a_version_comes_from_git_through_bat() {
        let e = Engines {
            bat: Some("/bin/bat".into()),
            ugrep: Some("/bin/ugrep".into()),
            ..engines()
        };
        let kind = version("src/a b.c");
        assert_eq!(kind.title(), "src/a b.c @ 0123456");
        let (git, bat) = pipe(view_plan(&kind, 80, &e));
        assert_eq!(
            git.args,
            os(&[
                "-C",
                "/r",
                "show",
                "--no-ext-diff",
                "--no-textconv",
                "0123456789abcdef0123456789abcdef01234567:src/a b.c"
            ])
        );
        assert_eq!(bat.program, PathBuf::from("/bin/bat"));
        assert_eq!(
            bat.args,
            os(&[
                "--color=always",
                "--style=numbers",
                "--theme=base16",
                "--terminal-width=80",
                "--file-name=src/a b.c",
                "--highlight-line=7",
                "-"
            ])
        );
        let (cat, ug) = pipe(view_plan(&version("doc/r.pdf"), 80, &e));
        assert_eq!(
            cat.args,
            os(&[
                "-C",
                "/r",
                "cat-file",
                "blob",
                "0123456789abcdef0123456789abcdef01234567:doc/r.pdf"
            ])
        );
        assert_eq!(ug.program, PathBuf::from("/bin/ugrep"));
        let a: Vec<String> = ug
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            a.contains(&"--label=doc/r.pdf".to_string())
                && a.ends_with(&["-e".to_string(), String::new()]),
            "a document's text, every line: {a:?}"
        );
    }

    #[test]
    fn a_file_without_a_line_marks_none() {
        let e = Engines {
            bat: Some("/bin/bat".into()),
            ..engines()
        };
        let kind = ViewKind::Version {
            root: "/r".into(),
            rev: "0123456789abcdef0123456789abcdef01234567".into(),
            path: "a.c".into(),
            line: 0,
        };
        let (_, bat) = pipe(view_plan(&kind, 80, &e));
        assert!(
            !bat.args
                .iter()
                .any(|a| a.to_string_lossy().starts_with("--highlight-line")),
            "{:?}",
            bat.args
        );
    }

    #[test]
    fn a_diff_runs_in_the_files_folder_side_by_side() {
        let kind = ViewKind::Diff {
            path: "/r/src/a b.c".into(),
            full: true,
        };
        let (git, delta) = pipe(view_plan(&kind, 120, &engines()));
        assert_eq!(
            git.args,
            os(&[
                "-C",
                "/r/src",
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--ignore-submodules=dirty",
                "--color=always",
                "HEAD",
                "--unified=99999999",
                "--",
                "a b.c"
            ])
        );
        for want in ["--side-by-side", "--paging=never", "--width=120"] {
            assert!(
                delta.args.contains(&want.into()),
                "{want}: {:?}",
                delta.args
            );
        }
        let short = ViewKind::Diff {
            path: "/r/src/a b.c".into(),
            full: false,
        };
        let (git, _) = pipe(view_plan(&short, 120, &engines()));
        assert!(git.args.contains(&"--unified=10".into()));
    }

    #[test]
    fn the_repositorys_own_filters_are_turned_off_in_a_diff() {
        let scoped = b"global\0filter.lfs.clean\0local\0filter.ev.clean\0local\0filter.ev.smudge\0\
                       worktree\0filter.a.b.process\0local\0hook.x.y.event\0local\0hook.x.y.command\0\
                       global\0hook.mine.command\0local\0hook.jobs\0";
        let args: Vec<String> = programs_off(scoped)
            .unwrap()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let off = |n: &str| {
            ["clean=", "process=", "required=false"]
                .map(|s| ["-c".to_string(), format!("filter.{n}.{s}")])
        };
        let hook = |n: &str| [["-c".to_string(), format!("hook.{n}.enabled=false")]];
        let want: Vec<String> = [&off("ev")[..], &off("a.b"), &hook("x.y"), &hook("mine")]
            .concat()
            .concat();
        assert_eq!(
            args, want,
            "git-lfs of the user's own config stays; every hook defined in config is off"
        );
        assert_eq!(programs_off(b"local\0filter.x=y.clean\0"), None);
        assert_eq!(programs_off(b"global\0hook.x=y.command\0"), None);
        assert_eq!(programs_off(b""), Some(Vec::new()));
    }

    #[test]
    fn no_diff_when_git_cannot_name_the_repositorys_filters() {
        let dir = temp_dir("view_old_git");
        stub(&dir, "git");
        let e = Engines {
            git: Some(dir.join("git")),
            ..engines()
        };
        let kind = ViewKind::Diff {
            path: dir.join("a.txt"),
            full: true,
        };
        // exit 1: no filter at all
        std::fs::write(dir.join("git.exit"), "1").unwrap();
        assert!(matches!(view_plan(&kind, 80, &e), PreviewPlan::Pipe(..)));
        // a git before 2.26 knows no --show-scope: the filters would run, so there is no diff
        std::fs::write(dir.join("git.exit"), "129").unwrap();
        std::fs::write(
            dir.join("git.err"),
            "error: unknown option `show-scope'\nusage\n",
        )
        .unwrap();
        let PreviewPlan::Text(t) = view_plan(&kind, 80, &e) else {
            panic!("expected a text")
        };
        assert!(t.ends_with("unknown option `show-scope'"), "{t}");
        let config = &calls(&dir, "git")[0];
        let n = crate::engines::GIT_RUNS_NO_PROGRAMS.len();
        assert_eq!(config[n + 2..n + 5], ["config", "-z", "--show-scope"]);
    }

    #[test]
    fn a_folder_is_diffed_from_inside() {
        let dir = temp_dir("view_folder");
        let kind = ViewKind::Diff {
            path: dir.clone(),
            full: false,
        };
        let (git, _) = pipe(view_plan(&kind, 80, &engines()));
        assert_eq!(
            &git.args[..2],
            &[OsString::from("-C"), dir.into_os_string()][..]
        );
        assert_eq!(git.args.last(), Some(&OsString::from(".")));
    }

    #[test]
    fn history_log_and_commit() {
        let path = PathBuf::from("/r/src/main.c");
        let (git, delta) = pipe(view_plan(
            &ViewKind::History {
                path: path.clone(),
                whole: false,
            },
            100,
            &engines(),
        ));
        assert_eq!(
            &git.args[..6],
            &os(&[
                "-C",
                "/r/src",
                "log",
                "--no-ext-diff",
                "--no-textconv",
                "--color=always"
            ])[..]
        );
        assert!(git.args.contains(&"-p".into()) && git.args.ends_with(&os(&["--", "main.c"])));
        assert!(delta.args.contains(&"--side-by-side".into()));

        let PreviewPlan::Run(log) = view_plan(&ViewKind::Log { path }, 100, &engines()) else {
            panic!("a log needs no delta")
        };
        assert!(
            log.args.contains(&"--name-status".into())
                && log.args.ends_with(&os(&["--", "main.c"]))
        );

        let commit = ViewKind::Commit {
            root: "/r".into(),
            hash: "abcdef1".into(),
            whole: false,
        };
        let (git, delta) = pipe(view_plan(&commit, 100, &engines()));
        assert_eq!(&git.args[..3], &os(&["-C", "/r", "show"])[..]);
        assert!(git.args.ends_with(&os(&["--end-of-options", "abcdef1"])));
        assert!(
            git.args.contains(&"--stat-width=100".into()),
            "the stat bars fit the view"
        );
        assert!(
            !delta.args.contains(&"--side-by-side".into()),
            "a commit reads top to bottom"
        );
    }

    #[test]
    fn whole_files_show_every_line_of_what_changed() {
        let whole = "--unified=99999999".into();
        let history = |whole| ViewKind::History {
            path: "/r/a.c".into(),
            whole,
        };
        let commit = |whole| ViewKind::Commit {
            root: "/r".into(),
            hash: "abcdef1".into(),
            whole,
        };
        for kind in [history(true), commit(true)] {
            let (git, _) = pipe(view_plan(&kind, 100, &engines()));
            assert!(git.args.contains(&whole), "{kind:?}: {:?}", git.args);
        }
        for kind in [history(false), commit(false)] {
            let (git, _) = pipe(view_plan(&kind, 100, &engines()));
            assert!(!git.args.contains(&whole), "{kind:?}: as git shows it");
        }
    }

    #[test]
    fn without_delta_git_colours_it_without_git_it_says_so() {
        let e = Engines {
            delta: None,
            ..engines()
        };
        let kind = ViewKind::Diff {
            path: "/r/a".into(),
            full: true,
        };
        assert!(matches!(view_plan(&kind, 80, &e), PreviewPlan::Run(_)));
        let none = Engines::default();
        assert_eq!(
            view_plan(&kind, 80, &none),
            PreviewPlan::Text("git is not installed".into())
        );
    }

    #[test]
    fn titles_and_empty_notes_name_the_file() {
        let diff = ViewKind::Diff {
            path: "/r/src/main.c".into(),
            full: true,
        };
        assert!(diff.title().contains("main.c"));
        assert!(
            diff.empty_note().contains("main.c"),
            "{}",
            diff.empty_note()
        );
        let commit = ViewKind::Commit {
            root: "/r".into(),
            hash: "abcdef1".into(),
            whole: false,
        };
        assert!(commit.title().contains("abcdef1"));
    }
}
