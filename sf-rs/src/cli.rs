//! sf's command line: `sf [TAB] [QUERY...] [--print] [-- ENGINE-OPTIONS...]`, `sf in FILE [WORDS...]`,
//! the one-action commands and `sf doctor`. Engine options go to the starting tab's engine (fd, ugrep, git); those that change what
//! the engine prints, which sf reads, are refused.

use std::ffi::OsString;

use crate::model::Tab;
use crate::platform::COMMANDS;

/// The views `sf history|log|diff [--short] FILE` start with (Ctrl-U, Alt-U, Ctrl-G, Alt-G in tmux).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Diff,
    ShortDiff,
    History,
    Log,
}

#[derive(Debug, PartialEq)]
pub enum Cli {
    Help,
    Doctor,
    /// `sf open|reveal|copy-path PATH`, `sf copy-text TEXT`.
    Command {
        name: String,
        value: OsString,
    },
    /// `sf in FILE [WORDS...]`: the window inside one file (Ctrl-T); Esc quits it.
    InFile {
        file: OsString,
        query: String,
    },
    /// `sf history|log|diff [--short] FILE`: a view of the file's git; Esc quits it.
    View {
        view: View,
        file: OsString,
    },
    /// `sf grep-versions …`: the Git tab's search in every version (Ctrl-Shift-Alt-F), run as its list.
    GrepVersions(crate::versions::Query),
    /// The window, or with `print` the list it would show.
    Run {
        tab: Tab,
        query: String,
        print: bool,
        extra: Vec<OsString>,
    },
}

/// The arguments after `sf`; an error is said with the usage.
pub fn parse(args: &[OsString]) -> Result<Cli, String> {
    let first = args.first().and_then(|a| a.to_str());
    if first == Some("grep-versions") {
        return crate::versions::parse_args(&args[1..]).map(Cli::GrepVersions);
    }
    if let Some(name) = first.filter(|n| *n == "doctor" || COMMANDS.contains(n)) {
        if name == "doctor" {
            return if args.len() == 1 {
                Ok(Cli::Doctor)
            } else {
                Err("sf doctor takes no arguments".into())
            };
        }
        let what = if name == "copy-text" { "TEXT" } else { "PATH" };
        return match args {
            [_, value] => Ok(Cli::Command {
                name: name.into(),
                value: value.clone(),
            }),
            _ => Err(format!("sf {name}: one {what} expected")),
        };
    }
    if let Some(name) = first.filter(|n| ["history", "log", "diff"].contains(n)) {
        let rest = &args[1..];
        let (view, files) = match (name, rest.first().and_then(|a| a.to_str())) {
            ("diff", Some("--short")) => (View::ShortDiff, &rest[1..]),
            ("diff", _) => (View::Diff, rest),
            ("history", _) => (View::History, rest),
            _ => (View::Log, rest),
        };
        return match files {
            [file] => Ok(Cli::View {
                view,
                file: file.clone(),
            }),
            _ => Err(format!("sf {name}: one FILE expected")),
        };
    }
    if first == Some("in") {
        // the words go to the search as they are, a leading dash too (`-word`: not that word)
        let Some(file) = args.get(1) else {
            return Err("sf in: a FILE expected".into());
        };
        let mut words = Vec::new();
        for word in &args[2..] {
            let Some(word) = word.to_str() else {
                return Err("the query must be valid UTF-8".into());
            };
            words.push(word);
        }
        return Ok(Cli::InFile {
            file: file.clone(),
            query: words.join(" "),
        });
    }
    let split = args.iter().position(|a| a == "--");
    let (own, extra) = match split {
        Some(at) => (&args[..at], args[at + 1..].to_vec()),
        None => (args, Vec::new()),
    };
    let (mut tab, mut print, mut words) = (None, false, Vec::new());
    for (i, arg) in own.iter().enumerate() {
        let Some(word) = arg.to_str() else {
            return Err("the query must be valid UTF-8".into());
        };
        match word {
            "-h" | "--help" => return Ok(Cli::Help),
            "--print" => print = true,
            w if w.starts_with('-') && w.len() > 1 => {
                return Err(format!("unknown option: {w} (engine options go after --)"));
            }
            w if i == 0 => match Tab::from_name(w) {
                Some(t) => tab = Some(t),
                None => return Err(format!("unknown tab: {w}")),
            },
            w if tab.is_none() => return Err(format!("unknown tab: {w}")),
            w => words.push(w),
        }
    }
    let tab = tab.unwrap_or(Tab::Files);
    check_extra(tab, &extra)?;
    Ok(Cli::Run {
        tab,
        query: words.join(" "),
        print,
        extra,
    })
}

/// The options of one engine that change what it prints, which sf reads.
struct Engine {
    name: &'static str,
    /// Long options without their `--`; none of the engines takes abbreviations.
    deny_long: &'static [&'static str],
    deny_short: &'static str,
    /// Short options with a value: the rest of a cluster (`-e rs` as `-ers`) or the next argument.
    value_short: &'static str,
}

const FD: Engine = Engine {
    name: "fd",
    deny_long: &[
        "list-details",
        "print0",
        "exec",
        "exec-batch",
        "color",
        "quiet",
        "format",
        "path-separator",
        "base-directory",
        "gen-completions",
        "help",
        "version",
    ],
    deny_short: "l0xXcqhV",
    value_short: "dEteSoxXcj",
};

const UGREP: Engine = Engine {
    name: "ugrep",
    deny_long: &[
        "count",
        "json",
        "xml",
        "csv",
        "format",
        "format-begin",
        "format-end",
        "format-open",
        "format-close",
        "replace",
        "separator",
        "null",
        "null-data",
        "color",
        "colour",
        "colors",
        "query",
        "pretty",
        "heading",
        "tree",
        "only-line-number",
        "byte-offset",
        "column-number",
        "initial-tab",
        "after-context",
        "before-context",
        "context",
        "any-line",
        "passthru",
        "files-with-matches",
        "files-without-match",
        "no-filename",
        "hex",
        "with-hex",
        "hexdump",
        "stats",
        "config",
        "save-config",
        "pager",
        "help",
        "version",
    ],
    deny_short: "cQABCybkThlLVXW",
    value_short: "ABCDdefgJKMmNOtQZ",
};

const GIT: Engine = Engine {
    name: "git log",
    deny_long: &[
        "format",
        "pretty",
        "oneline",
        "color",
        "no-color",
        "patch",
        "stat",
        "numstat",
        "shortstat",
        "name-only",
        "name-status",
        "raw",
        "summary",
        "dirstat",
        "compact-summary",
        "show-signature",
        "walk-reflogs",
        "patch-with-stat",
        "patch-with-raw",
        "cc",
        "remerge-diff",
        "word-diff",
        "unified",
        "function-context",
        "check",
        "binary",
        "log-size",
        "line-prefix",
        "help",
    ],
    deny_short: "puzgUWXLh",
    value_short: "nSGOlMCB",
};

/// Refuses the engine options sf cannot read the output of; the message names the option.
pub fn check_extra(tab: Tab, extra: &[OsString]) -> Result<(), String> {
    let engine = match tab {
        Tab::Files | Tab::Folders => &FD,
        Tab::Content | Tab::Deep => &UGREP,
        Tab::Git => &GIT,
        Tab::Help if extra.is_empty() => return Ok(()),
        Tab::Help => return Err("sf help: the manual takes no engine options".into()),
    };
    let sf = format!("sf {}", tab.title().to_lowercase());
    let refuse = |option: &str| {
        Err(format!(
            "{sf}: {option} is refused: it changes what {} prints, which sf reads",
            engine.name
        ))
    };
    let mut value_next = false;
    for arg in extra {
        let Some(arg) = arg.to_str() else {
            value_next = false;
            continue;
        };
        if std::mem::take(&mut value_next) {
            continue;
        }
        if arg == "--" {
            break; // the engine's own end of options: what follows are its patterns or paths
        }
        if arg == "--print" {
            return Err(format!(
                "{sf}: --print is not an option of {} (sf's own options go before --)",
                engine.name
            ));
        }
        if let Some(long) = arg.strip_prefix("--") {
            let name = long.split('=').next().unwrap_or(long);
            if engine.deny_long.contains(&name) {
                return refuse(&format!("--{name}"));
            }
        } else if let Some(cluster) = arg.strip_prefix('-') {
            for (i, c) in cluster.char_indices() {
                if engine.deny_short.contains(c) {
                    return refuse(&format!("-{c}"));
                }
                if engine.value_short.contains(c) {
                    value_next = i + c.len_utf8() == cluster.len();
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn run(tab: Tab, query: &str, print: bool, extra: &[&str]) -> Cli {
        Cli::Run {
            tab,
            query: query.into(),
            print,
            extra: args(extra),
        }
    }

    #[test]
    fn the_views_take_one_file() {
        for (given, view) in [
            (vec!["history", "src/a.c"], View::History),
            (vec!["log", "src/a.c"], View::Log),
            (vec!["diff", "src/a.c"], View::Diff),
            (vec!["diff", "--short", "src/a.c"], View::ShortDiff),
        ] {
            assert_eq!(
                parse(&args(&given)),
                Ok(Cli::View {
                    view,
                    file: "src/a.c".into()
                }),
                "{given:?}"
            );
        }
        assert_eq!(
            parse(&args(&["log"])),
            Err("sf log: one FILE expected".into())
        );
        assert_eq!(
            parse(&args(&["diff", "a", "b"])),
            Err("sf diff: one FILE expected".into())
        );
    }

    #[test]
    fn in_takes_a_file_and_the_words_as_they_are() {
        assert_eq!(
            parse(&args(&["in", "src/a b.c", "ankara izmir"])),
            Ok(Cli::InFile {
                file: "src/a b.c".into(),
                query: "ankara izmir".into()
            })
        );
        assert_eq!(
            parse(&args(&["in", "a.c", "ankara", "-izmir"])),
            Ok(Cli::InFile {
                file: "a.c".into(),
                query: "ankara -izmir".into()
            }),
            "a word with a dash is a word here"
        );
        assert_eq!(parse(&args(&["in"])), Err("sf in: a FILE expected".into()));
    }

    #[test]
    fn a_tab_with_a_query_and_print() {
        assert_eq!(parse(&args(&[])), Ok(run(Tab::Files, "", false, &[])));
        assert_eq!(
            parse(&args(&["content"])),
            Ok(run(Tab::Content, "", false, &[]))
        );
        assert_eq!(
            parse(&args(&["content", "foo", "bar"])),
            Ok(run(Tab::Content, "foo bar", false, &[])),
            "query words are joined"
        );
        assert_eq!(
            parse(&args(&["deep", "needle", "--", "-i"])),
            Ok(run(Tab::Deep, "needle", false, &["-i"]))
        );
        assert_eq!(
            parse(&args(&["files", "--print", "main"])),
            Ok(run(Tab::Files, "main", true, &[]))
        );
        assert_eq!(
            parse(&args(&["--print"])),
            Ok(run(Tab::Files, "", true, &[]))
        );
        assert_eq!(
            parse(&args(&["search", "x"])),
            Ok(run(Tab::Content, "x", false, &[]))
        );
    }

    #[test]
    fn engine_options_after_the_double_dash() {
        assert_eq!(
            parse(&args(&["files", "main", "--", "-e", "rs", "--max-depth=2"])),
            Ok(run(
                Tab::Files,
                "main",
                false,
                &["-e", "rs", "--max-depth=2"]
            ))
        );
        assert_eq!(
            parse(&args(&["--", "-H"])),
            Ok(run(Tab::Files, "", false, &["-H"])),
            "Files without a tab name"
        );
        assert_eq!(
            parse(&args(&["content", "x", "--", "--print"])),
            Err(
                "sf content: --print is not an option of ugrep (sf's own options go before --)"
                    .into()
            ),
            "after -- everything is the engine's"
        );
    }

    #[test]
    fn help_doctor_and_the_commands() {
        assert_eq!(parse(&args(&["--help"])), Ok(Cli::Help));
        assert_eq!(parse(&args(&["files", "-h"])), Ok(Cli::Help));
        assert_eq!(parse(&args(&["doctor"])), Ok(Cli::Doctor));
        let q = crate::versions::Query {
            pattern: "x".into(),
            opts: crate::model::VersionsOpts::default(),
            revs: Vec::new(),
        };
        assert_eq!(
            parse(&args(&["grep-versions", "-e", "x"])),
            Ok(Cli::GrepVersions(q)),
            "the Git tab's Ctrl-Shift-Alt-F runs it as its list"
        );
        assert!(parse(&args(&["grep-versions"])).is_err());
        assert_eq!(
            parse(&args(&["open", "a b"])),
            Ok(Cli::Command {
                name: "open".into(),
                value: "a b".into()
            })
        );
        assert_eq!(
            parse(&args(&["copy-text", "--"])),
            Ok(Cli::Command {
                name: "copy-text".into(),
                value: "--".into()
            }),
            "the value is never an option"
        );
    }

    #[test]
    fn mistakes_are_said() {
        for (given, said) in [
            (vec!["nope"], "unknown tab: nope"),
            (
                vec!["files", "--hidden"],
                "unknown option: --hidden (engine options go after --)",
            ),
            (vec!["doctor", "x"], "sf doctor takes no arguments"),
            (vec!["open"], "sf open: one PATH expected"),
            (
                vec!["copy-text", "a", "b"],
                "sf copy-text: one TEXT expected",
            ),
        ] {
            assert_eq!(parse(&args(&given)), Err(said.to_string()), "{given:?}");
        }
    }

    #[test]
    fn options_that_change_the_output_are_refused() {
        for (tab, given, option) in [
            (Tab::Files, vec!["--list-details"], "--list-details"),
            (Tab::Files, vec!["-Hl"], "-l"),
            (Tab::Folders, vec!["--exec=ls"], "--exec"),
            (Tab::Content, vec!["-c"], "-c"),
            (Tab::Content, vec!["--json"], "--json"),
            (Tab::Content, vec!["-wC3"], "-C"),
            (Tab::Content, vec!["--colour=always"], "--colour"),
            (Tab::Deep, vec!["-l"], "-l"),
            (Tab::Deep, vec!["--format=%f"], "--format"),
            (Tab::Git, vec!["--oneline"], "--oneline"),
            (Tab::Git, vec!["-p"], "-p"),
        ] {
            let err = check_extra(tab, &args(&given)).unwrap_err();
            assert!(err.contains(&format!(" {option} ")), "{given:?}: {err}");
        }
    }

    #[test]
    fn other_options_and_values_pass() {
        for (tab, given) in [
            (
                Tab::Files,
                vec!["-e", "rs", "-HI", "--max-depth", "2", "-E", "target"],
            ),
            (Tab::Files, vec!["-el"]), // -e takes "l" as its value: not --list-details
            (Tab::Folders, vec!["-d3", "--changed-within=2weeks"]),
            (
                Tab::Content,
                vec!["-t", "rust", "-w", "-i", "--max-depth=3", "-g", "*.c"],
            ),
            (Tab::Content, vec!["-ec"]), // a second pattern "c"
            (Tab::Deep, vec!["-i", "--exclude=*.dll", "-t", "rust"]),
            (
                Tab::Git,
                vec!["--author=bilal", "-n", "5", "--since=2.weeks", "src"],
            ),
            (Tab::Files, vec!["--", "-c"]), // after the engine's own --, a pattern
        ] {
            assert_eq!(check_extra(tab, &args(&given)), Ok(()), "{given:?}");
        }
    }
}
