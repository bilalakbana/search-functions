//! The command lines for the lists and the previews. Pure: nothing runs here.
//!
//! Every value from the disk or from git goes into an argument of its own, after `--` or after the
//! option that takes it (`-e PATTERN`), never through a shell.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::engines::Engines;
use crate::find::{RowWords, words_of};
use crate::items::Item;
use crate::model::{InFileOpts, Opt, Options, Tab};

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
/// Each match's colours: the match in reverse video (`ESC[7m`), which `items` reads as its marks; nothing
/// else coloured, so the path, the line number and the text around it come out plain.
pub const EACH_MATCH_COLORS: &str = "ms=7:mc=7:sl=:cx=:fn=:ln=:bn=:cn=:se=";

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
    SearchFiles {
        archives: bool,
    },
    SearchLines,
    /// Each match with the text around it, as ugrep colours it (`EACH_MATCH_COLORS`).
    SearchMatches,
    Folders,
    Git,
    /// `sf grep-versions`' rows: `REV:PATH\0LINE\0TEXT`.
    Versions,
}

#[derive(Debug, PartialEq)]
pub enum ListPlan {
    Run {
        cmd: Cmd,
        kind: ParseKind,
        /// Only the rows with each of these words on their line or in their path (Content's and
        /// Deep's lists of matches).
        keep: Option<RowWords>,
    },
    /// Nothing to run; the list shows this line.
    Hint(String),
}

/// How Content or Deep searches for a pattern.
struct Search {
    matching_lines: bool,
    each_match: bool,
    /// Inside zip, tar, 7z... (`-z`).
    archives: bool,
    /// PDF and Office files as text: ugrep+ instead of ugrep.
    documents: bool,
    /// Binary files are searched too (no `--ignore-binary`).
    binaries: bool,
    /// How the pattern is read: smart case or the case as typed, a pattern or words.
    reading: Reading,
    /// Listing matches: ugrep finds the lines with any of the words, the rows kept have each of them on
    /// the line or in the path.
    words: Option<RowWords>,
}

/// How ugrep reads what was typed.
#[derive(Clone, Copy)]
struct Reading {
    match_case: bool,
    /// Words, every one on a line (`--bool`)...
    words_on_a_line: bool,
    /// ... or every one somewhere in the file (`--bool --files`).
    words_in_the_file: bool,
    /// Not inside a longer word (`-w`).
    whole_word: bool,
    /// The pattern is text as typed, not a regular expression (`ugrep_literal`).
    literal: bool,
}

/// `text` for ugrep as it is typed: every character its regular expressions or Boolean queries read
/// otherwise gets a backslash.
fn ugrep_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.^$|()[]{}*+?-\"".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Where a command takes the case and words options (`Reading::flags`).
const READING: &str = "--smart-case";

impl Reading {
    /// ugrep's options for it: `--smart-case` unless the case counts, a Boolean query for words.
    fn flags(self) -> Vec<&'static str> {
        let mut flags = Vec::new();
        if !self.match_case {
            flags.push("--smart-case");
        }
        if self.words_on_a_line || self.words_in_the_file {
            flags.push("--bool");
        }
        if self.words_in_the_file {
            flags.push("--files");
        }
        if self.whole_word {
            flags.push("-w");
        }
        flags
    }

    /// What ugrep gets after `-e`: the query, or its text as typed.
    fn pattern(self, query: &str) -> String {
        if self.literal {
            ugrep_literal(query)
        } else {
            query.to_string()
        }
    }

    /// `opts` with READING replaced by the flags.
    fn into(self, opts: &[&'static str]) -> Vec<&'static str> {
        opts.iter()
            .flat_map(|&o| if o == READING { self.flags() } else { vec![o] })
            .collect()
    }
}

impl Search {
    fn of(tab: Tab, opts: &Options, query: &str) -> Search {
        // While the pattern is empty every file is listed (as ugrep-fzf does), always as a plain file
        // list: every line of every file, or every PDF converted, would be far too much. Content lists
        // the text files, Deep every file.
        let pattern = !query.is_empty();
        let reading = |match_case, on_a_line, in_the_file, whole_word, regex: bool| Reading {
            match_case,
            words_on_a_line: pattern && on_a_line,
            words_in_the_file: pattern && in_the_file,
            whole_word: pattern && whole_word,
            literal: pattern && !regex,
        };
        // listing matches (not files): each word on the line or in the path, unless all in the file
        let row_words = |lists: bool, in_the_file: bool, regex, whole_word, match_case| {
            (pattern && lists && !in_the_file).then(|| RowWords {
                words: words_of(query),
                regex,
                whole_word,
                match_case,
            })
        };
        if tab == Tab::Deep {
            let d = &opts.deep;
            let words = row_words(
                d.each_match,
                d.words_in_the_file,
                d.regex,
                d.whole_word,
                d.match_case,
            );
            return Search {
                matching_lines: false,
                each_match: pattern && d.each_match,
                archives: pattern,
                documents: pattern && d.documents,
                binaries: true,
                reading: reading(
                    d.match_case,
                    d.words_on_a_line && words.is_none(),
                    d.words_in_the_file,
                    d.whole_word,
                    d.regex,
                ),
                words,
            };
        }
        let c = &opts.content;
        let (matching_lines, each_match, archives) = if pattern {
            (c.matching_lines, c.each_match, c.archives)
        } else {
            (false, false, false)
        };
        let words = row_words(
            matching_lines || each_match,
            c.words_in_the_file,
            c.regex,
            c.whole_word,
            c.match_case,
        );
        Search {
            matching_lines,
            each_match,
            archives,
            documents: archives,
            // with Archives & documents the file list has binaries too, the lists of matches not
            binaries: archives && !matching_lines && !each_match,
            reading: reading(
                c.match_case,
                c.words_on_a_line && words.is_none(),
                c.words_in_the_file,
                c.whole_word,
                c.regex,
            ),
            words,
        }
    }

    /// ugrep's `-e` options: each word (listing matches), else the query; as typed without Regular
    /// expression.
    fn patterns(&self, query: &str) -> Vec<String> {
        let typed = match &self.words {
            Some(w) => w.words.clone(),
            None => vec![query.to_string()],
        };
        typed
            .iter()
            .flat_map(|p| ["-e".to_string(), self.reading.pattern(p)])
            .collect()
    }
}

/// Git, Ctrl-Alt-F: `sf grep-versions` (`exe`, sf itself) as the list, over the `chosen` versions
/// (None: every one); a hint while nothing is typed or no version is chosen.
pub fn versions_plan(
    exe: &Path,
    query: &str,
    opts: &crate::model::VersionsOpts,
    chosen: Option<Vec<String>>,
) -> ListPlan {
    if query.is_empty() {
        return ListPlan::Hint(
            "type a pattern · searched in the files of every version (^S options, versions)".into(),
        );
    }
    let revs = match chosen {
        Some(revs) if revs.is_empty() => {
            return ListPlan::Hint("No version chosen: ^S, All versions".into());
        }
        Some(revs) => revs,
        None => Vec::new(),
    };
    let q = crate::versions::Query {
        pattern: query.to_string(),
        opts: opts.clone(),
        revs,
    };
    ListPlan::Run {
        cmd: Cmd::new(exe).args(crate::versions::args(&q)),
        kind: ParseKind::Versions,
        keep: None,
    }
}

/// Content's and Deep's lists of matches with a pattern: its words, each to be found on a row's line or
/// in its path; None when files are listed, all words in the file are asked for, or the tab has none.
pub fn row_words(tab: Tab, opts: &Options, query: &str) -> Option<RowWords> {
    if !tab.searches() {
        return None;
    }
    Search::of(tab, opts, query).words
}

/// Ctrl-T: the lines of one file (`key`, as listed) with the typed words (as `opts` say: all on a
/// line, all in the file, or any of them), every line while there is none; read as Matching lines are.
pub fn in_file_plan(
    key: &std::ffi::OsStr,
    query: &str,
    opts: &InFileOpts,
    e: &Engines,
) -> ListPlan {
    let Some(ugrep) = &e.ugrep else {
        return ListPlan::Hint("ugrep is not installed".into());
    };
    let words = !query.trim().is_empty();
    let reading = Reading {
        match_case: opts.match_case,
        words_on_a_line: words && opts.words_on_a_line,
        words_in_the_file: words && opts.words_in_the_file,
        whole_word: false,
        literal: false,
    };
    let pattern = in_file_pattern(query, opts);
    // -H: the name even for one file, so the rows read as Content's matching lines
    let cmd = Cmd::new(ugrep)
        .args(reading.into(&[
            "-nHU",
            READING,
            "--null",
            "--separator=\x1f",
            "--color=never",
        ]))
        .args(["--replace=\x01%o\x02", "-e", &pattern, "--"])
        .arg(key);
    ListPlan::Run {
        cmd,
        kind: ParseKind::SearchLines,
        keep: None,
    }
}

/// What ugrep gets in one file: the words as typed for a Boolean query, else any of them (`a|b`).
pub fn in_file_pattern(query: &str, opts: &InFileOpts) -> String {
    if opts.words_on_a_line || opts.words_in_the_file {
        query.to_string()
    } else {
        query.split_whitespace().collect::<Vec<_>>().join("|")
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
        Tab::Help => hint("Help is the manual, no list"),
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
                keep: None,
            }
        }
        Tab::Content | Tab::Deep => {
            let search = Search::of(tab, opts, query);
            let patterns = search.patterns(query);
            let Search {
                matching_lines,
                each_match,
                archives,
                documents,
                binaries,
                reading,
                words,
            } = search;
            let program = if documents {
                let Some(p) = &e.ugrep_plus else {
                    let option = if tab == Tab::Deep {
                        Opt::Documents
                    } else {
                        Opt::Archives
                    };
                    return hint(&format!(
                        "ugrep+ is not installed (needed for {})",
                        option.label(tab)
                    ));
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
            // Each match (detailedsearch): -o with 50 characters of the line on either side; --replace
            // would drop them, so the match is found by its colour instead (EACH_MATCH_COLORS).
            let first = match (matching_lines, each_match, archives) {
                (_, true, false) => "-rnoU",
                (_, true, true) => "-rzno",
                (true, _, false) => "-rnU",
                (true, _, true) => "-rzn",
                (false, _, false) => "-rlU",
                (false, _, true) => "-rzl",
            };
            let mut cmd = Cmd::new(program)
                .args([first, "--sort=name"])
                .args(reading.flags());
            if !binaries {
                cmd = cmd.arg("--ignore-binary");
            } else if each_match {
                cmd = cmd.arg("-a"); // a binary's matches as text, as detailedsearch
            }
            cmd = cmd.arg("-s");
            let around = ["-B50", "-A50"];
            if print {
                cmd = cmd.arg("--color=auto");
                if each_match {
                    cmd = cmd.args(around);
                }
            } else if each_match {
                cmd = cmd
                    .args(["--null", "--separator=\x1f", "--color=always"])
                    .args(around)
                    .env("GREP_COLORS", EACH_MATCH_COLORS);
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
            let kind = if each_match {
                ParseKind::SearchMatches
            } else if matching_lines {
                ParseKind::SearchLines
            } else {
                ParseKind::SearchFiles { archives }
            };
            ListPlan::Run {
                cmd: cmd
                    .args(extra)
                    .arg("--exclude-dir=.git")
                    .args(patterns)
                    .arg("."),
                kind,
                keep: words,
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
                keep: None,
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
    /// Content's matches as numbered blocks: `lines` (ugrep's records) with lines around each from
    /// a text file, `binary` (one match a line) for any other file.
    Numbered {
        path: PathBuf,
        lines: Cmd,
        binary: Cmd,
        header: Header,
    },
    Text(String),
    Nothing,
}

/// Content's and Deep's preview: with a pattern, the pattern and its count, drawn at the preview's top
/// right (`preview::Found`).
#[derive(Debug, PartialEq)]
pub struct Header {
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
        Tab::Help => PreviewPlan::Nothing,
        Tab::Files => PreviewPlan::Sniff {
            text: bat(None),
            binary: file_type(),
            header: None,
            path,
        },
        Tab::Content | Tab::Deep => {
            let s = Search::of(tab, opts, query);
            let grep = if s.documents { &e.ugrep_plus } else { &e.ugrep };
            let with_pattern = |g: &PathBuf, opts: &[&'static str]| {
                Cmd::new(g)
                    .args(s.reading.into(opts))
                    .args(extra)
                    .args(s.patterns(query))
                    .arg("--")
                    .arg(&path)
            };
            let header = Header {
                matches: grep
                    .as_ref()
                    .filter(|_| !query.is_empty())
                    .map(|g| Matches {
                        pattern: query.to_string(),
                        text: with_pattern(g, &["-c", "-o", "-U", "--smart-case"]),
                        binary: with_pattern(g, &["-z", "-c", "-o", "-a", "--smart-case"]),
                    }),
            };
            // binaries, archives, documents: each match with some bytes around it
            let matches = |g: &PathBuf| {
                with_pattern(
                    g,
                    &["-znao", "--smart-case", "--color=always", "-B200", "-A200"],
                )
                .env("GREP_COLORS", GREP_PALETTE)
            };
            if s.matching_lines || s.each_match {
                let binary = match (tab, grep) {
                    // Deep's matches in a binary are what its row shows
                    (Tab::Deep, Some(g)) => Binary::Show(matches(g)),
                    // documents and archives: their text as ugrep+ converts it
                    (Tab::Content, Some(plus)) if s.documents => Binary::Show(
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
                    header: Some(header),
                    path,
                };
            }
            let Some(grep) = grep.as_ref().filter(|_| !query.is_empty()) else {
                return PreviewPlan::Sniff {
                    text: bat(None),
                    binary: file_type(),
                    header: Some(header),
                    path,
                };
            };
            PreviewPlan::Numbered {
                // a text file's matching lines as the list reads them; the preview adds the lines around.
                // -a: what sf calls text is read as text, NUL bytes and all (else "Binary file matches")
                lines: with_pattern(
                    grep,
                    &[
                        "-nHUa",
                        "--smart-case",
                        "--null",
                        "--separator=\x1f",
                        "--color=never",
                        "--replace=\x01%o\x02",
                    ],
                ),
                binary: matches(grep),
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
            // Ctrl-Alt-F's rows: the file as it was in that version
            if let Some((rev, path)) = item.version() {
                let version = crate::views::ViewKind::Version {
                    root: root.to_path_buf(),
                    rev: rev.to_string(),
                    path: path.to_string(),
                    line: item.line.unwrap_or(1),
                };
                return crate::views::view_plan(&version, width, e);
            }
            let Some(hash) = item.key.to_str().filter(|h| is_hash(h)) else {
                return PreviewPlan::Nothing;
            };
            // the same as the commit's view, at the preview's width
            let commit = crate::views::ViewKind::Commit {
                root: root.to_path_buf(),
                hash: hash.to_string(),
                whole: opts.git.whole_files,
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
            ListPlan::Run { cmd, kind, .. } => (cmd, kind),
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
    fn deep_searches_binaries_and_archives() {
        // advancedsearch: no --ignore-binary, -z for archives, .rar left out
        let (cmd, kind) = run(list_plan(
            Tab::Deep,
            &Options::default(),
            "needle",
            &engines(),
        ));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep"));
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
    fn deep_documents_uses_ugrep_plus() {
        let mut o = Options::default();
        o.toggle(Tab::Deep, Opt::Documents);
        let (cmd, _) = run(list_plan(Tab::Deep, &o, "needle", &engines()));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep+"));
        assert_eq!(cmd.args[0], "-rzl");
        let e = Engines {
            ugrep_plus: None,
            ..engines()
        };
        let ListPlan::Hint(h) = list_plan(Tab::Deep, &o, "needle", &e) else {
            panic!("expected a hint")
        };
        assert!(h.contains("needed for Documents"), "{h}");
    }

    #[test]
    fn deep_each_match_reads_binaries_as_text() {
        // detailedsearch: every match, in binaries too (-a), with 50 columns around it
        let mut o = Options::default();
        o.toggle(Tab::Deep, Opt::EachMatch);
        let (cmd, kind) = run(list_plan(Tab::Deep, &o, "needle", &engines()));
        assert_eq!(
            cmd.args,
            os(&[
                "-rzno",
                "--sort=name",
                "--smart-case",
                "-a",
                "-s",
                "--null",
                "--separator=\x1f",
                "--color=always",
                "-B50",
                "-A50",
                "--exclude=*.rar",
                "--exclude-dir=.git",
                "-e",
                "needle",
                "."
            ])
        );
        assert_eq!(
            cmd.env,
            vec![("GREP_COLORS".to_string(), EACH_MATCH_COLORS.to_string())]
        );
        assert_eq!(kind, ParseKind::SearchMatches);
    }

    #[test]
    fn deep_empty_query_lists_every_file() {
        // binaries too, but no archive members: those are what a pattern finds
        let mut o = Options::default();
        o.toggle(Tab::Deep, Opt::EachMatch);
        o.toggle(Tab::Deep, Opt::Documents);
        let (cmd, kind) = run(list_plan(Tab::Deep, &o, "", &engines()));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(
            cmd.args,
            os(&[
                "-rlU",
                "--sort=name",
                "--smart-case",
                "-s",
                "--null",
                "--exclude-dir=.git",
                "-e",
                "",
                "."
            ])
        );
        assert_eq!(kind, ParseKind::SearchFiles { archives: false });
    }

    #[test]
    fn deep_hidden_and_ignored_are_its_own() {
        let mut o = Options::default();
        (o.deep.hidden, o.deep.ignored) = (true, false);
        let (cmd, _) = run(list_plan(Tab::Deep, &o, "x", &engines()));
        assert!(cmd.args.contains(&"--hidden".into()));
        assert!(cmd.args.contains(&"--ignore-files".into()));
        let (cmd, _) = run(list_plan(Tab::Content, &o, "x", &engines()));
        assert!(!cmd.args.contains(&"--hidden".into()));
        assert!(!cmd.args.contains(&"--ignore-files".into()));
    }

    fn has(cmd: &Cmd, arg: &str) -> bool {
        cmd.args.iter().any(|a| a == arg)
    }

    #[test]
    fn words_are_a_boolean_query_on_a_line_or_in_the_file() {
        for tab in [Tab::Content, Tab::Deep] {
            let mut o = Options::default();
            o.toggle(tab, Opt::WordsOnALine);
            let (cmd, _) = run(list_plan(tab, &o, "ankara izmir", &engines()));
            assert!(
                has(&cmd, "--bool") && !has(&cmd, "--files"),
                "{tab:?}: {:?}",
                cmd.args
            );
            let PreviewPlan::Numbered {
                lines,
                binary,
                header,
                ..
            } = preview_plan(
                tab,
                &o,
                "ankara izmir",
                &item("a.txt", None),
                72,
                &engines(),
                Path::new("/r"),
            )
            else {
                panic!("{tab:?}")
            };
            let count = header.matches.unwrap();
            for c in [&lines, &binary, &count.text, &count.binary] {
                assert!(
                    has(c, "--bool") && !has(c, "--files"),
                    "{tab:?}: {:?}",
                    c.args
                );
            }
            o.toggle(tab, Opt::WordsInTheFile);
            let (cmd, _) = run(list_plan(tab, &o, "ankara izmir", &engines()));
            assert!(
                has(&cmd, "--bool") && has(&cmd, "--files"),
                "{tab:?}: {:?}",
                cmd.args
            );
        }
    }

    #[test]
    fn whole_word_goes_to_the_list_the_count_and_the_matches() {
        for tab in [Tab::Content, Tab::Deep] {
            let mut o = Options::default();
            let (cmd, _) = run(list_plan(tab, &o, "config", &engines()));
            assert!(!has(&cmd, "-w"), "{tab:?}: off by default");
            o.toggle(tab, Opt::WholeWord);
            let (cmd, _) = run(list_plan(tab, &o, "config", &engines()));
            assert!(has(&cmd, "-w"), "{tab:?}: {:?}", cmd.args);
            let PreviewPlan::Numbered {
                lines,
                binary,
                header,
                ..
            } = preview_plan(
                tab,
                &o,
                "config",
                &item("a.txt", None),
                72,
                &engines(),
                Path::new("/r"),
            )
            else {
                panic!("{tab:?}")
            };
            let count = header.matches.unwrap();
            for c in [&lines, &binary, &count.text, &count.binary] {
                assert!(has(c, "-w"), "{tab:?}: {:?}", c.args);
            }
            let (cmd, _) = run(list_plan(tab, &o, "", &engines()));
            assert!(!has(&cmd, "-w"), "{tab:?}: no pattern, every file");
        }
    }

    #[test]
    fn without_regular_expression_the_pattern_is_searched_as_typed() {
        assert_eq!(
            ugrep_literal(r#"a.b(c)[d]{2}*+?|^$\-" x"#),
            r#"a\.b\(c\)\[d\]\{2\}\*\+\?\|\^\$\\\-\" x"#
        );
        for tab in [Tab::Content, Tab::Deep] {
            let mut o = Options::default();
            let pattern_of = |c: &Cmd| {
                let i = c.args.iter().position(|a| a == "-e").expect("-e");
                c.args[i + 1].clone()
            };
            let (cmd, _) = run(list_plan(tab, &o, "a.b(", &engines()));
            assert_eq!(
                pattern_of(&cmd),
                "a.b(",
                "{tab:?}: a regular expression by default"
            );
            o.toggle(tab, Opt::Regex);
            let (cmd, _) = run(list_plan(tab, &o, "a.b(", &engines()));
            assert_eq!(pattern_of(&cmd), r"a\.b\(", "{tab:?}");
            let PreviewPlan::Numbered {
                lines,
                binary,
                header,
                ..
            } = preview_plan(
                tab,
                &o,
                "a.b(",
                &item("a.txt", None),
                72,
                &engines(),
                Path::new("/r"),
            )
            else {
                panic!("{tab:?}")
            };
            let count = header.matches.unwrap();
            for c in [&lines, &binary, &count.text, &count.binary] {
                assert_eq!(pattern_of(c), r"a\.b\(", "{tab:?}: {:?}", c.args);
            }
            assert_eq!(count.pattern, "a.b(", "the header shows it as typed");
        }
    }

    /// What follows each `-e` of `cmd`.
    fn patterns_of(cmd: &Cmd) -> Vec<String> {
        let a: Vec<String> = cmd
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        a.windows(2)
            .filter(|w| w[0] == "-e")
            .map(|w| w[1].clone())
            .collect()
    }

    #[test]
    fn listing_matches_takes_each_word_on_the_line_or_in_the_path() {
        let query = r#"CODE "of x""#;
        for (tab, list) in [
            (Tab::Content, Opt::MatchingLines),
            (Tab::Content, Opt::EachMatch),
            (Tab::Deep, Opt::EachMatch),
        ] {
            let mut o = Options::default();
            o.toggle(tab, list);
            o.toggle(tab, Opt::WordsOnALine); // the words rule replaces it here
            let ListPlan::Run { cmd, keep, .. } = list_plan(tab, &o, query, &engines()) else {
                panic!("{tab:?}")
            };
            assert_eq!(
                patterns_of(&cmd),
                ["CODE", "of x"],
                "{tab:?}: any of the words"
            );
            assert!(!has(&cmd, "--bool"), "{tab:?}: {:?}", cmd.args);
            let want = RowWords {
                words: vec!["CODE".into(), "of x".into()],
                regex: true,
                whole_word: false,
                match_case: false,
            };
            assert_eq!(
                keep,
                Some(want),
                "{tab:?}: each word on the line or in the path"
            );
            let PreviewPlan::Sniff { header, binary, .. } = preview_plan(
                tab,
                &o,
                query,
                &item("a.txt", Some(3)),
                72,
                &engines(),
                Path::new("/r"),
            ) else {
                panic!("{tab:?}")
            };
            let count = header.unwrap().matches.unwrap();
            assert_eq!(
                patterns_of(&count.text),
                ["CODE", "of x"],
                "{tab:?}: the count"
            );
            if let Binary::Show(m) = binary {
                assert_eq!(
                    patterns_of(&m),
                    ["CODE", "of x"],
                    "{tab:?}: a binary's matches"
                );
            }
            o.toggle(tab, Opt::Regex);
            let ListPlan::Run { cmd, keep, .. } = list_plan(tab, &o, "a.b c", &engines()) else {
                panic!()
            };
            assert_eq!(
                patterns_of(&cmd),
                [r"a\.b", "c"],
                "{tab:?}: each word as typed"
            );
            assert!(!keep.unwrap().regex);
        }
        let mut o = Options::default();
        let ListPlan::Run { cmd, keep, .. } = list_plan(Tab::Content, &o, query, &engines()) else {
            panic!()
        };
        assert_eq!(
            (patterns_of(&cmd), keep),
            (vec![query.to_string()], None),
            "listing files"
        );
        o.toggle(Tab::Content, Opt::MatchingLines);
        o.toggle(Tab::Content, Opt::WordsInTheFile);
        let ListPlan::Run { cmd, keep, .. } = list_plan(Tab::Content, &o, query, &engines()) else {
            panic!()
        };
        assert!(
            has(&cmd, "--files") && keep.is_none(),
            "all words in the file: as before"
        );
    }

    #[test]
    fn a_versions_row_previews_the_file_as_it_was_in_that_version() {
        let rev = "0123456789abcdef0123456789abcdef01234567";
        let raw = format!("{rev}:src/a.c\x009\x00the needle");
        let it = crate::items::parse(ParseKind::Versions, raw.as_bytes(), 0).unwrap();
        let mut e = engines();
        e.bat = Some("/bin/bat".into());
        let plan = preview_plan(
            Tab::Git,
            &Options::default(),
            "",
            &it,
            72,
            &e,
            Path::new("/r"),
        );
        let PreviewPlan::Pipe(git, bat) = plan else {
            panic!("{plan:?}")
        };
        assert!(
            git.args.contains(&format!("{rev}:src/a.c").into()),
            "{:?}",
            git.args
        );
        assert!(
            bat.args.contains(&"--highlight-line=9".into()),
            "{:?}",
            bat.args
        );
    }

    #[test]
    fn match_case_drops_smart_case() {
        for tab in [Tab::Content, Tab::Deep] {
            let mut o = Options::default();
            let (cmd, _) = run(list_plan(tab, &o, "Needle", &engines()));
            assert!(has(&cmd, "--smart-case"), "{tab:?}");
            o.toggle(tab, Opt::MatchCase);
            let (cmd, _) = run(list_plan(tab, &o, "Needle", &engines()));
            assert!(!has(&cmd, "--smart-case"), "{tab:?}: {:?}", cmd.args);
            let PreviewPlan::Numbered { lines, header, .. } = preview_plan(
                tab,
                &o,
                "Needle",
                &item("a.txt", None),
                72,
                &engines(),
                Path::new("/r"),
            ) else {
                panic!("{tab:?}")
            };
            assert!(
                !has(&lines, "--smart-case") && !has(&header.matches.unwrap().text, "--smart-case")
            );
        }
    }

    #[test]
    fn in_a_file_the_words_follow_its_options() {
        let args = |o: &InFileOpts| {
            run(in_file_plan("f.c".as_ref(), "ankara izmir", o, &engines()))
                .0
                .args
        };
        let o = InFileOpts::default();
        let a = args(&o);
        assert!(
            a.contains(&"--bool".into()) && a.contains(&"ankara izmir".into()),
            "{a:?}"
        );
        let o = InFileOpts {
            words_on_a_line: false,
            words_in_the_file: true,
            match_case: true,
        };
        let a = args(&o);
        assert!(
            a.contains(&"--files".into()) && !a.contains(&"--smart-case".into()),
            "{a:?}"
        );
        let o = InFileOpts {
            words_on_a_line: false,
            ..InFileOpts::default()
        };
        let a = args(&o);
        assert!(
            !a.contains(&"--bool".into()) && a.contains(&"ankara|izmir".into()),
            "any of them: {a:?}"
        );
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
        let PreviewPlan::Numbered {
            lines,
            binary,
            header,
            ..
        } = plan
        else {
            panic!("{plan:?}")
        };
        let before_e = |c: &Cmd| {
            let w = c.args.iter().position(|a| a == "-w");
            w.is_some() && w.map(|w| w + 1) == c.args.iter().position(|a| a == "-e")
        };
        assert!(before_e(&lines), "{:?}", lines.args);
        assert!(before_e(&binary), "{:?}", binary.args);
        let count = header.matches.unwrap();
        assert!(before_e(&count.text) && before_e(&count.binary));
    }

    #[test]
    fn in_file_lists_the_lines_of_one_file() {
        let any = InFileOpts {
            words_on_a_line: false,
            ..InFileOpts::default()
        };
        let (cmd, kind) = run(in_file_plan(
            "src/a b.c".as_ref(),
            "foo bar",
            &any,
            &engines(),
        ));
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
            in_file_plan("a".as_ref(), "", &InFileOpts::default(), &none),
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
    fn preview_search_numbers_the_matches() {
        let plan = preview_plan(
            Tab::Content,
            &Options::default(),
            "ne",
            &item("a", None),
            72,
            &engines(),
            Path::new("/r"),
        );
        // the matching lines as sf reads them; the preview adds the lines around from the file. -a: a
        // file sf calls text is read as text, also when ugrep would see a NUL byte in it as binary
        // (it would print "Binary file … matches" instead of the lines)
        let PreviewPlan::Numbered { lines, path, .. } = plan else {
            panic!("{plan:?}")
        };
        assert_eq!(path, PathBuf::from("/r/a"));
        assert_eq!(lines.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(
            lines.args,
            os(&[
                "-nHUa",
                "--smart-case",
                "--null",
                "--separator=\x1f",
                "--color=never",
                "--replace=\x01%o\x02",
                "-e",
                "ne",
                "--",
                "/r/a"
            ])
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
        let PreviewPlan::Numbered { binary, .. } = plan else {
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
            PreviewPlan::Numbered { header, .. } => Some(header),
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
    fn each_match_lists_every_match_with_the_text_around_it() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::EachMatch);
        let (cmd, kind) = run(list_plan(Tab::Content, &o, "needle", &engines()));
        assert_eq!(kind, ParseKind::SearchMatches);
        assert_eq!(&cmd.args[..2], &os(&["-rnoU", "--sort=name"])[..]);
        for want in [
            "--null",
            "--separator=\x1f",
            "--color=always",
            "-B50",
            "-A50",
        ] {
            assert!(cmd.args.contains(&want.into()), "{want}: {:?}", cmd.args);
        }
        assert!(
            cmd.env
                .iter()
                .any(|(k, v)| k == "GREP_COLORS" && v.starts_with("ms=7:")),
            "{:?}",
            cmd.env
        );
        // into a pipe: ugrep's own lines
        let (cmd, _) = run(list_plan_with(
            Tab::Content,
            &o,
            "needle",
            &engines(),
            &[],
            true,
        ));
        assert!(cmd.args.contains(&"-B50".into()) && cmd.args.contains(&"--color=auto".into()));
        assert!(!cmd.args.contains(&"--null".into()) && cmd.env.is_empty());
        o.toggle(Tab::Content, Opt::Archives);
        let (cmd, _) = run(list_plan(Tab::Content, &o, "needle", &engines()));
        assert_eq!(cmd.program, PathBuf::from("/bin/ugrep+"));
        assert_eq!(&cmd.args[..1], &os(&["-rzno"])[..]);
    }

    #[test]
    fn preview_of_a_match_is_its_line() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::EachMatch);
        let plan = preview_plan(
            Tab::Content,
            &o,
            "ne",
            &item("a", Some(7)),
            72,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Sniff { text, .. } = plan else {
            panic!("{plan:?}")
        };
        assert!(text.unwrap().args.contains(&"--highlight-line".into()));
    }

    #[test]
    fn deep_preview_numbers_the_matches() {
        let deep = |o: &Options, key: &str, line: Option<u32>, query: &str| {
            preview_plan(
                Tab::Deep,
                o,
                query,
                &item(key, line),
                72,
                &engines(),
                Path::new("/r"),
            )
        };
        let mut o = Options::default();
        let plan = deep(&o, "a.dll", None, "ne");
        let PreviewPlan::Numbered {
            lines,
            binary,
            header,
            ..
        } = plan
        else {
            panic!("{plan:?}")
        };
        assert_eq!(lines.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(binary.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(binary.args[0], "-znao");
        assert!(header.matches.is_some());
        o.toggle(Tab::Deep, Opt::Documents);
        let plan = deep(&o, "a.pdf", None, "ne");
        let PreviewPlan::Numbered { binary, .. } = plan else {
            panic!("{plan:?}")
        };
        assert_eq!(binary.program, PathBuf::from("/bin/ugrep+"));
    }

    #[test]
    fn deep_preview_of_a_match_is_its_line_or_the_binary_matches() {
        let mut o = Options::default();
        o.toggle(Tab::Deep, Opt::EachMatch);
        let plan = preview_plan(
            Tab::Deep,
            &o,
            "ne",
            &item("a.dll", Some(7)),
            72,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Sniff {
            text,
            binary: Binary::Show(binary),
            ..
        } = plan
        else {
            panic!("{plan:?}")
        };
        assert!(text.unwrap().args.contains(&"--highlight-line".into()));
        assert_eq!(binary.program, PathBuf::from("/bin/ugrep"));
        assert_eq!(binary.args[0], "-znao");
    }

    #[test]
    fn deep_preview_without_a_pattern_is_the_file() {
        let plan = preview_plan(
            Tab::Deep,
            &Options::default(),
            "",
            &item("a.dll", None),
            72,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Sniff {
            binary: Binary::Describe(Some(file)),
            header: Some(header),
            ..
        } = plan
        else {
            panic!("{plan:?}")
        };
        assert_eq!(file.program, PathBuf::from("/bin/file"));
        assert_eq!(header.matches, None);
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
    fn preview_git_shows_whole_files_when_asked() {
        let mut o = Options::default();
        o.toggle(Tab::Git, Opt::WholeFiles);
        let plan = preview_plan(
            Tab::Git,
            &o,
            "",
            &item("abcdef1", None),
            90,
            &engines(),
            Path::new("/r"),
        );
        let PreviewPlan::Pipe(show, _) = plan else {
            panic!("{plan:?}")
        };
        assert!(
            show.args.contains(&"--unified=99999999".into()),
            "{:?}",
            show.args
        );
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
