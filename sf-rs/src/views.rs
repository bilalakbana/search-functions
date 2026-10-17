//! The full-screen views of sf: a file's diff against HEAD (Ctrl-G, Alt-G short), its history with
//! the changes (Ctrl-U), its log (Alt-U) and a commit (Enter in Git), drawn inside sf. They are made by
//! git (run in the file's folder with `-C`) and shown through delta, side by side for diffs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::commands::{Cmd, GIT_SHOW_FORMAT, PreviewPlan, delta_args};
use crate::engines::Engines;

/// What a view shows.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewKind {
    /// The working tree against HEAD: the whole file (`full`, Ctrl-G) or 10 lines around changes (Alt-G).
    Diff { path: PathBuf, full: bool },
    /// Every commit of the file with its changes (Ctrl-U).
    History { path: PathBuf },
    /// Every commit of the file, the files each changed (Alt-U).
    Log { path: PathBuf },
    /// One commit (Enter in Git).
    Commit { root: PathBuf, hash: String },
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
            ViewKind::History { path } => format!("{} · history with changes", name(path)),
            ViewKind::Log { path } => format!("{} · log", name(path)),
            ViewKind::Commit { hash, .. } => format!("commit {hash}"),
        }
    }

    /// What the view says when git printed nothing.
    pub fn empty_note(&self) -> String {
        match self {
            ViewKind::Diff { path, .. } => format!(
                "No local changes in {}, or git does not track it",
                path.display()
            ),
            ViewKind::History { path } | ViewKind::Log { path } => {
                format!("No commits of {}", path.display())
            }
            ViewKind::Commit { hash, .. } => format!("Nothing to show for {hash}"),
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

/// The commands of a view `width` columns wide.
pub fn view_plan(kind: &ViewKind, width: u16, e: &Engines) -> PreviewPlan {
    let Some(git_path) = &e.git else {
        return PreviewPlan::Text("git is not installed".into());
    };
    let git = |folder: &Path| Cmd::new(git_path).arg("-C").arg(folder);
    let (cmd, side_by_side) = match kind {
        ViewKind::Diff { path, full } => {
            let (folder, name) = place(path);
            let context = if *full {
                "--unified=99999999"
            } else {
                "--unified=10"
            };
            let cmd = git(&folder)
                .args(["diff", "--color=always", "HEAD", context, "--"])
                .arg(name);
            (cmd, true)
        }
        ViewKind::History { path } => {
            let (folder, name) = place(path);
            let cmd = git(&folder)
                .args(["log", "--color=always", "-p", "--"])
                .arg(name);
            (cmd, true)
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
        ViewKind::Commit { root, hash } => {
            let cmd = git(root).args([
                "show",
                "--color=always",
                "--date=format:%Y-%m-%d %H:%M:%S",
                GIT_SHOW_FORMAT,
                "--patch-with-stat",
                "--end-of-options",
                hash,
            ]);
            (cmd, false)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    fn engines() -> Engines {
        Engines {
            git: Some("/bin/git".into()),
            delta: Some("/bin/delta".into()),
            theme: "base16".into(),
            ..Engines::default()
        }
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn pipe(plan: PreviewPlan) -> (Cmd, Cmd) {
        match plan {
            PreviewPlan::Pipe(git, delta) => (git, delta),
            other => panic!("expected git | delta, got {other:?}"),
        }
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
            &ViewKind::History { path: path.clone() },
            100,
            &engines(),
        ));
        assert_eq!(
            &git.args[..4],
            &os(&["-C", "/r/src", "log", "--color=always"])[..]
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
        };
        let (git, delta) = pipe(view_plan(&commit, 100, &engines()));
        assert_eq!(&git.args[..3], &os(&["-C", "/r", "show"])[..]);
        assert!(git.args.ends_with(&os(&["--end-of-options", "abcdef1"])));
        assert!(
            !delta.args.contains(&"--side-by-side".into()),
            "a commit reads top to bottom"
        );
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
        };
        assert!(commit.title().contains("abcdef1"));
    }
}
