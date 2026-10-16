//! Runs a list's engine as a child process and streams its rows over a channel.
//!
//! The child gets a process group of its own, so cancelling also ends what it started (ugrep+ is a
//! shell script whose `ug` would outlive a killed shell). Its exit status is ignored: ugrep exits 1
//! when nothing matches and 2 as soon as one file is unreadable; whatever went wrong is on stderr,
//! and every stderr line becomes an `Error`.

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::commands::{Cmd, ParseKind, SEP};
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

    /// Cancels the running list and starts `cmd` in `root`; its messages carry `generation`.
    pub fn start(&mut self, generation: u64, cmd: Cmd, kind: ParseKind, root: &Path) {
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
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let child = Arc::new(Mutex::new((child, false)));

        let err_tx = self.tx.clone();
        let errors = thread::spawn(move || read_errors(generation, stderr, &err_tx));
        let (tx, job) = (self.tx.clone(), child.clone());
        thread::spawn(move || {
            read_items(generation, kind, stdout, &tx);
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

fn read_errors(generation: u64, stderr: impl Read, tx: &Sender<ListMsg>) {
    let mut shown = 0;
    let mut more = 0;
    let (mut reader, mut line) = (BufReader::new(stderr), Vec::new());
    while let Ok(Some(_)) = read_record(&mut reader, b'\n', &mut line, MAX_ERROR_LINE) {
        let line = String::from_utf8_lossy(&line).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if shown == MAX_ERRORS {
            more += 1;
        } else if tx.send(ListMsg::Error(generation, line)).is_ok() {
            shown += 1;
        } else {
            return;
        }
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
fn read_items(generation: u64, kind: ParseKind, stdout: impl Read, tx: &Sender<ListMsg>) {
    let delimiter = match kind {
        ParseKind::Files | ParseKind::Folders | ParseKind::SearchFiles { .. } => 0,
        ParseKind::SearchLines | ParseKind::Git => b'\n',
    };
    let streamed = matches!(kind, ParseKind::SearchFiles { .. } | ParseKind::SearchLines);
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    let (mut record, mut batch, mut records) = (Vec::new(), Vec::new(), Vec::new());
    let mut seen = HashSet::new();
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
        if let Some(item) = parse(kind, &record, 0) {
            if matches!(kind, ParseKind::SearchFiles { .. }) && !seen.insert(item.key.clone()) {
                continue;
            }
            batch.push(item);
        }
    }
    match kind {
        ParseKind::Git => batch = numbered(&records),
        ParseKind::Files => items::sort_files(&mut batch),
        ParseKind::Folders => {
            items::sort_folders(&mut batch);
            batch.insert(0, items::parent());
        }
        ParseKind::SearchFiles { .. } | ParseKind::SearchLines => {}
    }
    if !batch.is_empty() {
        let _ = tx.send(ListMsg::Items(generation, batch));
    }
}

/// Git rows numbered as sub-functions/git_logs does: the oldest commit 1, the newest the count.
fn numbered(records: &[Vec<u8>]) -> Vec<Item> {
    let mut number = records.iter().filter(|r| r.contains(&SEP)).count() as u32;
    records
        .iter()
        .filter_map(|r| {
            let commit = r.contains(&SEP);
            let item = parse(ParseKind::Git, r, number);
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
        r.start(1, cmd, kind, Path::new("/"));
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
        r.start(1, cmd, kind, root);
        until_done(&rx, 1)
    }

    #[test]
    fn files_are_sorted() {
        let (items, _) = run(printf("./b\\0./a/z\\0./B\\0./a\\0"), ParseKind::Files);
        assert_eq!(keys(&items), ["B", "a", "a/z", "b"]);
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
        r.start(1, sh(&script), ParseKind::Files, Path::new("/"));
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
        r.start(1, sh(&script), ParseKind::Files, Path::new("/"));
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
        r.start(1, sh(&script), ParseKind::Files, Path::new("/"));
        let pid = wait_for_file(&pidfile);
        r.start(
            2,
            printf("x\\0"),
            ParseKind::SearchFiles { archives: false },
            Path::new("/"),
        );
        assert!(std::fs::metadata(format!("/proc/{pid}")).is_err());
        let (items, _) = until_done(&rx, 2);
        assert_eq!(keys(&items), ["x"]);
    }
}
