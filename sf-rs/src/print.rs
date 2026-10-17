//! `sf --print`, also when the output is not a terminal: the list the window would show, a row a line,
//! instead of the window. Exit status as grep's: 0 something was listed, 1 nothing, 2 an error.

use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::channel;

use crate::commands::{ListPlan, list_plan_with};
use crate::engines::Engines;
use crate::filter::Filter;
use crate::model::{Options, Tab};
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
    if tab != Tab::Content {
        let plan = list_plan_with(tab, options, query, engines, extra, false);
        let mut out = io::BufWriter::new(io::stdout().lock());
        return rows(tab, plan, query, root, &mut out, &mut io::stderr());
    }
    // ugrep's own output, as grep's: path, or path:line:text; coloured on a terminal
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

/// Files, Folders, Git: the window's rows of `plan` (sorted, numbered, filtered by `query`) to `out`,
/// the engine's messages to `err`.
fn rows(
    tab: Tab,
    plan: ListPlan,
    query: &str,
    root: &Path,
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
    filter.set_query(query);
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
        let written = if tab == Tab::Git {
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
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let root = temp_dir("print_root");
        let plan = list_plan_with(tab, &Options::default(), query, engines, &[], false);
        let code = rows(tab, plan, query, &root, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
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
