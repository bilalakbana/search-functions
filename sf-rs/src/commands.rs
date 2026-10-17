//! The command lines for the lists and the previews. Pure: nothing runs here.
//!
//! Every value from the disk or from git goes into an argument of its own, after `--` or after the
//! option that takes it (`-e PATTERN`), never through a shell.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::engines::Engines;
use crate::items::Item;
use crate::model::{Opt, Options, Tab};

/// ugrep's colours in the previews: matches red, line numbers and context dim, file names blue.
pub const GREP_PALETTE: &str = "ln=38;2;128;135;162:cx=38;2;128;135;162:sl=:mt=1;38;2;237;135;150:fn=38;2;138;173;244:se=38;2;110;115;141";

/// In Matching lines ugrep writes `path NUL [{member} SEP] line SEP text`, every match in the text
/// between MATCH_START and MATCH_END (`--replace`). A path cannot hold a NUL, so the key is exact.
pub const SEP: u8 = 0x1f;
pub const MATCH_START: u8 = 0x01;
pub const MATCH_END: u8 = 0x02;

/// catppuccin/delta's Macchiato settings as delta options; git's own delta settings are left alone.
pub fn delta_args(theme: &str) -> Vec<OsString> {
    let fixed = [
        "--file-style=#cad3f5",
        "--file-decoration-style=#6e738d",
        "--hunk-header-decoration-style=#6e738d box ul",
        "--hunk-header-file-style=bold",
        "--hunk-header-line-number-style=bold #a5adcb",
        "--hunk-header-style=file line-number syntax",
        "--line-numbers-left-style=#6e738d",
        "--line-numbers-right-style=#6e738d",
        "--line-numbers-zero-style=#6e738d",
        "--line-numbers-minus-style=bold #ed8796",
        "--line-numbers-plus-style=bold #a6da95",
        "--minus-style=syntax #4c3a4c",
        "--minus-emph-style=bold syntax #6a485a",
        "--plus-style=syntax #3e4b4c",
        "--plus-emph-style=bold syntax #51655a",
        "--commit-decoration-style=#6e738d bold box ul",
    ];
    let mut args = vec![
        OsString::from("--dark"),
        format!("--syntax-theme={theme}").into(),
    ];
    args.extend(fixed.into_iter().map(OsString::from));
    args
}

/// The Git list: graph, short hash, author, refs, subject, age; the full hash after a 0x1f at the end.
const GIT_LOG_FORMAT: &str = "--format=format:%C(bold blue)%h%C(reset) %C(dim white)%an%C(reset)%C(bold yellow)%d%C(reset) \
     %C(white)%s%C(reset) %C(bold green)(%ar)%C(reset)%x1f%H";

/// The header of a commit in the Git preview.
pub const GIT_SHOW_FORMAT: &str = "--format=%C(magenta)<%an>%C(reset) %C(green)(%ar)%C(reset) %C(cyan)%ad%C(reset) - \
     %C(yellow)%h%C(reset) %C(white dim)<%ae>%C(reset)%n%C(white)%s%C(reset)%n";

#[derive(Clone, Debug, PartialEq)]
pub struct Cmd {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
}

impl Cmd {
    pub fn new(program: impl Into<PathBuf>) -> Cmd {
        Cmd {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, a: impl Into<OsString>) -> Cmd {
        self.args.push(a.into());
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: Into<OsString>>(mut self, args: I) -> Cmd {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Cmd {
        self.env.push((key.to_string(), value.to_string()));
        self
    }
}

/// How a list's output is cut into rows and parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseKind {
    Files,
    SearchFiles { archives: bool },
    SearchLines,
    Folders,
    Git,
}

#[derive(Debug, PartialEq)]
pub enum ListPlan {
    Run {
        cmd: Cmd,
        kind: ParseKind,
    },
    /// Nothing to run; the list shows this line.
    Hint(String),
}

/// Ctrl-T: the lines of one file (`key`, as listed) that `pattern` matches, every line while it is
/// empty, read as Matching lines are.
pub fn in_file_plan(key: &std::ffi::OsStr, pattern: &str, e: &Engines) -> ListPlan {
    let Some(ugrep) = &e.ugrep else {
        return ListPlan::Hint("ugrep is not installed".into());
    };
    // -H: the name even for one file, so the rows read as Content's matching lines
    let cmd = Cmd::new(ugrep)
        .args([
            "-nHU",
            "--smart-case",
            "--null",
            "--separator=\x1f",
            "--color=never",
        ])
        .args(["--replace=\x01%o\x02", "-e", pattern, "--"])
        .arg(key);
    ListPlan::Run {
        cmd,
        kind: ParseKind::SearchLines,
    }
}

/// The list's command as the window starts it (the tests' shorthand).
#[cfg(test)]
pub fn list_plan(tab: Tab, opts: &Options, query: &str, e: &Engines) -> ListPlan {
    list_plan_with(tab, opts, query, e, &[], false)
}

/// The list's command with `extra` options for the engine (from `sf TAB -- OPTIONS`); with `print`,
/// Content's command prints ugrep's own grep-style output for `sf --print` instead of sf's records.
pub fn list_plan_with(
    tab: Tab,
    opts: &Options,
    query: &str,
    e: &Engines,
    extra: &[OsString],
    print: bool,
) -> ListPlan {
    let hint = |h: &str| ListPlan::Hint(h.to_string());
    let hidden = opts.get(tab, Opt::Hidden);
    let ignored = opts.get(tab, Opt::Ignored);
    match tab {
        Tab::Files | Tab::Folders => {
            let Some(fd) = &e.fd else {
                return hint("fd is not installed");
            };
            let (kind, letter) = if tab == Tab::Files {
                (ParseKind::Files, "f")
            } else {
                (ParseKind::Folders, "d")
            };
            let mut cmd = Cmd::new(fd).args(["--type", letter, "--color=never", "-0"]);
            if hidden {
                cmd = cmd.arg("--hidden");
            }
            if ignored {
                cmd = cmd.arg("--no-ignore");
            }
            ListPlan::Run {
                cmd: cmd.args(["--exclude", ".git"]).args(extra),
                kind,
            }
        }
        Tab::Content => {
            // While the pattern is empty every file is listed (as ugrep-fzf does), always as a plain file
            // list: every line of every file, or every PDF converted, would be far too much.
            let (matching_lines, archives) = if query.is_empty() {
                (false, false)
            } else {
                (opts.content.matching_lines, opts.content.archives)
            };
            let program = if archives {
                let Some(p) = &e.ugrep_plus else {
                    return hint("ugrep+ is not installed (needed for Archives & documents)");
                };
                p
            } else {
                let Some(p) = &e.ugrep else {
                    return hint("ugrep is not installed");
                };
                p
            };
            // -s: no messages about unreadable files; what stays on stderr (a broken pattern) is shown.
            // sf reads NUL after a path, 0x1f after a line number and every match between 0x01 and 0x02;
            // printed (`sf --print`), ugrep's own output goes out, coloured on a terminal.
            let mode = match (matching_lines, archives) {
                (false, false) => &[
                    "-rlU",
                    "--sort=name",
                    "--smart-case",
                    "--ignore-binary",
                    "-s",
                ][..],
                (false, true) => &["-rzl", "--sort=name", "--smart-case", "-s"][..],
                (true, false) => &[
                    "-rnU",
                    "--sort=name",
                    "--smart-case",
                    "--ignore-binary",
                    "-s",
                ][..],
                (true, true) => &[
                    "-rzn",
                    "--sort=name",
                    "--smart-case",
                    "--ignore-binary",
                    "-s",
                ][..],
            };
            let mut cmd = Cmd::new(program).args(mode);
            if print {
                cmd = cmd.arg("--color=auto");
            } else {
                cmd = cmd.arg("--null");
                if matching_lines {
                    cmd = cmd.args(["--separator=\x1f", "--color=never", "--replace=\x01%o\x02"]);
                }
            }
            if archives {
                cmd = cmd.arg("--exclude=*.rar");
            }
            if hidden {
                cmd = cmd.arg("--hidden");
            }
            if !ignored {
                cmd = cmd.arg("--ignore-files");
            }
            let kind = if matching_lines {
                ParseKind::SearchLines
            } else {
                ParseKind::SearchFiles { archives }
            };
            ListPlan::Run {
                cmd: cmd
                    .args(extra)
                    .args(["--exclude-dir=.git", "-e", query, "."]),
                kind,
            }
        }
        Tab::Git => {
            let Some(git) = &e.git else {
                return hint("git is not installed");
            };
            let mut cmd = Cmd::new(git).arg("log");
            if opts.git.all_branches {
                cmd = cmd.arg("--all");
            }
            let cmd = cmd
                .args(["--graph", "--color=always", "--abbrev=7", GIT_LOG_FORMAT])
                .args(extra);
            ListPlan::Run {
                cmd,
                kind: ParseKind::Git,
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum PreviewPlan {
    Run(Cmd),
    /// The first command's output is the second one's input.
    Pipe(Cmd, Cmd),
    /// `text` for a text file (`None`: the file itself), `binary` for any other; `header` above it.
    Sniff {
        path: PathBuf,
        text: Option<Cmd>,
        binary: Binary,
        header: Option<Header>,
    },
    Text(String),
    Nothing,
}

/// The lines above a Content preview: the file, and with a pattern the pattern and its count.
#[derive(Debug, PartialEq)]
pub struct Header {
    pub file: String,
    pub matches: Option<Matches>,
}

/// How many times the pattern occurs: `ugrep -c -o` for a text file (`text`) or any other (`binary`).
#[derive(Debug, PartialEq)]
pub struct Matches {
    pub pattern: String,
    pub text: Cmd,
    pub binary: Cmd,
}

/// What a binary file's preview shows.
#[derive(Debug, PartialEq)]
pub enum Binary {
    /// "No text preview: " and the command's output (`file -b`); `None`: "binary file".
    Describe(Option<Cmd>),
    /// The command's output as it is (matches inside it, a document's text).
    Show(Cmd),
}

/// The preview as the window starts it (the tests' shorthand).
#[cfg(test)]
pub fn preview_plan(
    tab: Tab,
    opts: &Options,
    query: &str,
    item: &Item,
    width: u16,
    e: &Engines,
    root: &Path,
) -> PreviewPlan {
    preview_plan_with(tab, opts, query, item, width, e, root, &[])
}

/// The preview of `item`; Content's matches and their count take `extra`, the ugrep options from the
/// command line, as the list does.
#[allow(clippy::too_many_arguments)]
pub fn preview_plan_with(
    tab: Tab,
    opts: &Options,
    query: &str,
    item: &Item,
    width: u16,
    e: &Engines,
    root: &Path,
    extra: &[OsString],
) -> PreviewPlan {
    if !item.selectable {
        return PreviewPlan::Nothing;
    }
    let path = root.join(&item.key);
    let bat = |line: Option<u32>| {
        e.bat.as_ref().map(|bat| {
            // line numbers, no header or grid: the root is in the preview's border
            let mut cmd = Cmd::new(bat).args([
                "--color=always".to_string(),
                "--style=numbers".into(),
                format!("--theme={}", e.theme),
                format!("--terminal-width={width}"),
            ]);
            if let Some(n) = line {
                cmd = cmd.args(["--highlight-line".to_string(), n.to_string()]);
            }
            cmd.arg("--").arg(&path)
        })
    };
    let file_type = || {
        Binary::Describe(
            e.file
                .as_ref()
                .map(|f| Cmd::new(f).args(["-b", "--"]).arg(&path)),
        )
    };
    match tab {
        Tab::Files => PreviewPlan::Sniff {
            text: bat(None),
            binary: file_type(),
            header: None,
            path,
        },
        Tab::Content => {
            let s = &opts.content;
            let grep = if s.archives { &e.ugrep_plus } else { &e.ugrep };
            let count = |g: &PathBuf, opts: &[&str]| {
                Cmd::new(g)
                    .args(opts)
                    .args(extra)
                    .arg("-e")
                    .arg(query)
                    .arg("--")
                    .arg(&path)
            };
            let header = Some(Header {
                file: crate::items::printable(&String::from_utf8_lossy(
                    item.key.as_encoded_bytes(),
                )),
                matches: grep
                    .as_ref()
                    .filter(|_| !query.is_empty())
                    .map(|g| Matches {
                        pattern: query.to_string(),
                        text: count(g, &["-c", "-o", "-U", "--smart-case"]),
                        binary: count(g, &["-z", "-c", "-o", "-a", "--smart-case"]),
                    }),
            });
            if s.matching_lines {
                // documents and archives: their text as ugrep+ converts it
                let binary = match &e.ugrep_plus {
                    Some(plus) if s.archives => Binary::Show(
                        Cmd::new(plus)
                            .args(["-z", "-n", "--color=always", "-e", "", "--"])
                            .arg(&path)
                            .env("GREP_COLORS", GREP_PALETTE),
                    ),
                    _ => file_type(),
                };
                return PreviewPlan::Sniff {
                    text: bat(item.line),
                    binary,
                    header,
                    path,
                };
            }
            let Some(grep) = grep.as_ref().filter(|_| !query.is_empty()) else {
                return PreviewPlan::Sniff {
                    text: bat(None),
                    binary: file_type(),
                    header,
                    path,
                };
            };
            let matches = |opts: &[&str]| {
                Cmd::new(grep)
                    .args(opts)
                    .args(extra)
                    .arg("-e")
                    .arg(query)
                    .arg("--")
                    .arg(&path)
                    .env("GREP_COLORS", GREP_PALETTE)
            };
            PreviewPlan::Sniff {
                text: Some(matches(&[
                    "-nU",
                    "--smart-case",
                    "--color=always",
                    "-C",
                    "8",
                ])),
                // binaries, archives, documents: the matches with some bytes around them
                binary: Binary::Show(matches(&[
                    "-znao",
                    "--smart-case",
                    "--color=always",
                    "-B200",
                    "-A200",
                ])),
                header,
                path,
            }
        }
        Tab::Folders => match &e.ls {
            Some(ls) => {
                PreviewPlan::Run(Cmd::new(ls).args(["-A", "--color=always", "--"]).arg(&path))
            }
            None => PreviewPlan::Text("ls is not installed".into()),
        },
        Tab::Git => {
            let Some(hash) = item.key.to_str().filter(|h| is_hash(h)) else {
                return PreviewPlan::Nothing;
            };
            // the same as the commit's view, at the preview's width
            let commit = crate::views::ViewKind::Commit {
                root: root.to_path_buf(),
                hash: hash.to_string(),
            };
            crate::views::view_plan(&commit, width, e)
        }
    }
}

/// An abbreviated or full commit hash, the only thing a Git row's key may be.
pub fn is_hash(s: &str) -> bool {
    (7..=40).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engines() -> Engines {
        let p = |n: &str| Some(PathBuf::from(format!("/bin/{n}")));
        Engines {
            fd: p("fd"),
            bat: p("bat"),
            ugrep: p("ugrep"),
            ugrep_plus: p("ugrep+"),
            git: p("git"),
            delta: p("delta"),
            file: p("file"),
            ls: p("ls"),
            theme: "base16".into(),
        }
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn run(plan: ListPlan) -> (Cmd, ParseKind) {
        match plan {
            ListPlan::Run { cmd, kind } => (cmd, kind),
            ListPlan::Hint(h) => panic!("expected a command, got the hint {h:?}"),
        }
    }

    fn item(key: &str, line: Option<u32>) -> Item {
        let raw = match line {
            None => key.to_string(),
            Some(n) => format!("{key}\0{n}\x1fx"),
        };
        let kind = if line.is_some() {
            ParseKind::SearchLines
        } else {
            ParseKind::Files
        };
        crate::items::parse(kind, raw.as_bytes(), 0).unwrap()
    }

    #[test]
    fn files_cmd_with_defaults() {
        let (cmd, kind) = run(list_plan(Tab::Files, &Options::default(), "", &engines()));
        assert_eq!(cmd.program, PathBuf::from("/bin/fd"));
        assert_eq!(
            cmd.args,
            os(&[
                "--type",
                "f",
                "--color=never",
                "-0",
                "--no-ignore",
                "--exclude",
                ".git"
            ])
        );
        assert_eq!(kind, ParseKind::Files);
    }

    #[test]
    fn files_cmd_with_hidden_without_ignored() {
        let mut o = Options::default();
        (o.files.hidden, o.files.ignored) = (true, false);
        let (cmd, _) = run(list_plan(Tab::Files, &o, "", &engines()));
        assert_eq!(
            cmd.args,
            os(&[
                "--type",
                "f",
                "--color=never",
                "-0",
                "--hidden",
                "--exclude",
                ".git"
            ])
        );
    }

    #[test]
    fn missing_fd_is_hint() {
        let e = Engines {
            fd: None,
            ..engines()
        };
        assert_eq!(
            list_plan(Tab::Files, &Options::default(), "", &e),
            ListPlan::Hint("fd is not installed".into())
        );
        assert!(matches!(
            list_plan(Tab::Folders, &Options::default(), "", &e),
            ListPlan::Hint(_)
        ));
    }

    #[test]
    fn folders_cmd_with_defaults() {
        let (cmd, kind) = run(list_plan(Tab::Folders, &Options::default(), "", &engines()));
        assert_eq!(
            cmd.args,
            os(&[
                "--type",
                "d",
                "--color=never",
                "-0",
                "--no-ignore",
                "--exclude",
                ".git"
            ])
        );
        assert_eq!(kind, ParseKind::Folders);
    }

    #[test]
    fn search_empty_query_lists_all_files() {
        // as ugrep-fzf: `-e ''` matches every text file
        let want = os(&[
            "-rlU",
            "--sort=name",
            "--smart-case",
            "--ignore-binary",
            "-s",
            "--null",
            "--exclude-dir=.git",
            "-e",
            "",
            ".",
        ]);
        let (cmd, kind) = run(list_plan(Tab::Content, &Options::default(), "", &engines()));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(cmd.args, want);
        assert_eq!(kind, ParseKind::SearchFiles { archives: false });
        // also with Matching lines and Archives: every line / every converted PDF would be far too much
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::MatchingLines);
        o.toggle(Tab::Content, Opt::Archives);
        let (cmd, kind) = run(list_plan(Tab::Content, &o, "", &engines()));
        assert_eq!(
            (cmd.program, cmd.args, kind),
            (
                PathBuf::from("/bin/ugrep"),
                want,
                ParseKind::SearchFiles { archives: false }
            )
        );
    }

    #[test]
    fn search_empty_query_without_ugrep_is_hint() {
        let e = Engines {
            ugrep: None,
            ..engines()
        };
        let ListPlan::Hint(h) = list_plan(Tab::Content, &Options::default(), "", &e) else {
            panic!("expected a hint")
        };
        assert!(h.contains("ugrep is not installed"), "{h}");
    }

    #[test]
    fn search_files_cmd() {
        let (cmd, kind) = run(list_plan(
            Tab::Content,
            &Options::default(),
            "needle",
            &engines(),
        ));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(
            cmd.args,
            os(&[
                "-rlU",
                "--sort=name",
                "--smart-case",
                "--ignore-binary",
                "-s",
                "--null",
                "--exclude-dir=.git",
                "-e",
                "needle",
                "."
            ])
        );
        assert_eq!(kind, ParseKind::SearchFiles { archives: false });
    }

    #[test]
    fn search_hidden_and_ignored_flags() {
        let mut o = Options::default();
        (o.content.hidden, o.content.ignored) = (true, false);
        let (cmd, _) = run(list_plan(Tab::Content, &o, "x", &engines()));
        assert!(
            cmd.args.contains(&"--ignore-files".into()),
            "ignored off: .gitignore is honoured"
        );
        assert!(cmd.args.contains(&"--hidden".into()));
        (o.content.hidden, o.content.ignored) = (false, true);
        let (cmd, _) = run(list_plan(Tab::Content, &o, "x", &engines()));
        assert!(!cmd.args.contains(&"--ignore-files".into()));
        assert!(!cmd.args.contains(&"--hidden".into()));
    }

    #[test]
    fn search_archives_uses_ugrep_plus_z() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::Archives);
        let (cmd, kind) = run(list_plan(Tab::Content, &o, "needle", &engines()));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep+"));
        assert_eq!(
            cmd.args,
            os(&[
                "-rzl",
                "--sort=name",
                "--smart-case",
                "-s",
                "--null",
                "--exclude=*.rar",
                "--exclude-dir=.git",
                "-e",
                "needle",
                "."
            ])
        );
        assert_eq!(kind, ParseKind::SearchFiles { archives: true });
    }

    #[test]
    fn search_lines_key_ends_at_nul() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::MatchingLines);
        let (cmd, kind) = run(list_plan(Tab::Content, &o, "needle", &engines()));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(
            cmd.args,
            os(&[
                "-rnU",
                "--sort=name",
                "--smart-case",
                "--ignore-binary",
                "-s",
                "--null",
                "--separator=\x1f",
                "--color=never",
                "--replace=\x01%o\x02",
                "--exclude-dir=.git",
                "-e",
                "needle",
                "."
            ])
        );
        assert_eq!(kind, ParseKind::SearchLines);
    }

    #[test]
    fn search_lines_with_archives() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::MatchingLines);
        o.toggle(Tab::Content, Opt::Archives);
        let (cmd, kind) = run(list_plan(Tab::Content, &o, "needle", &engines()));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep+"));
        assert_eq!(&cmd.args[..2], &os(&["-rzn", "--sort=name"])[..]);
        assert!(cmd.args.contains(&"--replace=\x01%o\x02".into()));
        assert!(cmd.args.contains(&"--exclude=*.rar".into()));
        assert_eq!(kind, ParseKind::SearchLines);
    }

    #[test]
    fn extra_options_come_before_sf_own_positionals() {
        let o = Options::default();
        let extra = os(&["-e", "rs", "x"]);
        let (cmd, _) = run(list_plan_with(
            Tab::Files,
            &o,
            "",
            &engines(),
            &extra,
            false,
        ));
        assert_eq!(
            &cmd.args[cmd.args.len() - 3..],
            &extra[..],
            "fd: after sf's options"
        );
        let (cmd, _) = run(list_plan_with(
            Tab::Content,
            &o,
            "needle",
            &engines(),
            &os(&["-w"]),
            false,
        ));
        assert_eq!(
            &cmd.args[cmd.args.len() - 5..],
            &os(&["-w", "--exclude-dir=.git", "-e", "needle", "."])[..],
            "ugrep: before the pattern and the folder"
        );
        let (cmd, _) = run(list_plan_with(
            Tab::Git,
            &o,
            "",
            &engines(),
            &os(&["--author=b"]),
            false,
        ));
        assert_eq!(cmd.args.last(), Some(&OsString::from("--author=b")));
    }

    #[test]
    fn content_printed_is_ugreps_own_output() {
        let mut o = Options::default();
        let (cmd, kind) = run(list_plan_with(
            Tab::Content,
            &o,
            "needle",
            &engines(),
            &[],
            true,
        ));
        assert_eq!(kind, ParseKind::SearchFiles { archives: false });
        assert_eq!(
            cmd.args,
            os(&[
                "-rlU",
                "--sort=name",
                "--smart-case",
                "--ignore-binary",
                "-s",
                "--color=auto",
                "--exclude-dir=.git",
                "-e",
                "needle",
                "."
            ])
        );
        o.toggle(Tab::Content, Opt::MatchingLines);
        let (cmd, _) = run(list_plan_with(
            Tab::Content,
            &o,
            "needle",
            &engines(),
            &[],
            true,
        ));
        assert_eq!(&cmd.args[..2], &os(&["-rnU", "--sort=name"])[..]);
        for gone in [
            "--null",
            "--separator=\x1f",
            "--replace=\x01%o\x02",
            "--color=never",
        ] {
            assert!(!cmd.args.contains(&gone.into()), "{gone} in {:?}", cmd.args);
        }
        assert!(cmd.args.contains(&"--color=auto".into()));
    }

    #[test]
    fn content_preview_counts_and_shows_with_the_engine_options() {
        let plan = preview_plan_with(
            Tab::Content,
            &Options::default(),
            "needle",
            &item("a.c", None),
            80,
            &engines(),
            Path::new("/r"),
            &os(&["-w"]),
        );
        let PreviewPlan::Sniff {
            text: Some(text),
            header: Some(header),
            ..
        } = plan
        else {
            panic!("{plan:?}")
        };
        let before_e = |c: &Cmd| {
            let w = c.args.iter().position(|a| a == "-w");
            w.is_some() && w.map(|w| w + 1) == c.args.iter().position(|a| a == "-e")
        };
        assert!(before_e(&text), "{:?}", text.args);
        let count = header.matches.unwrap();
        assert!(before_e(&count.text) && before_e(&count.binary));
    }

    #[test]
    fn in_file_lists_the_lines_of_one_file() {
        let (cmd, kind) = run(in_file_plan("src/a b.c".as_ref(), "foo|bar", &engines()));
        assert_eq!(kind, ParseKind::SearchLines);
        assert_eq!(
            cmd.args,
            os(&[
                "-nHU",
                "--smart-case",
                "--null",
                "--separator=\x1f",
                "--color=never",
                "--replace=\x01%o\x02",
                "-e",
                "foo|bar",
                "--",
                "src/a b.c"
            ])
        );
        let none = Engines::default();
        assert_eq!(
            in_file_plan("a".as_ref(), "", &none),
            ListPlan::Hint("ugrep is not installed".into())
        );
    }

    #[test]
    fn query_starting_with_dash_follows_e() {
        let (cmd, _) = run(list_plan(
            Tab::Content,
            &Options::default(),
            "-rf",
            &engines(),
        ));
        let at = cmd.args.iter().position(|a| a == "-rf").unwrap();
        assert_eq!(cmd.args[at - 1], "-e");
    }

    #[test]
    fn missing_ugrep_plus_is_hint() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::Archives);
        let e = Engines {
            ugrep_plus: None,
            ..engines()
        };
        let ListPlan::Hint(h) = list_plan(Tab::Content, &o, "x", &e) else {
            panic!("expected a hint")
        };
        assert!(h.contains("ugrep+ is not installed"), "{h}");
    }

    #[test]
    fn missing_ugrep_is_hint() {
        let e = Engines {
            ugrep: None,
            ..engines()
        };
        let ListPlan::Hint(h) = list_plan(Tab::Content, &Options::default(), "x", &e) else {
            panic!("expected a hint")
        };
        assert!(h.contains("ugrep is not installed"), "{h}");
    }

    #[test]
    fn git_all_branches_flag() {
        let (cmd, kind) = run(list_plan(Tab::Git, &Options::default(), "", &engines()));
        assert_eq!(&cmd.args[..2], &os(&["log", "--all"])[..]);
        assert!(
            cmd.args
                .last()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with("%x1f%H")
        );
        assert_eq!(kind, ParseKind::Git);
        let mut o = Options::default();
        o.toggle(Tab::Git, Opt::AllBranches);
        let (cmd, _) = run(list_plan(Tab::Git, &o, "", &engines()));
        assert!(!cmd.args.contains(&"--all".into()));
    }

    #[test]
    fn preview_file_sniffs_with_bat_at_width() {
        let plan = preview_plan(
            Tab::Files,
            &Options::default(),
            "",
            &item("-a b", None),
            72,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Sniff {
            path, text, binary, ..
        } = plan
        else {
            panic!("{plan:?}")
        };
        assert_eq!(path, PathBuf::from("/r/-a b"));
        let text = text.unwrap();
        assert_eq!(text.program, PathBuf::from("/bin/bat"));
        assert_eq!(
            text.args,
            os(&[
                "--color=always",
                "--style=numbers",
                "--theme=base16",
                "--terminal-width=72",
                "--",
                "/r/-a b"
            ])
        );
        let Binary::Describe(Some(file)) = binary else {
            panic!("{binary:?}")
        };
        assert_eq!(file.args, os(&["-b", "--", "/r/-a b"]));
    }

    #[test]
    fn preview_without_bat_shows_file_raw() {
        let e = Engines {
            bat: None,
            ..engines()
        };
        let plan = preview_plan(
            Tab::Files,
            &Options::default(),
            "",
            &item("a", None),
            72,
            &e,
            Path::new("/r"),
        );
        assert!(
            matches!(
                plan,
                PreviewPlan::Sniff {
                    text: None,
                    binary: Binary::Describe(Some(_)),
                    ..
                }
            ),
            "{plan:?}"
        );
    }

    #[test]
    fn preview_search_shows_matches_with_context() {
        let plan = preview_plan(
            Tab::Content,
            &Options::default(),
            "ne",
            &item("a", None),
            72,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Sniff { text, .. } = plan else {
            panic!("{plan:?}")
        };
        let text = text.unwrap();
        assert_eq!(text.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(
            text.args,
            os(&[
                "-nU",
                "--smart-case",
                "--color=always",
                "-C",
                "8",
                "-e",
                "ne",
                "--",
                "/r/a"
            ])
        );
        assert_eq!(
            text.env,
            vec![("GREP_COLORS".to_string(), GREP_PALETTE.to_string())]
        );
    }

    #[test]
    fn preview_search_with_archives_uses_ugrep_plus_for_binaries() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::Archives);
        let plan = preview_plan(
            Tab::Content,
            &o,
            "ne",
            &item("a.zip", None),
            72,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Sniff {
            binary: Binary::Show(binary),
            ..
        } = plan
        else {
            panic!("{plan:?}")
        };
        assert_eq!(binary.program, PathBuf::from("/bin/ugrep+"));
        assert_eq!(
            binary.args,
            os(&[
                "-znao",
                "--smart-case",
                "--color=always",
                "-B200",
                "-A200",
                "-e",
                "ne",
                "--",
                "/r/a.zip"
            ])
        );
    }

    fn header_of(plan: PreviewPlan) -> Option<Header> {
        match plan {
            PreviewPlan::Sniff { header, .. } => header,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn preview_content_has_a_header_with_the_match_count() {
        let h = header_of(preview_plan(
            Tab::Content,
            &Options::default(),
            "ne",
            &item("src/a b", None),
            72,
            &engines(),
            Path::new("/r"),
        ));
        let h = h.expect("a header");
        assert_eq!(h.file, "src/a b");
        let m = h.matches.expect("the matches");
        assert_eq!(m.pattern, "ne");
        assert_eq!(m.text.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(
            m.text.args,
            os(&[
                "-c",
                "-o",
                "-U",
                "--smart-case",
                "-e",
                "ne",
                "--",
                "/r/src/a b"
            ])
        );
        assert_eq!(
            m.binary.args,
            os(&[
                "-z",
                "-c",
                "-o",
                "-a",
                "--smart-case",
                "-e",
                "ne",
                "--",
                "/r/src/a b"
            ])
        );
    }

    #[test]
    fn preview_content_header_counts_with_ugrep_plus_for_archives() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::Archives);
        o.toggle(Tab::Content, Opt::MatchingLines);
        let h = header_of(preview_plan(
            Tab::Content,
            &o,
            "ne",
            &item("a.zip", Some(3)),
            72,
            &engines(),
            Path::new("/r"),
        ));
        assert_eq!(
            h.unwrap().matches.unwrap().binary.program,
            PathBuf::from("/bin/ugrep+")
        );
    }

    #[test]
    fn preview_content_without_pattern_shows_only_the_file() {
        let h = header_of(preview_plan(
            Tab::Content,
            &Options::default(),
            "",
            &item("a\x1bb", None),
            72,
            &engines(),
            Path::new("/r"),
        ));
        let h = h.unwrap();
        assert_eq!(h.file, "a\u{fffd}b");
        assert!(h.matches.is_none());
    }

    #[test]
    fn preview_in_files_has_no_header() {
        let h = header_of(preview_plan(
            Tab::Files,
            &Options::default(),
            "ne",
            &item("a", None),
            72,
            &engines(),
            Path::new("/r"),
        ));
        assert!(h.is_none());
    }

    #[test]
    fn preview_matching_line_highlights_it() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::MatchingLines);
        let plan = preview_plan(
            Tab::Content,
            &o,
            "ne",
            &item("a", Some(42)),
            72,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Sniff { text, .. } = plan else {
            panic!("{plan:?}")
        };
        assert_eq!(
            text.unwrap().args,
            os(&[
                "--color=always",
                "--style=numbers",
                "--theme=base16",
                "--terminal-width=72",
                "--highlight-line",
                "42",
                "--",
                "/r/a"
            ])
        );
    }

    #[test]
    fn preview_folder_lists_entries() {
        let plan = preview_plan(
            Tab::Folders,
            &Options::default(),
            "",
            &item("..", None),
            72,
            &engines(),
            Path::new("/r"),
        );
        assert_eq!(
            plan,
            PreviewPlan::Run(Cmd::new("/bin/ls").args(["-A", "--color=always", "--", "/r/.."]))
        );
    }

    #[test]
    fn preview_git_pipes_show_into_delta() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let plan = preview_plan(
            Tab::Git,
            &Options::default(),
            "",
            &item(hash, None),
            90,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Pipe(show, delta) = plan else {
            panic!("{plan:?}")
        };
        assert_eq!(show.program, PathBuf::from("/bin/git"));
        assert_eq!(
            &show.args[..3],
            &os(&["-C", "/r", "show"])[..],
            "the root's repository"
        );
        assert_eq!(show.args.last().unwrap(), hash);
        assert_eq!(show.args[show.args.len() - 2], "--end-of-options");
        assert_eq!(delta.program, PathBuf::from("/bin/delta"));
        assert!(delta.args.contains(&"--width=90".into()));
    }

    #[test]
    fn preview_git_without_delta_runs_show() {
        let e = Engines {
            delta: None,
            ..engines()
        };
        let plan = preview_plan(
            Tab::Git,
            &Options::default(),
            "",
            &item("abcdef1", None),
            90,
            &e,
            Path::new("/r"),
        );
        assert!(matches!(plan, PreviewPlan::Run(_)), "{plan:?}");
    }

    #[test]
    fn preview_git_rejects_what_is_no_hash() {
        let plan = preview_plan(
            Tab::Git,
            &Options::default(),
            "",
            &item("--output=x", None),
            90,
            &engines(),
            Path::new("/r"),
        );
        assert_eq!(plan, PreviewPlan::Nothing);
    }

    #[test]
    fn preview_of_unselectable_row_is_nothing() {
        let mut it = item("a", None);
        it.selectable = false;
        assert_eq!(
            preview_plan(
                Tab::Files,
                &Options::default(),
                "",
                &it,
                72,
                &engines(),
                Path::new("/r")
            ),
            PreviewPlan::Nothing
        );
    }

    #[test]
    fn delta_args_carry_theme() {
        assert!(delta_args("Nord").contains(&"--syntax-theme=Nord".into()));
    }
}
