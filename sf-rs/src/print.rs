//! `sf --print`, also when the output is not a terminal: the list the window would show, a row a line,
//! instead of the window. On a terminal every row is drawn as in the window: no control character
//! reaches it (a file name holding escape sequences could otherwise drive the terminal). Into a pipe or
//! a file the real bytes go, for scripts: the paths, and in Content ugrep's own output. Exit status as
//! grep's: 0 something was listed, 1 nothing, 2 an error.

use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::channel;

use crate::commands::{ListPlan, list_plan_with};
use crate::engines::Engines;
use crate::filter::Filter;
use crate::model::{Opt, Options, Tab};
use crate::runner::{ListMsg, Runner};

/// Prints the list of `tab`; returns the exit status.
pub fn print(
    tab: Tab,
    options: &Options,
    query: &str,
    engines: &Engines,
    extra: &[OsString],
    root: &Path,
) -> u8 {
    let terminal = io::stdout().is_terminal();
    if !tab.searches() || terminal {
        let plan = list_plan_with(tab, options, query, engines, extra, false);
        let mut out = io::BufWriter::new(io::stdout().lock());
        return rows(
            tab,
            plan,
            query,
            options.get(tab, Opt::MatchCase),
            root,
            terminal,
            &mut out,
            &mut io::stderr(),
        );
    }
    // into a pipe: ugrep's own output, as grep's: path, or path:line:text
    match list_plan_with(tab, options, query, engines, extra, true) {
        ListPlan::Hint(hint) => {
            eprintln!("sf: {hint}");
            2
        }
        ListPlan::Run { cmd, .. } => {
            let status = Command::new(&cmd.program)
                .args(&cmd.args)
                .envs(cmd.env.iter().map(|(k, v)| (k, v)))
                .current_dir(root)
                .status();
            match status {
                Ok(s) => s.code().map_or(2, |c| c.clamp(0, 2) as u8),
                Err(e) => {
                    eprintln!("sf: {}: {e}", cmd.program.display());
                    2
                }
            }
        }
    }
}

/// The window's rows of `plan` (sorted, numbered, filtered by `query`) to `out`, the engine's messages
/// to `err`: for a `terminal` as drawn in the window, else Files' and Folders' paths as they are.
#[allow(clippy::too_many_arguments)]
fn rows(
    tab: Tab,
    plan: ListPlan,
    query: &str,
    match_case: bool,
    root: &Path,
    terminal: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> u8 {
    let (cmd, kind) = match plan {
        ListPlan::Run { cmd, kind } => (cmd, kind),
        ListPlan::Hint(hint) => {
            let _ = writeln!(err, "sf: {hint}");
            return 2;
        }
    };
    let (tx, rx) = channel();
    let mut runner = Runner::new(tx);
    runner.start(1, cmd, kind, root);
    let mut filter = Filter::new();
    filter.set_match_case(match_case);
    // as in the window: in Content and Deep the query is ugrep's pattern, not a filter of the rows
    filter.set_query(if tab.searches() { "" } else { query });
    let mut failed = false;
    for msg in rx {
        match msg {
            ListMsg::Items(_, items) => filter.push(items),
            ListMsg::Error(_, message) => {
                failed = true;
                let _ = writeln!(err, "{message}");
            }
            ListMsg::Done(_) => break,
        }
    }
    let mut listed = false;
    let parent = crate::items::parent();
    let rows = (0..filter.len()).filter_map(|i| filter.get(i));
    // Folders' first row, "..", is for going up in the window, not a folder of the list
    for item in rows.filter(|i| i.selectable && **i != parent) {
        // the drawn row has no control character: names through `printable`, Git through `ansi_text`
        let written = if terminal || tab == Tab::Git || tab.searches() {
            let line: String = item
                .display()
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect();
            writeln!(out, "{line}")
        } else {
            out.write_all(item.key.as_bytes())
                .and_then(|()| out.write_all(b"\n"))
        };
        match written {
            Ok(()) => listed = true,
            // the reader went away (`sf --print | head`): nothing more to do
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return 0,
            Err(e) => {
                let _ = writeln!(err, "sf: writing the list: {e}");
                return 2;
            }
        }
    }
    if let Err(e) = out.flush()
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        let _ = writeln!(err, "sf: writing the list: {e}");
        return 2;
    }
    match (failed, listed) {
        (true, _) => 2,
        (false, true) => 0,
        (false, false) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{executable, temp_dir};

    fn print_rows(tab: Tab, query: &str, engines: &Engines) -> (u8, String, String) {
        let (code, out, err) = printed(tab, query, engines, false);
        (code, String::from_utf8(out).unwrap(), err)
    }

    fn printed(tab: Tab, query: &str, engines: &Engines, terminal: bool) -> (u8, Vec<u8>, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let root = temp_dir("print_root");
        let plan = list_plan_with(tab, &Options::default(), query, engines, &[], false);
        let code = rows(tab, plan, query, false, &root, terminal, &mut out, &mut err);
        (code, out, String::from_utf8(err).unwrap())
    }

    #[test]
    fn a_terminal_never_gets_a_control_character_a_pipe_gets_the_name() {
        let name = "./a\x1b]0;owned\x07\x1b[2Jb.txt\0";
        let (_, out, _) = printed(Tab::Files, "", &fd(name, ""), true);
        assert!(!out.iter().any(|&b| b < 0x20 && b != b'\n'), "{out:?}");
        let (_, out, _) = printed(Tab::Files, "", &fd(name, ""), false);
        assert_eq!(
            out, b"a\x1b]0;owned\x07\x1b[2Jb.txt\n",
            "scripts get the real bytes"
        );
    }

    #[test]
    fn content_on_a_terminal_is_the_window_rows() {
        let bin = temp_dir("print_ugrep");
        // what sf's own ugrep command prints for Matching lines: path NUL line 0x1f text with marks
        let script =
            "#!/bin/sh\nprintf 'a\\033[2J.c\\0003\\037x \\001needle\\002\\033]0;t\\007 y\\n'\n";
        executable(&bin.join("ugrep"), script);
        let e = Engines {
            ugrep: Some(bin.join("ugrep")),
            ..Engines::default()
        };
        let mut o = Options::default();
        o.content.matching_lines = true;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let plan = list_plan_with(Tab::Content, &o, "needle", &e, &[], false);
        let code = rows(
            Tab::Content,
            plan,
            "needle",
            false,
            &temp_dir("print_c"),
            true,
            &mut out,
            &mut err,
        );
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{err:?}");
        assert!(
            out.contains(":3:x needle") && !out.contains('\x1b'),
            "{out:?}"
        );
    }

    #[test]
    fn deep_rows_are_not_filtered_by_the_pattern() {
        let bin = temp_dir("print_deep");
        executable(&bin.join("ugrep"), "#!/bin/sh\nprintf 'lib/a.dll\\000'\n");
        let e = Engines {
            ugrep: Some(bin.join("ugrep")),
            ..Engines::default()
        };
        let (code, out, err) = print_rows(Tab::Deep, "needle", &e);
        assert_eq!((code, out.as_str()), (0, "lib/a.dll\n"), "{err:?}");
    }

    /// Engines whose fd prints `listing` (NUL-separated, as with `-0`) and `message` on stderr.
    fn fd(listing: &str, message: &str) -> Engines {
        let bin = temp_dir("print_fd");
        let script = format!(
            "#!/bin/sh\nprintf '{}'\n[ -n '{message}' ] && echo '{message}' >&2\nexit 0\n",
            listing.replace('\0', "\\0")
        );
        executable(&bin.join("fd"), &script);
        Engines {
            fd: Some(bin.join("fd")),
            ..Engines::default()
        }
    }

    #[test]
    fn files_are_sorted_and_filtered_as_in_the_window() {
        let e = fd("./src/b.rs\0./a.txt\0./src/main.rs\0", "");
        assert_eq!(
            print_rows(Tab::Files, "", &e),
            (0, "a.txt\nsrc/b.rs\nsrc/main.rs\n".into(), "".into())
        );
        assert_eq!(
            print_rows(Tab::Files, "SRC rs", &e),
            (0, "src/b.rs\nsrc/main.rs\n".into(), "".into())
        );
        assert_eq!(print_rows(Tab::Files, "zzz", &e).0, 1, "nothing listed");
    }

    #[test]
    fn match_case_filters_the_printed_rows_too() {
        let e = fd("./src/Main.rs\0./src/main.c\0", "");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let plan = list_plan_with(Tab::Files, &Options::default(), "", &e, &[], false);
        let root = temp_dir("print_case");
        let code = rows(
            Tab::Files,
            plan,
            "Main",
            true,
            &root,
            false,
            &mut out,
            &mut err,
        );
        assert_eq!(
            (code, String::from_utf8(out).unwrap()),
            (0, "src/Main.rs\n".into())
        );
    }

    #[test]
    fn folders_leave_out_the_parent_row() {
        let e = fd("./b/c\0./a\0", "");
        assert_eq!(print_rows(Tab::Folders, "", &e).1, "a\nb/c\n");
    }

    #[test]
    fn engine_messages_go_to_stderr_with_status_2() {
        let (code, out, err) = print_rows(Tab::Files, "", &fd("./a\0", "fd: some error"));
        assert_eq!((code, out.as_str()), (2, "a\n"));
        assert!(err.contains("fd: some error"), "{err}");
        let (code, _, err) = print_rows(Tab::Files, "", &Engines::default());
        assert_eq!(code, 2);
        assert!(err.contains("fd is not installed"), "{err}");
    }

    #[test]
    fn git_rows_without_colour() {
        let bin = temp_dir("print_git");
        let script = "#!/bin/sh\nprintf '* \\033[1;34mabc1234\\033[m T second\\037abc1234\\n'\n\
                      printf '* \\033[1;34mdef5678\\033[m T first\\037def5678\\n'\n";
        executable(&bin.join("git"), script);
        let e = Engines {
            git: Some(bin.join("git")),
            ..Engines::default()
        };
        let (code, out, _) = print_rows(Tab::Git, "first", &e);
        assert_eq!(code, 0);
        assert!(
            !out.contains('\x1b') && out.contains("def5678 T first") && !out.contains("second"),
            "{out:?}"
        );
    }
}
