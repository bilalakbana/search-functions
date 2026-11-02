//! Runs a list's engine as a child process and streams its rows over a channel.
//!
//! The child gets a process group of its own, so cancelling also ends what it started (ugrep+ is a
//! shell script whose `ug` would outlive a killed shell). Its exit status is ignored: ugrep exits 1
//! when nothing matches and 2 as soon as one file is unreadable; whatever went wrong is on stderr,
//! and every stderr line becomes an `Error`.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::commands::{Cmd, ParseKind, SEP};
use crate::find::{Keeper, RowWords};
use crate::items::{self, Item, parse};

#[derive(Debug)]
pub enum ListMsg {
    Items(u64, Vec<Item>),
    Error(u64, String),
    Done(u64),
}

/// Rows are sent at most this many at a time ...
const BATCH: usize = 512;
/// ... and at least this often while the engine keeps writing.
const BATCH_TIME: Duration = Duration::from_millis(50);
/// More stderr lines than this are counted, not shown.
const MAX_ERRORS: usize = 100;
/// The longest record kept: paths, line numbers and the start of a matching line fit many times over
/// (its text is cut at 1000 characters anyway).
const MAX_RECORD: usize = 64 << 10;
/// The longest stderr line kept.
const MAX_ERROR_LINE: usize = 4 << 10;
/// The most rows a list of matches (Matching lines, Each match, the versions search) keeps: a row
/// takes about 200 bytes, so 200 MB. A one-letter pattern in a large tree has tens of millions, more
/// memory than the machine has (2.7 million Each match rows of `struct` in Linux's tree: 537 MB).
#[cfg(not(test))]
const MAX_MATCH_ROWS: usize = 1_000_000;
#[cfg(test)]
const MAX_MATCH_ROWS: usize = 1000;

pub struct Runner {
    tx: Sender<ListMsg>,
    current: Option<Job>,
}

struct Job {
    /// The child and whether it has been reaped; its pid is the process group's id, which is safe to
    /// signal only while the child is unreaped (until then the number cannot be reused).
    child: Arc<Mutex<(Child, bool)>>,
}

impl Runner {
    pub fn new(tx: Sender<ListMsg>) -> Runner {
        Runner { tx, current: None }
    }

    /// Cancels the running list and starts `cmd` in `root`; its messages carry `generation`. With
    /// `keep`, only the rows with each of its words on their line or in their path are listed.
    pub fn start(
        &mut self,
        generation: u64,
        cmd: Cmd,
        kind: ParseKind,
        keep: Option<RowWords>,
        root: &Path,
    ) {
        self.cancel();
        let spawned = Command::new(&cmd.program)
            .args(&cmd.args)
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                let _ = self.tx.send(ListMsg::Error(
                    generation,
                    format!("{}: {e}", cmd.program.display()),
                ));
                let _ = self.tx.send(ListMsg::Done(generation));
                return;
            }
        };
        track(child.id());
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let child = Arc::new(Mutex::new((child, false)));

        let err_tx = self.tx.clone();
        let errors = thread::spawn(move || read_errors(generation, stderr, &err_tx));
        let (tx, job, root) = (self.tx.clone(), child.clone(), root.to_path_buf());
        thread::spawn(move || {
            read_items(
                generation,
                kind,
                keep.map(|k| k.keeper()),
                stdout,
                &tx,
                &root,
                &job,
            );
            let _ = errors.join();
            reap(&job);
            let _ = tx.send(ListMsg::Done(generation));
        });
        self.current = Some(Job { child });
    }

    /// Kills the running list's process group and reaps the child.
    pub fn cancel(&mut self) {
        let Some(job) = self.current.take() else {
            return;
        };
        let mut guard = job.child.lock().unwrap_or_else(|e| e.into_inner());
        let (child, reaped) = &mut *guard;
        if !*reaped {
            untrack(child.id());
            // SAFETY: kill(2) with a negative pid signals that process group; the group's id is our
            // unreaped child's pid, so it cannot belong to anyone else.
            unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            let _ = child.wait();
            *reaped = true;
        }
    }
}

/// Waits for the child after its output has ended, without holding the lock while it runs (cancel
/// needs it).
fn reap(job: &Mutex<(Child, bool)>) {
    untrack(job.lock().unwrap_or_else(|e| e.into_inner()).0.id());
    loop {
        {
            let mut guard = job.lock().unwrap_or_else(|e| e.into_inner());
            let (child, reaped) = &mut *guard;
            if *reaped || !matches!(child.try_wait(), Ok(None)) {
                *reaped = true;
                return;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// The process groups of the engines running now (the list's, the preview's). Each has a group of its
/// own, so cancelling ends what it started; but then the signal that ends sf (SIGHUP when the terminal
/// closes, SIGTERM, Ctrl-C's SIGINT under --print) never reaches them, and an engine that finds
/// nothing to write ran on for good. `end_engines_with_sf` kills these groups first.
/// shortcut: SIGKILL cannot be caught, so `kill -9` of sf still leaves an engine running until it
/// writes; a slot is a group, and more than GROUPS.len() at once go untracked.
static GROUPS: [AtomicI32; 8] = [const { AtomicI32::new(0) }; 8];

/// `pgid` is an engine's process group from now on, until `untrack`, which comes before its reaping.
pub fn track(pgid: u32) {
    let pgid = pgid as i32;
    let _ = GROUPS.iter().any(|g| {
        g.compare_exchange(0, pgid, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    });
}

pub fn untrack(pgid: u32) {
    for g in &GROUPS {
        let _ = g.compare_exchange(pgid as i32, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

/// SIGHUP, SIGTERM, SIGINT and SIGQUIT kill the engines' process groups, then end sf as they would.
pub fn end_engines_with_sf() {
    extern "C" fn on_signal(signal: libc::c_int) {
        for g in &GROUPS {
            let pgid = g.load(Ordering::SeqCst);
            if pgid > 0 {
                // SAFETY: kill(2) is async-signal-safe; the group is our unreaped child's (`track`).
                unsafe { libc::kill(-pgid, libc::SIGKILL) };
            }
        }
        // SAFETY: signal(2) and raise(3) are async-signal-safe: sf ends as the signal says.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    for signal in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT, libc::SIGQUIT] {
        // SAFETY: signal(2) with a handler that makes only async-signal-safe calls; one sf was started
        // ignoring (nohup's SIGHUP, a script's `&` job's SIGINT) keeps ignoring, as its engines then do
        unsafe {
            if libc::signal(signal, handler) == libc::SIG_IGN {
                libc::signal(signal, libc::SIG_IGN);
            }
        }
    }
}

/// ugrep's warning about a path it cannot read or open, or one that is gone (ugrep runs without -s).
pub fn unreadable(line: &[u8]) -> bool {
    uncoloured(line).starts_with(b"ugrep: warning: ")
}

/// `line` without its colour sequences (`ESC [ digits ; m`): Each match runs ugrep with --color=always,
/// which colours its messages too.
fn uncoloured(line: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if line[i] == 0x1b && line.get(i + 1) == Some(&b'[') {
            let end = line[i + 2..]
                .iter()
                .position(|b| !(b.is_ascii_digit() || *b == b';'))
                .map(|n| i + 2 + n);
            if let Some(end) = end.filter(|&e| line[e] == b'm') {
                i = end + 1;
                continue;
            }
        }
        out.push(line[i]);
        i += 1;
    }
    out
}

/// The one line that stands for `n` such warnings.
pub fn unreadable_note(n: usize) -> String {
    format!(
        "{n} {} could not be read",
        if n == 1 { "path" } else { "paths" }
    )
}

/// The first of ugrep's three lines for a pattern that is no regular expression; the second is the
/// expression as ugrep compiled it (with its own `(?mi)`, which the position counts), the third a
/// mark under the place with what is wrong.
const REGEX_ERROR: &str = "ugrep: error: error at position ";

/// ugrep's three lines for a bad regular expression as one that says what is wrong; None for others.
fn regex_error(lines: &[String]) -> Option<String> {
    let [first, _, mark] = lines else {
        return None;
    };
    let what = mark
        .trim_start_matches("\\___")
        .trim_end_matches("___/")
        .trim();
    (first.starts_with(REGEX_ERROR) && mark.contains("___") && !what.is_empty()).then(|| {
        format!(
            "ugrep: not a valid regular expression ({what}); \
             ⚙ Config: Regular expression off searches the text as typed"
        )
    })
}

/// The engine's stderr lines as errors; the `unreadable` ones as one line at the end.
fn read_errors(generation: u64, stderr: impl Read, tx: &Sender<ListMsg>) {
    let mut shown = 0;
    let mut more = 0;
    let mut unread = 0;
    let mut emit = |line: String| {
        if unreadable(line.as_bytes()) {
            unread += 1;
        } else if shown == MAX_ERRORS {
            more += 1;
        } else if tx.send(ListMsg::Error(generation, line)).is_ok() {
            shown += 1;
        } else {
            return false;
        }
        true
    };
    let (mut reader, mut line) = (BufReader::new(stderr), Vec::new());
    let mut held = Vec::new();
    while let Ok(Some(_)) = read_record(&mut reader, b'\n', &mut line, MAX_ERROR_LINE) {
        let line = String::from_utf8_lossy(&uncoloured(&line))
            .trim()
            .to_string();
        if line.is_empty() {
            continue;
        }
        if held.is_empty() && !line.starts_with(REGEX_ERROR) {
            if !emit(line) {
                return;
            }
            continue;
        }
        held.push(line);
        if held.len() == 3 {
            let lines = std::mem::take(&mut held);
            let sent = match regex_error(&lines) {
                Some(one) => emit(one),
                None => lines.into_iter().all(&mut emit),
            };
            if !sent {
                return;
            }
        }
    }
    if !held.into_iter().all(&mut emit) {
        return;
    }
    if unread > 0 {
        let _ = tx.send(ListMsg::Error(generation, unreadable_note(unread)));
    }
    if more > 0 {
        let _ = tx.send(ListMsg::Error(
            generation,
            format!("… and {more} more messages"),
        ));
    }
}

/// Rows from stdout. Files, Folders and Git need all of them first (sorted, sorted by depth,
/// numbered); the others are sent as they come. Only Git keeps its raw lines until the end.
fn read_items(
    generation: u64,
    kind: ParseKind,
    keep: Option<Keeper>,
    stdout: impl Read,
    tx: &Sender<ListMsg>,
    root: &Path,
    job: &Mutex<(Child, bool)>,
) {
    let delimiter = match kind {
        ParseKind::Files | ParseKind::Folders | ParseKind::SearchFiles { .. } => 0,
        ParseKind::SearchLines
        | ParseKind::SearchMatches
        | ParseKind::Git
        | ParseKind::Versions => b'\n',
    };
    let streamed = matches!(
        kind,
        ParseKind::SearchFiles { .. }
            | ParseKind::SearchLines
            | ParseKind::SearchMatches
            | ParseKind::Versions
    );
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    let (mut record, mut batch, mut records) = (Vec::new(), Vec::new(), Vec::new());
    let mut seen = HashSet::new();
    let mut split = Vec::new();
    let mut listed = 0;
    let mut sent = Instant::now();
    loop {
        // Send what is there before a read that may wait for the engine.
        let due =
            reader.buffer().is_empty() || batch.len() >= BATCH || sent.elapsed() >= BATCH_TIME;
        if streamed && !batch.is_empty() && due {
            if tx
                .send(ListMsg::Items(generation, std::mem::take(&mut batch)))
                .is_err()
            {
                return;
            }
            sent = Instant::now();
        }
        let cut = match read_record(&mut reader, delimiter, &mut record, MAX_RECORD) {
            Ok(None) => break,
            Ok(Some(cut)) => cut,
            Err(e) => {
                let _ = tx.send(ListMsg::Error(generation, format!("reading the list: {e}")));
                break;
            }
        };
        // a cut path would name another file; a cut line or commit only loses its end
        if cut && delimiter == 0 {
            let message = format!("left out a name longer than {} KiB", MAX_RECORD >> 10);
            if tx.send(ListMsg::Error(generation, message)).is_err() {
                return;
            }
            continue;
        }
        if kind == ParseKind::Git {
            records.push(std::mem::take(&mut record)); // numbered once all are there
            continue;
        }
        if delimiter == b'\n' && !rejoin(kind, &mut split, &mut record, root) {
            continue;
        }
        if let Some(item) = parse(kind, &record, 0) {
            if matches!(kind, ParseKind::SearchFiles { .. }) && !seen.insert(item.key.clone()) {
                continue;
            }
            let kept = |k: &Keeper| {
                item.match_parts()
                    .is_none_or(|(path, text)| k.keeps(&path, &text))
            };
            if !keep.as_ref().is_none_or(kept) {
                continue;
            }
            batch.push(item);
            if delimiter == b'\n' {
                listed += 1;
                if listed == MAX_MATCH_ROWS {
                    // not left to SIGPIPE: sf grep-versions ignores it and would search on
                    let guard = job.lock().unwrap_or_else(|e| e.into_inner());
                    if !guard.1 {
                        // SAFETY: kill(2) on our unreaped child's process group (see Job).
                        unsafe { libc::kill(-(guard.0.id() as libc::pid_t), libc::SIGKILL) };
                    }
                    break;
                }
            }
        }
    }
    match kind {
        ParseKind::Git => batch = numbered(std::mem::take(&mut records)),
        ParseKind::Files => items::sort_files(&mut batch),
        ParseKind::Folders => {
            items::sort_folders(&mut batch);
            batch.insert(0, items::parent());
        }
        ParseKind::SearchFiles { .. }
        | ParseKind::SearchLines
        | ParseKind::SearchMatches
        | ParseKind::Versions => {}
    }
    if !batch.is_empty() {
        let _ = tx.send(ListMsg::Items(generation, batch));
    }
    if listed == MAX_MATCH_ROWS {
        let _ = tx.send(ListMsg::Error(generation, cut_note()));
    }
}

/// What the list says when it stopped at MAX_MATCH_ROWS.
fn cut_note() -> String {
    format!("only the first {MAX_MATCH_ROWS} matches are listed: a narrower pattern lists them all")
}

/// Search and version rows are lines, but a path may hold a newline: its row then comes as several
/// lines, and only the last holds the NUL after the path. The lines before it wait in `split`; false
/// while `record` may be such a start. A Matching lines or Each match row joins the most waiting lines
/// that make its path one that exists in `root` (the others are ugrep's "Binary file … matches"
/// lines); a version row (`REV:PATH\0LINE\0TEXT`) joins from a line that starts with `REV:` to its
/// second NUL.
fn rejoin(kind: ParseKind, split: &mut Vec<Vec<u8>>, record: &mut Vec<u8>, root: &Path) -> bool {
    // shortcut: a path with 64 newlines or more is listed under its last line, not its path; raise
    // the limit if such names turn up
    const MAX_SPLIT: usize = 64;
    let nuls = record.iter().filter(|&&b| b == 0).count();
    if kind == ParseKind::Versions {
        let starts = |r: &[u8]| {
            r.get(40) == Some(&b':')
                && r.get(..40)
                    .is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit))
        };
        if split.is_empty() && (nuls >= 2 || !starts(record)) {
            return true;
        }
        split.push(std::mem::take(record));
        let joined = split.join(&b'\n');
        if joined.iter().filter(|&&b| b == 0).count() < 2 && split.len() < MAX_SPLIT {
            return false;
        }
        split.clear();
        *record = joined;
        return true;
    }
    let Some(nul) = record.iter().position(|&b| b == 0) else {
        if split.len() == MAX_SPLIT {
            split.remove(0);
        }
        split.push(std::mem::take(record));
        return false;
    };
    // Each match: ugrep's colour reset after the name is not part of it
    let name = &record[..nul];
    let name = name.strip_suffix(b"\x1b[m").unwrap_or(name);
    let joins = (1..=split.len()).rev().find(|&k| {
        let mut path = split[split.len() - k..].join(&b'\n');
        path.push(b'\n');
        path.extend_from_slice(name);
        root.join(OsStr::from_bytes(&path))
            .symlink_metadata()
            .is_ok()
    });
    if let Some(k) = joins {
        let mut joined = split[split.len() - k..].join(&b'\n');
        joined.push(b'\n');
        joined.append(record);
        *record = joined;
    }
    split.clear();
    true
}

/// Git rows numbered as sub-functions/git_logs does: the oldest commit 1, the newest the count.
fn numbered(records: Vec<Vec<u8>>) -> Vec<Item> {
    let mut number = records.iter().filter(|r| r.contains(&SEP)).count() as u32;
    records
        .into_iter()
        .map(|r| {
            let commit = r.contains(&SEP);
            let item = items::git_row(r, number);
            if commit {
                number = number.saturating_sub(1);
            }
            item
        })
        .collect()
}

/// Reads the next record (up to `delimiter`, which is not kept) into `buf`, keeping at most `max` bytes
/// of it and skipping the rest, so one huge line never sits in memory whole. `Ok(None)` at the end,
/// else whether the record was cut.
fn read_record(
    r: &mut impl BufRead,
    delimiter: u8,
    buf: &mut Vec<u8>,
    max: usize,
) -> io::Result<Option<bool>> {
    buf.clear();
    let (mut cut, mut any) = (false, false);
    loop {
        let available = match r.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok(any.then_some(cut));
        }
        any = true;
        let end = available.iter().position(|&b| b == delimiter);
        let part = &available[..end.unwrap_or(available.len())];
        let room = max - buf.len();
        cut |= part.len() > room;
        buf.extend_from_slice(&part[..part.len().min(room)]);
        let used = end.map_or(available.len(), |i| i + 1);
        r.consume(used);
        if end.is_some() {
            return Ok(Some(cut));
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;
    use std::sync::mpsc::{Receiver, channel};

    fn sh(script: &str) -> Cmd {
        Cmd::new("/bin/sh").args(["-c", script])
    }

    fn printf(format: &str) -> Cmd {
        Cmd::new("/usr/bin/printf").arg(format)
    }

    /// Every message of `generation` up to and including its Done.
    fn until_done(rx: &Receiver<ListMsg>, generation: u64) -> (Vec<Item>, Vec<String>) {
        let (mut items, mut errors) = (Vec::new(), Vec::new());
        loop {
            match rx
                .recv_timeout(Duration::from_secs(5))
                .expect("no Done within 5 s")
            {
                ListMsg::Items(g, v) if g == generation => items.extend(v),
                ListMsg::Error(g, e) if g == generation => errors.push(e),
                ListMsg::Done(g) if g == generation => return (items, errors),
                _ => {}
            }
        }
    }

    fn keys(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|i| i.key.to_string_lossy().into_owned())
            .collect()
    }

    fn run(cmd: Cmd, kind: ParseKind) -> (Vec<Item>, Vec<String>) {
        let (tx, rx) = channel();
        let mut r = Runner::new(tx);
        r.start(1, cmd, kind, None, Path::new("/"));
        until_done(&rx, 1)
    }

    fn gone(pid: &str) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => true,
            Ok(stat) => stat
                .rsplit(')')
                .next()
                .unwrap()
                .trim_start()
                .starts_with('Z'),
        }
    }

    fn wait_for_file(path: &Path) -> String {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(5) {
            if let Ok(s) = std::fs::read_to_string(path) {
                if s.ends_with('\n') {
                    return s.trim().to_string();
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("{} was not written", path.display());
    }

    #[test]
    fn read_record_keeps_at_most_max_bytes() {
        let mut data = vec![b'x'; 10 << 20];
        data.extend_from_slice(b"\nsecond\nlast");
        let mut r = BufReader::with_capacity(64 * 1024, &data[..]);
        let mut buf = Vec::new();
        assert_eq!(
            read_record(&mut r, b'\n', &mut buf, 1000).unwrap(),
            Some(true)
        );
        assert_eq!(buf.len(), 1000);
        assert!(buf.capacity() < 64 * 1024, "capacity {}", buf.capacity());
        assert_eq!(
            read_record(&mut r, b'\n', &mut buf, 1000).unwrap(),
            Some(false)
        );
        assert_eq!(buf, b"second");
        assert_eq!(
            read_record(&mut r, b'\n', &mut buf, 1000).unwrap(),
            Some(false)
        );
        assert_eq!(buf, b"last");
        assert_eq!(read_record(&mut r, b'\n', &mut buf, 1000).unwrap(), None);
    }

    #[test]
    fn read_record_handles_empty_records_and_nul() {
        let mut r = BufReader::new(&b"a\0\0b\0"[..]);
        let mut buf = Vec::new();
        let mut got = Vec::new();
        while let Some(cut) = read_record(&mut r, 0, &mut buf, 10).unwrap() {
            got.push((buf.clone(), cut));
        }
        assert_eq!(
            got,
            [
                (b"a".to_vec(), false),
                (b"".to_vec(), false),
                (b"b".to_vec(), false)
            ]
        );
    }

    #[test]
    fn a_huge_matching_line_is_cut() {
        // 20 MB on one line: the row keeps its key and line number, its text is cut
        let script =
            "printf 'big.min.js\\0001\\037'; head -c 20000000 /dev/zero | tr '\\0' x; printf '\\n'";
        let (items, errors) = run(sh(script), ParseKind::SearchLines);
        assert_eq!(keys(&items), ["big.min.js"]);
        assert_eq!(items[0].line, Some(1));
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_name_too_long_is_left_out_and_said() {
        let script = "head -c 100000 /dev/zero | tr '\\0' a; printf '\\0ok\\0'";
        let (items, errors) = run(sh(script), ParseKind::SearchFiles { archives: false });
        assert_eq!(keys(&items), ["ok"]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("longer than"), "{errors:?}");
    }

    #[test]
    fn only_rows_with_each_word_on_their_line_or_in_their_path_are_kept() {
        let (tx, rx) = channel();
        let mut r = Runner::new(tx);
        let keep = crate::find::RowWords {
            words: vec!["code".into(), "interest".into()],
            regex: true,
            whole_word: false,
            smart_case: false,
            match_case: false,
        };
        r.start(
            1,
            printf("CODE.md\\0007\\037the \\001interest\\002\\nb.md\\0001\\037\\001interest\\002 only\\n"),
            ParseKind::SearchLines,
            Some(keep),
            Path::new("/"),
        );
        let (items, errors) = until_done(&rx, 1);
        assert_eq!(
            keys(&items),
            ["CODE.md"],
            "code in its path, interest on its line"
        );
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn streams_items_from_printf() {
        let (items, errors) = run(
            printf("a\\0b\\0c\\0"),
            ParseKind::SearchFiles { archives: false },
        );
        assert_eq!(keys(&items), ["a", "b", "c"]);
        assert!(errors.is_empty());
    }

    #[test]
    fn rows_arrive_while_the_engine_still_runs() {
        let (tx, rx) = channel();
        let mut r = Runner::new(tx);
        r.start(
            1,
            sh("printf 'a\\0'; sleep 3"),
            ParseKind::SearchFiles { archives: false },
            None,
            Path::new("/"),
        );
        match rx.recv_timeout(Duration::from_millis(1000)) {
            Ok(ListMsg::Items(1, v)) => assert_eq!(keys(&v), ["a"]),
            other => panic!("expected the row at once, got {other:?}"),
        }
    }

    #[test]
    fn stderr_lines_become_errors() {
        let (items, errors) = run(
            sh("echo oops >&2; echo >&2; echo two >&2"),
            ParseKind::Files,
        );
        assert!(items.is_empty());
        assert_eq!(errors, ["oops", "two"]);
    }

    #[test]
    fn a_list_of_matches_stops_at_its_most_rows_and_says_so() {
        // an engine that never ends: the list stops it at MAX_MATCH_ROWS
        let endless = sh("while :; do printf 'a\\0001\\037x\\n'; done");
        let (items, errors) = run(endless, ParseKind::SearchLines);
        assert_eq!(items.len(), MAX_MATCH_ROWS);
        assert_eq!(errors, [cut_note()]);
        // one that ignores SIGPIPE (sf grep-versions, a Rust program) is ended all the same
        let deaf = sh(
            "trap '' PIPE; while :; do printf 'a\\0001\\037x\\n' || echo 'write error' >&2; done",
        );
        let (items, errors) = run(deaf, ParseKind::SearchLines);
        assert_eq!(items.len(), MAX_MATCH_ROWS);
        assert_eq!(errors, [cut_note()]);
        // a file list (one row a file) is not cut
        let files = sh("i=0; while [ $i -lt 1100 ]; do printf 'f%d\\0' $i; i=$((i+1)); done");
        let (items, errors) = run(files, ParseKind::SearchFiles { archives: false });
        assert_eq!((items.len(), errors.len()), (1100, 0));
    }

    #[test]
    fn a_bad_regular_expression_is_one_line_that_says_what_is_wrong() {
        // ugrep's own three lines: the position counts in its (?mi) prefix, the mark loses its column
        let script = "printf 'ugrep: error: error at position 7\\n(?mi)a(\\n       \\\\___mismatched ( )\\n\\n' >&2";
        let (_, errors) = run(sh(script), ParseKind::Files);
        assert_eq!(
            errors,
            ["ugrep: not a valid regular expression (mismatched ( )); \
              ⚙ Config: Regular expression off searches the text as typed"]
        );
        let script =
            "printf 'ugrep: error: error at position 19\\n(?mi)abc(\\n mismatched ( )___/\\n' >&2";
        let (_, errors) = run(sh(script), ParseKind::Files);
        assert!(errors[0].contains("(mismatched ( ))"), "{errors:?}");
        // anything else, or the output ending early, stays as it was
        let script = "printf 'ugrep: error: error at position 3\\nother\\n' >&2";
        let (_, errors) = run(sh(script), ParseKind::Files);
        assert_eq!(errors, ["ugrep: error: error at position 3", "other"]);
    }

    #[test]
    fn paths_that_could_not_be_read_are_one_line() {
        let script = "echo 'ugrep: warning: cannot read a.txt: Permission denied' >&2; \
                      echo 'ugrep: error: error at position 0' >&2; \
                      echo 'ugrep: warning: cannot open directory d: Permission denied' >&2; \
                      echo 'ugrep: warning: gone.txt: No such file or directory' >&2";
        let (_, errors) = run(sh(script), ParseKind::SearchFiles { archives: false });
        assert_eq!(
            errors,
            [
                "ugrep: error: error at position 0",
                "3 paths could not be read"
            ]
        );
        let (_, errors) = run(
            sh("echo 'ugrep: warning: cannot read a: Permission denied' >&2"),
            ParseKind::SearchFiles { archives: false },
        );
        assert_eq!(errors, ["1 path could not be read"]);
        // Each match runs ugrep with --color=always: its warnings come coloured
        let script = "printf '\\033[mugrep: warning:\\033[m cannot read u.txt:\\033[m Permission denied\\n' >&2; \
                      printf '\\033[1;31mugrep: error:\\033[m boom\\n' >&2";
        let (_, errors) = run(sh(script), ParseKind::SearchMatches);
        assert_eq!(errors, ["ugrep: error: boom", "1 path could not be read"]);
    }

    #[test]
    fn too_many_errors_are_counted() {
        let (_, errors) = run(
            sh("i=0; while [ $i -lt 150 ]; do echo e$i >&2; i=$((i+1)); done"),
            ParseKind::Files,
        );
        assert_eq!(errors.len(), MAX_ERRORS + 1);
        assert_eq!(errors.last().unwrap(), "… and 50 more messages");
    }

    #[test]
    fn nonzero_exit_is_not_an_error() {
        let (items, errors) = run(
            sh("printf 'a\\0'; exit 2"),
            ParseKind::SearchFiles { archives: false },
        );
        assert_eq!(keys(&items), ["a"]);
        assert!(errors.is_empty());
    }

    #[test]
    fn missing_program_is_an_error() {
        let (items, errors) = run(Cmd::new("/nonexistent/fd"), ParseKind::Files);
        assert!(items.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("/nonexistent/fd: "), "{errors:?}");
    }

    #[test]
    fn runs_in_root() {
        let dir = temp_dir("runner_root");
        let (items, _) = run_in(
            sh("pwd | tr '\\n' '\\0'"),
            ParseKind::SearchFiles { archives: false },
            &dir,
        );
        assert_eq!(keys(&items), [dir.to_str().unwrap()]);
    }

    fn run_in(cmd: Cmd, kind: ParseKind, root: &Path) -> (Vec<Item>, Vec<String>) {
        let (tx, rx) = channel();
        let mut r = Runner::new(tx);
        r.start(1, cmd, kind, None, root);
        until_done(&rx, 1)
    }

    #[test]
    fn a_newline_in_a_path_keeps_its_matching_line_one_row() {
        let dir = temp_dir("runner_newline");
        std::fs::write(dir.join("match\nnewline.txt"), "").unwrap();
        std::fs::write(dir.join("plain.txt"), "").unwrap();
        // ugrep's Matching lines rows, with a binary's message line before the split one
        let rows = "Binary file x.bin matches\\nmatch\\nnewline.txt\\0001\\037\\001needle\\002 a\\n\
                    plain.txt\\0002\\037\\001needle\\002 b\\n";
        let (items, errors) = run_in(printf(rows), ParseKind::SearchLines, &dir);
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(keys(&items), ["match\nnewline.txt", "plain.txt"]);
        assert_eq!(items[0].line, Some(1));
        // when a shorter join names a file too, the row is the longer path's, as its lines say
        std::fs::write(dir.join("x\nmatch\nnewline.txt"), "").unwrap();
        let rows = "x\\nmatch\\nnewline.txt\\0003\\037\\001needle\\002 c\\n";
        let (items, _) = run_in(printf(rows), ParseKind::SearchLines, &dir);
        assert_eq!(keys(&items), ["x\nmatch\nnewline.txt"]);
        // Each match: ugrep's colour reset after the name, and an escape sequence in the name itself
        std::fs::write(dir.join("esc-\x1b[31mred.txt"), "").unwrap();
        let rows = "match\\nnewline.txt\\033[m\\0001\\033[m\\037\\033[7mneedle\\033[m a\\n\
                    esc-\\033[31mred.txt\\033[m\\0002\\033[m\\037\\033[7mneedle\\033[m b\\n";
        let (items, _) = run_in(printf(rows), ParseKind::SearchMatches, &dir);
        assert_eq!(keys(&items), ["match\nnewline.txt", "esc-\x1b[31mred.txt"]);
        assert_eq!(items[1].line, Some(2));
    }

    #[test]
    fn a_newline_in_a_versions_path_keeps_its_row_one_row() {
        let rev = "a".repeat(40);
        let rows = format!(
            "Binary file {rev}:x matches\\n{rev}:two\\nlines.txt\\0003\\000needle\\n{rev}:b.txt\\0004\\000n\\n"
        );
        let (items, _) = run(printf(&rows), ParseKind::Versions);
        assert_eq!(
            keys(&items),
            [format!("{rev}:two\nlines.txt"), format!("{rev}:b.txt")]
        );
    }

    #[test]
    fn files_are_sorted() {
        let (items, _) = run(printf("./b/y\\0./a/s/z\\0./B\\0./a/x\\0"), ParseKind::Files);
        assert_eq!(keys(&items), ["B", "a/x", "a/s/z", "b/y"]);
    }

    #[test]
    fn folders_by_depth_after_parent() {
        let (items, _) = run(printf("./b/\\0./a/c/\\0./a/\\0"), ParseKind::Folders);
        assert_eq!(keys(&items), ["..", "a", "b", "a/c"]);
    }

    #[test]
    fn git_numbers_commits_oldest_first() {
        let (items, _) = run(
            printf("* x\\037abcdef1\\n|\\n* y\\037abcdef2\\n"),
            ParseKind::Git,
        );
        let shown: Vec<String> = items
            .iter()
            .map(|i| {
                i.display()
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect()
            })
            .collect();
        assert_eq!(shown, ["2 * x", "|", "1 * y"]);
        assert_eq!(keys(&items), ["abcdef1", "", "abcdef2"]);
    }

    #[test]
    fn search_files_deduplicated() {
        let (items, _) = run(
            printf("a.zip{m}\\0a.zip{n}\\0b\\0"),
            ParseKind::SearchFiles { archives: true },
        );
        assert_eq!(keys(&items), ["a.zip", "b"]);
    }

    #[test]
    fn cancel_kills_child_quickly() {
        let pidfile = temp_dir("runner_cancel").join("pid");
        let (tx, rx) = channel();
        let mut r = Runner::new(tx);
        let script = format!("echo $$ > '{}'; exec sleep 30", pidfile.display());
        r.start(1, sh(&script), ParseKind::Files, None, Path::new("/"));
        let pid = wait_for_file(&pidfile);
        let t = Instant::now();
        r.cancel();
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "cancel took {:?}",
            t.elapsed()
        );
        assert!(
            std::fs::metadata(format!("/proc/{pid}")).is_err(),
            "child {pid} not reaped"
        );
        drop(rx);
    }

    #[test]
    fn cancel_kills_grandchildren() {
        let pidfile = temp_dir("runner_grandchild").join("pid");
        let (tx, _rx) = channel();
        let mut r = Runner::new(tx);
        let script = format!("sleep 30 & echo $! > '{}'; wait", pidfile.display());
        r.start(1, sh(&script), ParseKind::Files, None, Path::new("/"));
        let pid = wait_for_file(&pidfile);
        r.cancel();
        let t = Instant::now();
        while !gone(&pid) && t.elapsed() < Duration::from_secs(1) {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(gone(&pid), "grandchild {pid} still running");
    }

    #[test]
    fn start_cancels_the_previous_list() {
        let pidfile = temp_dir("runner_restart").join("pid");
        let (tx, rx) = channel();
        let mut r = Runner::new(tx);
        let script = format!("echo $$ > '{}'; exec sleep 30", pidfile.display());
        r.start(1, sh(&script), ParseKind::Files, None, Path::new("/"));
        let pid = wait_for_file(&pidfile);
        r.start(
            2,
            printf("x\\0"),
            ParseKind::SearchFiles { archives: false },
            None,
            Path::new("/"),
        );
        assert!(std::fs::metadata(format!("/proc/{pid}")).is_err());
        let (items, _) = until_done(&rx, 2);
        assert_eq!(keys(&items), ["x"]);
    }
}
