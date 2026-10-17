//! Produces previews on a worker thread. Only the newest request counts: a newer one kills the
//! running preview's processes, and a result that a newer request overtook is dropped.

use std::fs::File;
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::commands::{Binary, Cmd, Header, Matches, PreviewPlan};
use crate::commands::{MATCH_END, MATCH_START};
use crate::items::{ansi_text, printable};
use crate::theme;

/// A preview's output is read up to this size.
const MAX_OUTPUT: usize = 4 << 20;
/// is_text looks at this many bytes ...
const SNIFF: usize = 8192;
/// ... and calls the file text when this share of them is printable, space or ≥ 0x80.
const MIN_PRINTABLE: f64 = 0.95;

#[derive(Debug)]
pub struct PreviewMsg {
    pub generation: u64,
    pub text: Text<'static>,
    /// Content's and Deep's pattern and how often it occurs in the file.
    pub found: Option<Found>,
}

/// A pattern and how often it occurs in the previewed file (None: the count failed).
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub pattern: String,
    pub count: Option<u64>,
}

pub struct Previewer {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    pending: Option<(u64, PreviewPlan)>,
    /// The process groups of the running preview (its count runs beside its matches), while their
    /// leaders are unreaped.
    running: Vec<u32>,
    closed: bool,
}

impl Previewer {
    pub fn new(tx: Sender<PreviewMsg>) -> Previewer {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        thread::spawn(move || work(&worker, &tx));
        Previewer { shared }
    }

    /// Replaces any pending request and ends the running one.
    pub fn request(&self, generation: u64, plan: PreviewPlan) {
        let mut st = lock(&self.shared.state);
        st.pending = Some((generation, plan));
        kill_all(&st.running);
        self.shared.wake.notify_one();
    }
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn work(shared: &Shared, tx: &Sender<PreviewMsg>) {
    loop {
        let (generation, plan) = {
            let mut st = lock(&shared.state);
            loop {
                if st.closed {
                    return;
                }
                if let Some(job) = st.pending.take() {
                    break job;
                }
                st = shared.wake.wait(st).unwrap_or_else(|e| e.into_inner());
            }
        };
        let (text, found) = produce(plan, shared);
        let st = lock(&shared.state);
        if st.closed {
            return;
        }
        if st.pending.is_some() {
            continue; // overtaken by a newer request
        }
        drop(st);
        if tx
            .send(PreviewMsg {
                generation,
                text,
                found,
            })
            .is_err()
        {
            return;
        }
    }
}

/// The preview, and with a pattern how often it occurs.
fn produce(plan: PreviewPlan, shared: &Shared) -> (Text<'static>, Option<Found>) {
    match plan {
        PreviewPlan::Sniff {
            path,
            text,
            binary,
            header,
        } => {
            let is_text = is_text(&path);
            with_header(header, matches!(is_text, Ok(true)), shared, || {
                sniffed(&path, &is_text, text, binary, shared).lines
            })
        }
        PreviewPlan::Numbered {
            path,
            lines,
            binary,
            header,
        } => {
            let is_text = is_text(&path);
            with_header(Some(header), matches!(is_text, Ok(true)), shared, || {
                numbered(&path, &is_text, lines, binary, shared)
            })
        }
        other => (unsniffed(other, shared), None),
    }
}

/// The lines of `body`, and the header's pattern with its count; the count runs beside the body's
/// commands (a big archive takes seconds for each).
fn with_header(
    header: Option<Header>,
    text: bool,
    shared: &Shared,
    body: impl FnOnce() -> Vec<Line<'static>>,
) -> (Text<'static>, Option<Found>) {
    thread::scope(|s| {
        let count = header
            .and_then(|h| h.matches)
            .map(|m| s.spawn(move || found(m, text, shared)));
        let body = body();
        let found = count.map(|c| c.join().unwrap_or_else(|e| std::panic::resume_unwind(e)));
        (Text::from(body), found)
    })
}

/// The pattern and how often `ugrep -c -o` counts it in a text file (`text`) or any other.
fn found(m: Matches, text: bool, shared: &Shared) -> Found {
    let count = run(&[if text { m.text } else { m.binary }], shared)
        .ok()
        .and_then(|out| count_of(&out.stdout));
    Found {
        pattern: m.pattern,
        count,
    }
}

/// The total of `ugrep -c -o`: one count per line, after the last `:` when there is a name (archive
/// members); `None` when the output is not that.
fn count_of(stdout: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(stdout).ok()?;
    let mut total = 0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        total += line.rsplit(':').next()?.parse::<u64>().ok()?;
    }
    Some(total)
}

fn unsniffed(plan: PreviewPlan, shared: &Shared) -> Text<'static> {
    match plan {
        PreviewPlan::Nothing => Text::default(),
        PreviewPlan::Text(s) => dim(&s),
        PreviewPlan::Run(cmd) => shown(run(&[cmd], shared)),
        PreviewPlan::Pipe(first, second) => shown(run(&[first, second], shared)),
        PreviewPlan::Sniff { .. } | PreviewPlan::Numbered { .. } => {
            unreachable!("handled by produce")
        }
    }
}

fn sniffed(
    path: &Path,
    is_text: &io::Result<bool>,
    text: Option<Cmd>,
    binary: Binary,
    shared: &Shared,
) -> Text<'static> {
    match is_text {
        Err(e) => red_lines(&format!("{}: {e}", path.display())),
        Ok(true) => match text {
            Some(cmd) => shown(run(&[cmd], shared)),
            None => file_text(path),
        },
        Ok(false) => match binary {
            Binary::Show(cmd) => shown(run(&[cmd], shared)),
            Binary::Describe(None) => dim("No text preview: binary file"),
            Binary::Describe(Some(cmd)) => match run(&[cmd], shared) {
                Ok(out) => {
                    let what = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let what = if what.is_empty() {
                        "binary file".to_string()
                    } else {
                        what
                    };
                    dim(&format!("No text preview: {what}"))
                }
                Err(e) => red_lines(&e),
            },
        },
    }
}

/// Content's matches under "Match #N": a text file's with lines around them, any other's as `binary`
/// shows them.
fn numbered(
    path: &Path,
    is_text: &io::Result<bool>,
    lines: Cmd,
    binary: Cmd,
    shared: &Shared,
) -> Vec<Line<'static>> {
    match is_text {
        Err(e) => red_lines(&format!("{}: {e}", path.display())).lines,
        Ok(true) => {
            let out = match run(&[lines], shared) {
                Ok(out) => out,
                Err(e) => return red_lines(&e).lines,
            };
            let mut file = match read_file(path) {
                Ok(file) => file,
                Err(e) => return red_lines(&format!("{}: {e}", path.display())).lines,
            };
            if file.len() == MAX_OUTPUT {
                // only whole lines around the matches
                let end = file.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
                file.truncate(end);
            }
            let mut shown = numbered_blocks(&out.stdout, &file);
            shown.extend(notes(&out));
            shown
        }
        Ok(false) => match run_limited(&[binary], shared, Some(MAX_BLOCKS)) {
            Ok(out) => {
                let stdout = out.stdout.strip_suffix(b"\n").unwrap_or(&out.stdout);
                let mut shown = if stdout.is_empty() {
                    Vec::new()
                } else {
                    numbered_lines(ansi_text(stdout))
                };
                shown.extend(notes(&out));
                shown
            }
            Err(e) => red_lines(&e).lines,
        },
    }
}

struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    cut: Option<Cut>,
}

/// Why an output was not read to its end.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cut {
    /// MAX_OUTPUT bytes read.
    Bytes,
    /// The lines asked for read (a binary's first MAX_BLOCKS matches).
    Lines,
}

/// Runs `cmds` as a pipeline in a process group of its own and collects the last one's output.
fn run(cmds: &[Cmd], shared: &Shared) -> Result<Output, String> {
    run_limited(cmds, shared, None)
}

/// `run`, reading at most `max_lines` lines; the pipeline is ended when there is more.
fn run_limited(cmds: &[Cmd], shared: &Shared, max_lines: Option<usize>) -> Result<Output, String> {
    let mut children: Vec<Child> = Vec::new();
    let mut errors = Vec::new();
    let mut stdout: Option<ChildStdout> = None;
    let mut pgid = 0;
    for cmd in cmds {
        let stdin = stdout.take().map_or_else(Stdio::null, Stdio::from);
        let spawned = Command::new(&cmd.program)
            .args(&cmd.args)
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(pgid as i32)
            .spawn();
        match spawned {
            Ok(mut child) => {
                if pgid == 0 {
                    pgid = child.id();
                }
                let mut err = child.stderr.take().expect("stderr is piped");
                errors.push(thread::spawn(move || {
                    read_capped(&mut err, 64 << 10, None).0
                }));
                stdout = child.stdout.take();
                children.push(child);
            }
            Err(e) => {
                kill_group(Some(pgid).filter(|&g| g != 0));
                for mut c in children {
                    let _ = c.wait();
                }
                return Err(format!("{}: {e}", cmd.program.display()));
            }
        }
    }
    {
        let mut st = lock(&shared.state);
        if st.pending.is_some() || st.closed {
            kill_group(Some(pgid)); // a newer request came while this one started
        } else {
            st.running.push(pgid);
        }
    }
    let (out, cut) = read_capped(
        &mut stdout.expect("a pipeline has a last command"),
        MAX_OUTPUT,
        max_lines,
    );
    if cut.is_some() {
        kill_group(Some(pgid));
    }
    lock(&shared.state).running.retain(|&g| g != pgid);
    for mut c in children {
        let _ = c.wait();
    }
    let stderr = errors
        .into_iter()
        .flat_map(|t| t.join().unwrap_or_default())
        .collect();
    Ok(Output {
        stdout: out,
        stderr,
        cut,
    })
}

/// Reads up to `max` bytes and `max_lines` lines; says why when there was more.
fn read_capped(r: &mut impl Read, max: usize, max_lines: Option<usize>) -> (Vec<u8>, Option<Cut>) {
    let mut out = Vec::new();
    let mut lines = 0;
    let mut buf = [0u8; 64 << 10];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => return (out, None),
            Ok(n) => {
                let mut chunk = &buf[..n];
                let mut cut = None;
                if let Some(limit) = max_lines {
                    if lines == limit {
                        return (out, Some(Cut::Lines)); // bytes after the last line wanted
                    }
                    let newlines = chunk.iter().enumerate().filter(|&(_, &b)| b == b'\n');
                    match newlines.map(|(i, _)| i).nth(limit - lines - 1) {
                        Some(end) => {
                            lines = limit;
                            if end + 1 < chunk.len() {
                                cut = Some(Cut::Lines);
                            }
                            chunk = &chunk[..=end];
                        }
                        None => lines += chunk.iter().filter(|&&b| b == b'\n').count(),
                    }
                }
                let room = max - out.len();
                out.extend_from_slice(&chunk[..chunk.len().min(room)]);
                if chunk.len() >= room {
                    return (out, Some(Cut::Bytes));
                }
                if cut.is_some() {
                    return (out, cut);
                }
            }
        }
    }
}

fn shown(result: Result<Output, String>) -> Text<'static> {
    let out = match result {
        Ok(out) => out,
        Err(e) => return red_lines(&e),
    };
    let stdout = out.stdout.strip_suffix(b"\n").unwrap_or(&out.stdout);
    let mut text = if stdout.is_empty() {
        Text::default()
    } else {
        ansi_text(stdout)
    };
    text.lines.extend(notes(&out));
    text
}

/// What goes under an output: that it was cut, and its errors in red.
fn notes(out: &Output) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    match out.cut {
        Some(Cut::Bytes) => lines.push(dim_line("… preview cut at 4 MiB")),
        Some(Cut::Lines) => lines.push(dim_line(&format!(
            "… only the first {MAX_BLOCKS} matches are shown"
        ))),
        None => {}
    }
    lines.extend(red_lines(&String::from_utf8_lossy(&out.stderr)).lines);
    lines
}

/// A text file as it is, for when bat is missing: no colours, nothing interpreted.
fn file_text(path: &Path) -> Text<'static> {
    let bytes = match read_file(path) {
        Ok(v) => v,
        Err(e) => return red_lines(&format!("{}: {e}", path.display())),
    };
    let s = String::from_utf8_lossy(&bytes);
    let s = s.strip_suffix('\n').unwrap_or(&s);
    Text::from(
        s.split('\n')
            .map(|l| {
                Line::raw(printable(
                    &l.strip_suffix('\r').unwrap_or(l).replace('\t', "    "),
                ))
            })
            .collect::<Vec<_>>(),
    )
}

/// The file's first MAX_OUTPUT bytes.
fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut v = Vec::new();
    File::open(path)?
        .take(MAX_OUTPUT as u64)
        .read_to_end(&mut v)?;
    Ok(v)
}

fn dim(s: &str) -> Text<'static> {
    let style = Style::new().fg(theme::DIM).add_modifier(Modifier::ITALIC);
    Text::from(
        s.lines()
            .map(|l| Line::from(Span::styled(printable(l), style)))
            .collect::<Vec<_>>(),
    )
}

fn red_lines(s: &str) -> Text<'static> {
    let style = Style::new().fg(theme::RED);
    Text::from(
        s.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| Line::from(Span::styled(printable(l), style)))
            .collect::<Vec<_>>(),
    )
}

impl Drop for Previewer {
    fn drop(&mut self) {
        let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        st.closed = true;
        kill_all(&st.running);
        self.shared.wake.notify_one();
    }
}

fn kill_all(groups: &[u32]) {
    for &g in groups {
        kill_group(Some(g));
    }
}

fn kill_group(pgid: Option<u32>) {
    if let Some(pgid) = pgid {
        // SAFETY: kill(2) on a process group whose leader is our unreaped child (see State::running).
        unsafe { libc::kill(-(pgid as libc::pid_t), libc::SIGKILL) };
    }
}

/// The matching lines of `stdout` (ugrep's records), each with lines around it from `file`, under
/// "Match #N".
fn numbered_blocks(stdout: &[u8], file: &[u8]) -> Vec<Line<'static>> {
    let file = file.strip_suffix(b"\n").unwrap_or(file);
    let file_lines: Vec<&[u8]> = file.split(|&b| b == b'\n').collect();
    let records: Vec<(usize, &[u8])> = stdout
        .split(|&b| b == b'\n')
        .filter_map(|record| {
            // path NUL line 0x1f text
            let rest = &record[record.iter().position(|&b| b == 0)? + 1..];
            let sep = rest.iter().position(|&b| b == 0x1f)?;
            let line = std::str::from_utf8(&rest[..sep]).ok()?.parse().ok()?;
            Some((line, &rest[sep + 1..]))
        })
        .collect();
    let last = records.iter().map(|(n, _)| n + AROUND).max().unwrap_or(0);
    let width = (last.to_string().len() + 1).max(4);
    let mut lines = Vec::new();
    let mut number = 1;
    for (i, &(line, text)) in records.iter().enumerate() {
        if i == MAX_BLOCKS {
            lines.push(dim_line(&format!(
                "… and {} more matching lines",
                records.len() - MAX_BLOCKS
            )));
            break;
        }
        if i > 0 {
            lines.push(rule());
        }
        let count = text.iter().filter(|&&b| b == MATCH_START).count().max(1);
        let label = if count == 1 {
            format!("Match #{number}")
        } else {
            format!("Match #{number}–{}", number + count - 1)
        };
        number += count;
        lines.push(Line::from(Span::styled(
            label,
            Style::new().fg(theme::GREEN).add_modifier(Modifier::BOLD),
        )));
        let numbered = |n: usize, mark: &'static str| {
            vec![
                Span::styled(format!("{n:>width$}"), Style::new().fg(theme::DIM)),
                Span::styled(mark, Style::new().fg(theme::FAINT)),
            ]
        };
        for n in line.saturating_sub(AROUND).max(1)..line {
            let Some(&content) = file_lines.get(n - 1) else {
                break;
            };
            let mut spans = numbered(n, "  ");
            spans.push(Span::raw(shown_line(content)));
            lines.push(Line::from(spans));
        }
        let mut spans = numbered(line, ": ");
        spans.extend(marked_spans(text));
        lines.push(Line::from(spans));
        for n in line + 1..=line + AROUND {
            let Some(&content) = file_lines.get(n - 1) else {
                break;
            };
            let mut spans = numbered(n, "  ");
            spans.push(Span::raw(shown_line(content)));
            lines.push(Line::from(spans));
        }
    }
    lines
}

/// Each line of `text` (one match a line) under "Match #N".
fn numbered_lines(text: Text<'static>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, line) in text.lines.into_iter().enumerate() {
        if i > 0 {
            lines.push(rule());
        }
        lines.push(Line::from(Span::styled(
            format!("Match #{}", i + 1),
            Style::new().fg(theme::GREEN).add_modifier(Modifier::BOLD),
        )));
        lines.push(line);
    }
    lines
}

/// Lines around each numbered match.
const AROUND: usize = 2;
/// Numbered blocks shown at most; a file with more says how many it left out.
const MAX_BLOCKS: usize = 300;
/// A line's characters shown at most (a minified file's line would fill the preview).
const MAX_LINE: usize = 1000;

fn rule() -> Line<'static> {
    Line::from(Span::styled("─".repeat(40), Style::new().fg(theme::FAINT)))
}

fn dim_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), Style::new().fg(theme::DIM)))
}

/// A line of the file as text: tabs as spaces, no carriage return at its end, other control
/// characters shown, cut at MAX_LINE characters.
fn shown_line(bytes: &[u8]) -> String {
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    let text = printable(&String::from_utf8_lossy(bytes).replace('\t', "    "));
    match text.char_indices().nth(MAX_LINE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

/// A matching line's text with its matches (between MATCH_START and MATCH_END) red.
fn marked_spans(text: &[u8]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let (mut in_match, mut piece) = (false, Vec::new());
    let mut flush = |piece: &mut Vec<u8>, in_match: bool| {
        if piece.is_empty() {
            return;
        }
        let style = if in_match {
            Style::new().fg(theme::RED).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::TEXT)
        };
        spans.push(Span::styled(shown_line(piece), style));
        piece.clear();
    };
    for &b in text {
        match b {
            MATCH_START => {
                flush(&mut piece, in_match);
                in_match = true;
            }
            MATCH_END => {
                flush(&mut piece, in_match);
                in_match = false;
            }
            b => piece.push(b),
        }
    }
    flush(&mut piece, in_match);
    spans
}

/// A port of sub-functions/is_text_file.c: the first 8 KiB hold no NUL and at least 95 % of them are
/// printable ASCII, white space or ≥ 0x80 (UTF-8). An empty file is no text.
pub fn is_text(path: &Path) -> io::Result<bool> {
    let mut head = Vec::with_capacity(SNIFF);
    File::open(path)?
        .take(SNIFF as u64)
        .read_to_end(&mut head)?;
    if head.is_empty() || head.contains(&0) {
        return Ok(false);
    }
    let printable = head
        .iter()
        .filter(|&&b| b >= 0x80 || (0x20..=0x7e).contains(&b) || b"\t\n\x0b\x0c\r".contains(&b))
        .count();
    Ok(printable as f64 / head.len() as f64 >= MIN_PRINTABLE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    fn sh(script: &str) -> Cmd {
        Cmd::new("/bin/sh").args(["-c", script])
    }

    fn texts(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn matches_are_numbered_blocks_with_two_lines_around() {
        let file = b"a\nb needle\nc\nd\ne needle needle\nf\ng\n";
        // ugrep's records: path NUL line 0x1f text, every match between 0x01 and 0x02
        let out = b"x\x002\x1fb \x01needle\x02\nx\x005\x1fe \x01needle\x02 \x01needle\x02\n";
        let lines = numbered_blocks(out, file);
        let rule = "─".repeat(40);
        assert_eq!(
            texts(&lines),
            [
                "Match #1",
                "   1  a",
                "   2: b needle",
                "   3  c",
                "   4  d",
                &rule,
                "Match #2–3",
                "   3  c",
                "   4  d",
                "   5: e needle needle",
                "   6  f",
                "   7  g",
            ]
        );
        let red = lines[8]
            .spans
            .iter()
            .chain(&lines[9].spans)
            .filter(|s| s.content == "needle" && s.style.fg == Some(theme::RED))
            .count();
        assert_eq!(red, 2, "both matches of line 5 are red, nothing else");
    }

    #[test]
    fn numbered_lines_show_tabs_as_spaces_and_drop_carriage_returns() {
        let file = b"\ta\r\n\tb needle\r\nc\r\n";
        let out = b"x\x002\x1f\tb \x01needle\x02\r\n";
        assert_eq!(
            texts(&numbered_blocks(out, file)),
            ["Match #1", "   1      a", "   2:     b needle", "   3  c"]
        );
    }

    #[test]
    fn the_last_line_is_context_without_a_final_newline() {
        let out = b"x\x001\x1f\x01needle\x02\n";
        assert_eq!(
            texts(&numbered_blocks(out, b"needle\nlast")),
            ["Match #1", "   1: needle", "   2  last"]
        );
    }

    #[test]
    fn a_flood_of_matches_is_cut() {
        let file: Vec<u8> = (0..400)
            .flat_map(|i| format!("x{i}\n").into_bytes())
            .collect();
        let out: Vec<u8> = (1..=400)
            .flat_map(|i| format!("f\0{i}\x1f\x01x\x02{}\n", i - 1).into_bytes())
            .collect();
        let lines = numbered_blocks(&out, &file);
        let last: String = texts(&lines).pop().unwrap();
        assert!(last.contains("100 more"), "{last}");
    }

    #[test]
    fn binary_matches_are_numbered_too() {
        let text = Text::from(vec![Line::from("x needle y"), Line::from("z needle")]);
        let rule = "─".repeat(40);
        assert_eq!(
            texts(&numbered_lines(text)),
            ["Match #1", "x needle y", &rule, "Match #2", "z needle"]
        );
    }

    fn plain(text: &Text) -> String {
        text.lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn one(plan: PreviewPlan) -> Text<'static> {
        let (tx, rx) = channel();
        let p = Previewer::new(tx);
        p.request(1, plan);
        let msg = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("no preview within 5 s");
        assert_eq!(msg.generation, 1);
        msg.text
    }

    fn file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = temp_dir("preview_files").join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn is_text_cases() {
        assert!(is_text(&file("t.txt", b"hello\nworld\n")).unwrap());
        assert!(is_text(&file("tr.txt", "çay, şeker, ğüıö\n".as_bytes())).unwrap());
        assert!(!is_text(&file("nul.bin", b"ab\0cd")).unwrap());
        assert!(!is_text(&file("empty", b"")).unwrap());
        assert!(
            !is_text(&file(
                "ctrl.bin",
                &[1u8, 2, 3, 4, b'a', b'b', b'c', b'd', b'e', b'f']
            ))
            .unwrap()
        );
        assert!(is_text(&missing_dir().join("x")).is_err());
    }

    fn missing_dir() -> std::path::PathBuf {
        temp_dir("preview_missing").join("not-there")
    }

    #[test]
    fn ansi_becomes_styled_spans() {
        let t = one(PreviewPlan::Run(
            Cmd::new("/usr/bin/printf").arg("\\033[31mred\\033[0m plain"),
        ));
        assert_eq!(plain(&t), "red plain");
        assert_eq!(
            t.lines[0].spans[0].style.fg,
            Some(ratatui::style::Color::Red)
        );
    }

    #[test]
    fn pipe_plan_feeds_second_command() {
        let t = one(PreviewPlan::Pipe(
            Cmd::new("/usr/bin/printf").arg("a"),
            Cmd::new("/usr/bin/tr").args(["a", "b"]),
        ));
        assert_eq!(plain(&t), "b");
    }

    #[test]
    fn failing_command_shows_its_message() {
        let t = one(PreviewPlan::Run(sh("echo out; echo bad >&2; exit 1")));
        assert_eq!(plain(&t), "out\nbad");
        assert_eq!(t.lines[1].spans[0].style.fg, Some(theme::RED));
    }

    #[test]
    fn missing_program_is_said() {
        let t = one(PreviewPlan::Run(Cmd::new("/nonexistent/bat")));
        assert!(plain(&t).starts_with("/nonexistent/bat: "), "{}", plain(&t));
    }

    #[test]
    fn sniff_runs_the_text_command_for_text() {
        let path = file("s.txt", b"text\n");
        let t = one(PreviewPlan::Sniff {
            path: path.clone(),
            text: Some(Cmd::new("/usr/bin/printf").arg("TEXT")),
            binary: Binary::Show(Cmd::new("/usr/bin/printf").arg("BIN")),
            header: None,
        });
        assert_eq!(plain(&t), "TEXT");
    }

    #[test]
    fn sniff_describes_a_binary() {
        let path = file("s.bin", b"\0\x01");
        let t = one(PreviewPlan::Sniff {
            path: path.clone(),
            text: None,
            binary: Binary::Describe(Some(Cmd::new("/usr/bin/printf").arg("data\\n"))),
            header: None,
        });
        assert_eq!(plain(&t), "No text preview: data");
        let t = one(PreviewPlan::Sniff {
            path,
            text: None,
            binary: Binary::Describe(None),
            header: None,
        });
        assert_eq!(plain(&t), "No text preview: binary file");
    }

    fn printf(s: &str) -> Cmd {
        Cmd::new("/usr/bin/printf").arg(s)
    }

    fn header(count: Option<Cmd>) -> Option<Header> {
        Some(Header {
            matches: count.map(|c| Matches {
                pattern: "ne".into(),
                text: c.clone(),
                binary: c,
            }),
        })
    }

    fn msg(plan: PreviewPlan) -> PreviewMsg {
        let (tx, rx) = channel();
        let p = Previewer::new(tx);
        p.request(1, plan);
        rx.recv_timeout(Duration::from_secs(5))
            .expect("no preview within 5 s")
    }

    fn found(count: Option<u64>) -> Option<Found> {
        Some(Found {
            pattern: "ne".into(),
            count,
        })
    }

    #[test]
    fn the_pattern_and_its_count_come_apart_from_the_text() {
        // drawn at the preview's top right, where they stay while it scrolls (Bilal)
        let path = file("h.txt", b"text\n");
        let m = msg(PreviewPlan::Sniff {
            path,
            text: Some(printf("BODY")),
            binary: Binary::Describe(None),
            header: header(Some(printf("3\\n"))),
        });
        assert_eq!(plain(&m.text), "BODY", "no header lines above the file");
        assert_eq!(m.found, found(Some(3)));
    }

    #[test]
    fn numbered_text_file_shows_the_header_then_the_blocks() {
        let path = file("n.txt", b"a\nb needle\nc\n");
        let m = msg(PreviewPlan::Numbered {
            path,
            // octal: \062 is "2", so the line number is not read as part of the NUL's digits
            lines: sh("printf 'n.txt\\000\\062\\037b \\001needle\\002\\n'; echo oops >&2"),
            binary: printf("never"),
            header: header(Some(printf("1\\n"))).unwrap(),
        });
        assert_eq!(
            plain(&m.text),
            "Match #1\n   1  a\n   2: b needle\n   3  c\noops"
        );
        assert_eq!(m.found, found(Some(1)));
    }

    #[test]
    fn numbered_binary_file_numbers_each_line_of_its_output() {
        let path = file("n.bin", b"x\0needle\0");
        let m = msg(PreviewPlan::Numbered {
            path,
            lines: printf("never"),
            binary: sh("printf 'x needle\\nneedle y\\n'; echo oops >&2"),
            header: header(None).unwrap(),
        });
        let rule = "─".repeat(40);
        assert_eq!(
            plain(&m.text),
            format!("Match #1\nx needle\n{rule}\nMatch #2\nneedle y\noops")
        );
        assert_eq!(m.found, None);
    }

    #[test]
    fn numbered_without_the_engine_says_so() {
        let path = file("n2.txt", b"needle\n");
        let m = msg(PreviewPlan::Numbered {
            path,
            lines: Cmd::new("/nonexistent/ugrep"),
            binary: printf("never"),
            header: header(None).unwrap(),
        });
        assert!(
            plain(&m.text).contains("/nonexistent/ugrep: No such file"),
            "{}",
            plain(&m.text)
        );
    }

    #[test]
    fn without_a_pattern_nothing_is_found() {
        let path = file("h2.txt", b"text\n");
        let m = msg(PreviewPlan::Sniff {
            path,
            text: Some(printf("BODY")),
            binary: Binary::Describe(None),
            header: header(None),
        });
        assert_eq!(plain(&m.text), "BODY");
        assert_eq!(m.found, None);
    }

    #[test]
    fn count_adds_up_archive_members() {
        let path = file("h.zip", b"PK\x03\x04\0\0");
        let m = msg(PreviewPlan::Sniff {
            path,
            text: None,
            binary: Binary::Show(printf("BIN")),
            header: header(Some(printf("h.zip{a}:1\\nh.zip{b:c}:2\\n"))),
        });
        assert_eq!(m.found, found(Some(3)));
    }

    #[test]
    fn count_that_fails_shows_a_question_mark() {
        let path = file("h3.txt", b"text\n");
        let m = msg(PreviewPlan::Sniff {
            path,
            text: Some(printf("BODY")),
            binary: Binary::Describe(None),
            header: header(Some(Cmd::new("/nonexistent/ugrep"))),
        });
        assert_eq!(m.found, found(None), "the pattern, its count unknown");
    }

    #[test]
    fn sniff_shows_a_text_file_raw_without_bat() {
        let path = file(
            "raw.txt",
            b"a\x1b[31mb\tc, enough plain text to stay above 95 %\r\n2\n",
        );
        let t = one(PreviewPlan::Sniff {
            path,
            text: None,
            binary: Binary::Describe(None),
            header: None,
        });
        assert_eq!(
            plain(&t),
            "a\u{fffd}[31mb    c, enough plain text to stay above 95 %\n2"
        );
    }

    #[test]
    fn sniff_of_unreadable_path_says_why() {
        let t = one(PreviewPlan::Sniff {
            path: missing_dir(),
            text: None,
            binary: Binary::Describe(None),
            header: None,
        });
        assert!(plain(&t).contains("not-there"), "{}", plain(&t));
    }

    #[test]
    fn text_and_nothing_plans() {
        assert_eq!(
            plain(&one(PreviewPlan::Text("ls is not installed".into()))),
            "ls is not installed"
        );
        assert_eq!(plain(&one(PreviewPlan::Nothing)), "");
    }

    #[test]
    fn output_is_capped() {
        let t0 = Instant::now();
        let t = one(PreviewPlan::Run(
            Cmd::new("/usr/bin/yes").arg("x".repeat(499)),
        ));
        assert!(t0.elapsed() < Duration::from_secs(5));
        let chars: usize = t.lines.iter().map(|l| l.width() + 1).sum();
        assert!(
            chars >= MAX_OUTPUT - 1000 && chars < MAX_OUTPUT + 1000,
            "{chars}"
        );
        assert!(plain(&t).ends_with("… preview cut at 4 MiB"));
    }

    fn drain(rx: &Receiver<PreviewMsg>, wait: Duration) -> Vec<u64> {
        let mut got = Vec::new();
        let end = Instant::now() + wait;
        while let Ok(msg) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            got.push(msg.generation);
        }
        got
    }

    #[test]
    fn newest_request_wins() {
        let (tx, rx) = channel();
        let p = Previewer::new(tx);
        for g in 1..=3 {
            p.request(g, PreviewPlan::Run(sh(&format!("sleep 0.3; echo {g}"))));
        }
        assert_eq!(drain(&rx, Duration::from_millis(1500)), [3]);
    }

    #[test]
    fn a_binary_shows_its_first_matches_and_stops_reading() {
        // the engine would go on for 30 s: the preview stops reading after MAX_BLOCKS matches
        let path = file("many.bin", b"x\0needle\0");
        let t = Instant::now();
        let m = msg(PreviewPlan::Numbered {
            path,
            lines: printf("never"),
            binary: sh("seq 400 | sed 's/^/x needle /'; exec sleep 30"),
            header: header(None).unwrap(),
        });
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
        let shown = plain(&m.text);
        assert_eq!(shown.matches("Match #").count(), MAX_BLOCKS, "{shown}");
        assert!(
            shown.ends_with("… only the first 300 matches are shown"),
            "{shown}"
        );
    }

    #[test]
    fn a_line_limit_is_cut_only_when_there_is_more() {
        let read = |bytes: &[u8]| read_capped(&mut &bytes[..], 100, Some(2));
        assert_eq!(read(b"a\nb\n"), (b"a\nb\n".to_vec(), None));
        assert_eq!(read(b"a\nb"), (b"a\nb".to_vec(), None));
        assert_eq!(read(b"a\nb\nc\n"), (b"a\nb\n".to_vec(), Some(Cut::Lines)));
        assert_eq!(
            read_capped(&mut &b"abcdef"[..], 4, Some(2)),
            (b"abcd".to_vec(), Some(Cut::Bytes))
        );
    }

    #[test]
    fn the_count_runs_beside_the_matches() {
        let path = file("both.txt", b"a\nneedle\n");
        let t = Instant::now();
        let m = msg(PreviewPlan::Numbered {
            path,
            lines: sh("sleep 1; printf 'both.txt\\000\\062\\037\\001needle\\002\\n'"),
            binary: printf("never"),
            header: header(Some(sh("sleep 1; echo 1"))).unwrap(),
        });
        assert!(
            t.elapsed() < Duration::from_millis(1800),
            "{:?}",
            t.elapsed()
        );
        assert!(plain(&m.text).starts_with("Match #1"), "{}", plain(&m.text));
        assert_eq!(m.found, found(Some(1)));
    }

    #[test]
    fn newer_request_ends_the_count_and_the_matches() {
        let dir = temp_dir("preview_kill_both");
        let (count_pid, lines_pid) = (dir.join("count"), dir.join("lines"));
        let (tx, rx) = channel();
        let p = Previewer::new(tx);
        let waits = |pidfile: &std::path::Path| {
            sh(&format!("echo $$ > '{}'; exec sleep 30", pidfile.display()))
        };
        p.request(
            1,
            PreviewPlan::Numbered {
                path: file("kill.txt", b"needle\n"),
                lines: waits(&lines_pid),
                binary: printf("never"),
                header: header(Some(waits(&count_pid))).unwrap(),
            },
        );
        let t = Instant::now();
        let pid = |f: &std::path::Path| loop {
            if let Ok(s) = std::fs::read_to_string(f)
                && s.ends_with('\n')
            {
                break s.trim().to_string();
            }
            assert!(t.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(10));
        };
        let pids = [pid(&count_pid), pid(&lines_pid)];
        p.request(2, PreviewPlan::Text("two".into()));
        let msg = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second preview not answered in 1 s");
        assert_eq!(msg.generation, 2);
        for pid in pids {
            assert!(
                std::fs::metadata(format!("/proc/{pid}")).is_err(),
                "process {pid} still there"
            );
        }
    }

    #[test]
    fn newer_request_kills_the_running_preview() {
        let pidfile = temp_dir("preview_kill").join("pid");
        let (tx, rx) = channel();
        let p = Previewer::new(tx);
        p.request(
            1,
            PreviewPlan::Run(sh(&format!(
                "echo $$ > '{}'; exec sleep 30",
                pidfile.display()
            ))),
        );
        let t = Instant::now();
        let pid = loop {
            if let Ok(s) = std::fs::read_to_string(&pidfile) {
                if s.ends_with('\n') {
                    break s.trim().to_string();
                }
            }
            assert!(t.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(10));
        };
        p.request(2, PreviewPlan::Text("two".into()));
        let msg = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second preview not answered in 1 s");
        assert_eq!(msg.generation, 2);
        assert!(
            std::fs::metadata(format!("/proc/{pid}")).is_err(),
            "preview process {pid} still there"
        );
    }
}
