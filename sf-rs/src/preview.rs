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

use crate::commands::{Binary, Cmd, Header, PreviewPlan};
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
    /// Lines of `text` above the file's own (the Content header), for scrolling to a line.
    pub header_lines: usize,
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
    /// The process group of the running preview, while its leader is unreaped.
    running: Option<u32>,
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
        kill_group(st.running);
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
        let (text, header_lines) = produce(plan, shared);
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
                header_lines,
            })
            .is_err()
        {
            return;
        }
    }
}

/// The preview and how many of its lines are a header above the file's own.
fn produce(plan: PreviewPlan, shared: &Shared) -> (Text<'static>, usize) {
    match plan {
        PreviewPlan::Sniff {
            path,
            text,
            binary,
            header,
        } => {
            let is_text = is_text(&path);
            let body = sniffed(&path, &is_text, text, binary, shared);
            let Some(header) = header else {
                return (body, 0);
            };
            let mut lines = header_lines(header, matches!(is_text, Ok(true)), shared);
            let n = lines.len();
            lines.extend(body.lines);
            (Text::from(lines), n)
        }
        other => (unsniffed(other, shared), 0),
    }
}

/// `File  path`, with a pattern `Pattern  p` and `Matches  n`, then an empty line.
fn header_lines(header: Header, text: bool, shared: &Shared) -> Vec<Line<'static>> {
    let label = |l: &str| Span::styled(format!("{l:<9}"), Style::new().fg(theme::DIM));
    let mut file = vec![label("File")];
    file.extend(crate::items::path_spans(&header.file));
    let mut lines = vec![Line::from(file)];
    if let Some(m) = header.matches {
        let count = run(&[if text { m.text } else { m.binary }], shared)
            .ok()
            .and_then(|out| count_of(&out.stdout));
        lines.push(Line::from(vec![
            label("Pattern"),
            Span::styled(printable(&m.pattern), Style::new().fg(theme::RED)),
        ]));
        lines.push(Line::from(vec![
            label("Matches"),
            Span::styled(
                count.map_or_else(|| "?".to_string(), |n| n.to_string()),
                Style::new().fg(theme::PEACH),
            ),
        ]));
    }
    lines.push(Line::default());
    lines
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
        PreviewPlan::Sniff { .. } => unreachable!("handled by produce"),
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

struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    capped: bool,
}

/// Runs `cmds` as a pipeline in a process group of its own and collects the last one's output.
fn run(cmds: &[Cmd], shared: &Shared) -> Result<Output, String> {
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
                errors.push(thread::spawn(move || read_capped(&mut err, 64 << 10).0));
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
            st.running = Some(pgid);
        }
    }
    let (out, capped) = read_capped(
        &mut stdout.expect("a pipeline has a last command"),
        MAX_OUTPUT,
    );
    if capped {
        kill_group(Some(pgid));
    }
    lock(&shared.state).running = None;
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
        capped,
    })
}

/// Reads up to `max` bytes, then (when not capped) on to the end so the writer never blocks.
fn read_capped(r: &mut impl Read, max: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = [0u8; 64 << 10];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => return (out, false),
            Ok(n) => {
                let room = max - out.len();
                out.extend_from_slice(&buf[..n.min(room)]);
                if n >= room {
                    return (out, true);
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
    if out.capped {
        text.lines.push(Line::from(Span::styled(
            "… preview cut at 4 MiB",
            Style::new().fg(theme::DIM),
        )));
    }
    text.lines
        .extend(red_lines(&String::from_utf8_lossy(&out.stderr)).lines);
    text
}

/// A text file as it is, for when bat is missing: no colours, nothing interpreted.
fn file_text(path: &Path) -> Text<'static> {
    let bytes = match File::open(path).and_then(|f| {
        let mut v = Vec::new();
        f.take(MAX_OUTPUT as u64).read_to_end(&mut v).map(|_| v)
    }) {
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
        kill_group(st.running);
        self.shared.wake.notify_one();
    }
}

fn kill_group(pgid: Option<u32>) {
    if let Some(pgid) = pgid {
        // SAFETY: kill(2) on a process group whose leader is our unreaped child (see State::running).
        unsafe { libc::kill(-(pgid as libc::pid_t), libc::SIGKILL) };
    }
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
    use crate::commands::Matches;
    use crate::testutil::temp_dir;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    fn sh(script: &str) -> Cmd {
        Cmd::new("/bin/sh").args(["-c", script])
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

    fn header(file: &str, count: Option<Cmd>) -> Option<Header> {
        Some(Header {
            file: file.into(),
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

    #[test]
    fn header_shows_file_pattern_and_count() {
        let path = file("h.txt", b"text\n");
        let m = msg(PreviewPlan::Sniff {
            path,
            text: Some(printf("BODY")),
            binary: Binary::Describe(None),
            header: header("src/h.txt", Some(printf("3\\n"))),
        });
        assert_eq!(
            plain(&m.text),
            "File     src/h.txt\nPattern  ne\nMatches  3\n\nBODY"
        );
        assert_eq!(m.header_lines, 4);
        assert_eq!(m.text.lines[1].spans[1].style.fg, Some(theme::RED));
        let file: Vec<_> = m.text.lines[0].spans[1..]
            .iter()
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect();
        assert_eq!(
            file,
            [
                ("src".to_string(), Some(theme::BLUE)),
                ("/".to_string(), Some(theme::YELLOW)),
                ("h.txt".to_string(), Some(theme::GREEN)),
            ]
        );
        assert_eq!(m.text.lines[2].spans[1].style.fg, Some(theme::PEACH));
    }

    #[test]
    fn header_without_pattern_is_the_file() {
        let path = file("h2.txt", b"text\n");
        let m = msg(PreviewPlan::Sniff {
            path,
            text: Some(printf("BODY")),
            binary: Binary::Describe(None),
            header: header("h2.txt", None),
        });
        assert_eq!(plain(&m.text), "File     h2.txt\n\nBODY");
        assert_eq!(m.header_lines, 2);
    }

    #[test]
    fn count_adds_up_archive_members() {
        let path = file("h.zip", b"PK\x03\x04\0\0");
        let m = msg(PreviewPlan::Sniff {
            path,
            text: None,
            binary: Binary::Show(printf("BIN")),
            header: header("h.zip", Some(printf("h.zip{a}:1\\nh.zip{b:c}:2\\n"))),
        });
        assert!(plain(&m.text).contains("Matches  3"), "{}", plain(&m.text));
    }

    #[test]
    fn count_that_fails_shows_a_question_mark() {
        let path = file("h3.txt", b"text\n");
        let m = msg(PreviewPlan::Sniff {
            path,
            text: Some(printf("BODY")),
            binary: Binary::Describe(None),
            header: header("h3.txt", Some(Cmd::new("/nonexistent/ugrep"))),
        });
        assert!(plain(&m.text).contains("Matches  ?"), "{}", plain(&m.text));
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
