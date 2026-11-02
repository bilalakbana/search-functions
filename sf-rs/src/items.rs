//! One row of a list, and the parsers that turn engine output into rows.
//!
//! A row is kept small (a list can hold millions): its key, and only what drawing needs to build the
//! coloured line for the few rows on screen.

use std::borrow::Cow;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use ansi_to_tui::IntoText;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::commands::{MATCH_END, MATCH_START, ParseKind, SEP, is_hash};
use crate::theme;

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// What the actions get: a path relative to the root with its real bytes, or a commit hash.
    pub key: OsString,
    /// The line number in Matching lines.
    pub line: Option<u32>,
    /// False for hints, error lines and graph-only Git lines.
    pub selectable: bool,
    look: Look,
}

#[derive(Clone, Debug, PartialEq)]
enum Look {
    /// The key as a path (coloured by `path_spans`). `lower`: the shown text in lower case, for the
    /// filter.
    Path { lower: Box<str> },
    /// A matching line: the key (and an archive member) blue, the line number dim, then `text`,
    /// printable already, its matches between MATCH_START and MATCH_END; `cut` when it was longer.
    /// Not filtered (in Search the pattern is the filter), so no lower-case copy.
    Match {
        member: Box<str>,
        text: Box<str>,
        cut: bool,
    },
    /// A line of a file in a version of the repository (Git, Ctrl-Shift-Alt-F): the key is `REV:PATH`, drawn
    /// as the short version, the path, the line number and `text` (printable; matches are marked when
    /// drawn).
    Version { text: Box<str>, cut: bool },
    /// A `git log --graph` row as git wrote it (its colours), made into spans only when drawn: a graph
    /// of many branches has hundreds of escape bytes a row, and a span for each colour took kilobytes
    /// (git/git with --all: 146,309 rows, 1.75 GB). `number`: a commit's, None on a graph-only row;
    /// `text`: the shown text, which Match case filters every row by.
    Git {
        number: Option<u32>,
        raw: Box<[u8]>,
        text: Box<str>,
        lower: Box<str>,
    },
    /// Drawn as it is: notes, the parent row.
    Styled {
        line: Box<Line<'static>>,
        lower: Box<str>,
    },
}

/// Longest text of a matching line that is kept (minified files have lines of megabytes).
const MAX_TEXT: usize = 1000;

/// Archive types whose members ugrep+ -z shows as `a.zip{member}` (the prototype's list).
const ARCHIVES: [&str; 23] = [
    "zip", "jar", "war", "apk", "docx", "xlsx", "pptx", "odt", "ods", "odp", "epub", "tar", "tgz",
    "gz", "bz2", "xz", "zst", "lz4", "lzma", "7z", "cpio", "pax", "Z",
];

impl Item {
    fn path(key: &[u8]) -> Item {
        let lower = printable(&String::from_utf8_lossy(key))
            .to_lowercase()
            .into_boxed_str();
        Item {
            key: OsString::from_vec(key.to_vec()),
            line: None,
            selectable: true,
            look: Look::Path { lower },
        }
    }

    fn styled(key: OsString, line: Line<'static>, selectable: bool) -> Item {
        let lower = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
            .to_lowercase()
            .into_boxed_str();
        Item {
            key,
            line: None,
            selectable,
            look: Look::Styled {
                line: Box::new(line),
                lower,
            },
        }
    }

    /// The coloured line; control characters are shown as U+FFFD.
    pub fn display(&self) -> Line<'static> {
        match &self.look {
            Look::Path { .. } => Line::from(path_spans(&printable(&String::from_utf8_lossy(
                self.key.as_bytes(),
            )))),
            Look::Match { member, text, cut } => {
                let faint = Style::new().fg(theme::border());
                let mut path = printable(&String::from_utf8_lossy(self.key.as_bytes()));
                path.push_str(member);
                let mut spans = path_spans(&path);
                spans.extend([
                    Span::styled(":", faint),
                    Span::styled(
                        self.line.unwrap_or(0).to_string(),
                        Style::new().fg(theme::info()),
                    ),
                    Span::styled(":", faint),
                ]);
                spans.extend(match_spans(text));
                if *cut {
                    spans.push(Span::styled("…", Style::new().fg(theme::info())));
                }
                Line::from(spans)
            }
            Look::Version { text, cut } => {
                let (rev, path) = self.version().unwrap_or(("", ""));
                let faint = Style::new().fg(theme::border());
                let mut spans = vec![
                    Span::styled(
                        rev.chars().take(7).collect::<String>(),
                        Style::new()
                            .fg(theme::path_dir())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                ];
                spans.extend(path_spans(&printable(path)));
                // line 0: the file itself (every file's history), no line and no text
                if self.line.is_some_and(|n| n > 0) {
                    spans.extend([
                        Span::styled(":", faint),
                        Span::styled(
                            self.line.unwrap_or(0).to_string(),
                            Style::new().fg(theme::info()),
                        ),
                        Span::styled(":", faint),
                        Span::raw(text.to_string()),
                    ]);
                }
                if *cut {
                    spans.push(Span::styled("…", Style::new().fg(theme::info())));
                }
                Line::from(spans)
            }
            Look::Git { number, raw, .. } => {
                let mut spans = Vec::new();
                if let Some(n) = number {
                    spans.push(Span::styled(n.to_string(), Style::new().fg(theme::info())));
                    spans.push(Span::raw(" "));
                }
                spans.extend(first_line(raw).spans);
                Line::from(spans)
            }
            Look::Styled { line, .. } => (**line).clone(),
        }
    }

    /// A versions row's text, and the byte where it starts in the drawn row (after the short version,
    /// the path and the line number).
    pub fn version_text(&self) -> Option<(usize, String)> {
        let Look::Version { text, .. } = &self.look else {
            return None;
        };
        let (rev, path) = self.version()?;
        let before = format!(
            "{} {}:{}:",
            rev.chars().take(7).collect::<String>(),
            printable(path),
            self.line.unwrap_or(0)
        );
        Some((before.len(), text.to_string()))
    }

    /// A versions row's version (the full hash) and path.
    pub fn version(&self) -> Option<(&str, &str)> {
        if !matches!(self.look, Look::Version { .. }) {
            return None;
        }
        self.key.to_str()?.split_once(':')
    }

    /// The shown text as it is, what a filter that matches case looks in after `haystack`.
    pub fn text(&self) -> String {
        match &self.look {
            Look::Path { .. } => printable(&String::from_utf8_lossy(self.key.as_bytes())),
            Look::Styled { line, .. } => line.spans.iter().map(|s| s.content.as_ref()).collect(),
            Look::Match { .. } => {
                let (path, text) = self.match_parts().unwrap_or_default();
                format!("{path}:{}:{text}", self.line.unwrap_or(0))
            }
            Look::Git { text, .. } => text.to_string(),
            Look::Version { .. } => self
                .display()
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect(),
        }
    }

    /// A matching line's path (with its archive member) and text as shown, without the match marks.
    pub fn match_parts(&self) -> Option<(String, String)> {
        let Look::Match { member, text, .. } = &self.look else {
            return None;
        };
        let mut path = printable(&String::from_utf8_lossy(self.key.as_bytes()));
        path.push_str(member);
        let marks = [char::from(MATCH_START), char::from(MATCH_END)];
        Some((path, text.replace(marks, "")))
    }

    /// The shown text in lower case, what the filter looks in; a matching line's is made when asked
    /// (Ctrl-Alt-F on Matching lines is rare, and rows stay small without a copy).
    pub fn haystack(&self) -> Cow<'_, str> {
        match &self.look {
            Look::Path { lower, .. } | Look::Styled { lower, .. } | Look::Git { lower, .. } => {
                Cow::Borrowed(lower)
            }
            Look::Match { .. } | Look::Version { .. } => Cow::Owned(self.text().to_lowercase()),
        }
    }
}

/// A dim line that selects nothing ("type a pattern", "No match").
pub fn hint(text: &str) -> Item {
    let style = Style::new()
        .fg(theme::info())
        .add_modifier(Modifier::ITALIC);
    Item::styled(
        OsString::new(),
        Line::from(Span::styled(printable(text), style)),
        false,
    )
}

/// A red line that selects nothing: an engine's message.
pub fn error(text: &str) -> Item {
    Item::styled(
        OsString::new(),
        Line::from(Span::styled(
            printable(text),
            Style::new().fg(theme::error()),
        )),
        false,
    )
}

/// The `..` row at the top of Folders.
pub fn parent() -> Item {
    let line = Line::from(vec![
        Span::styled("..", Style::new().fg(theme::path_name())),
        Span::styled(
            "  parent folder",
            Style::new()
                .fg(theme::info())
                .add_modifier(Modifier::ITALIC),
        ),
    ]);
    Item::styled("..".into(), line, true)
}

/// One record of a list's output (without its NUL or newline) as a row; `None` for nothing to show.
/// `git_number` is the number shown in front of a Git commit.
pub fn parse(kind: ParseKind, raw: &[u8], git_number: u32) -> Option<Item> {
    match kind {
        ParseKind::Files | ParseKind::Folders => {
            let mut path = raw.strip_prefix(b"./").unwrap_or(raw);
            if kind == ParseKind::Folders {
                path = path.strip_suffix(b"/").unwrap_or(path);
            }
            (!path.is_empty()).then(|| Item::path(path))
        }
        ParseKind::SearchFiles { archives } => {
            let path = raw.strip_prefix(b"./").unwrap_or(raw);
            let key = if archives { archive_of(path) } else { path };
            (!key.is_empty()).then(|| Item::path(key))
        }
        ParseKind::SearchLines => search_line(raw),
        ParseKind::SearchMatches => search_line(&marked(raw)),
        ParseKind::Versions => version_line(raw),
        ParseKind::Git => Some(git_row(raw.to_vec(), git_number)),
    }
}

/// ugrep's coloured Each match record as a Matching lines record: the match, `ESC[7m` up to the next
/// `ESC[m`, between MATCH_START and MATCH_END; every other escape sequence after the name dropped.
/// The name keeps its bytes (it may hold an escape sequence), less ugrep's colour reset after it.
fn marked(raw: &[u8]) -> Vec<u8> {
    let (name, raw) = raw.split_at(raw.iter().position(|&b| b == 0).unwrap_or(0));
    let mut out = name.strip_suffix(b"\x1b[m").unwrap_or(name).to_vec();
    out.reserve(raw.len());
    let mut in_match = false;
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == 0x1b && raw.get(i + 1) == Some(&b'[') {
            // ESC [ parameters m
            let end = raw[i + 2..]
                .iter()
                .position(|b| !(b.is_ascii_digit() || *b == b';'))
                .map(|n| i + 2 + n);
            if let Some(end) = end.filter(|&e| raw[e] == b'm') {
                match &raw[i + 2..end] {
                    b"7" => {
                        out.push(MATCH_START);
                        in_match = true;
                    }
                    b"" | b"0" if in_match => {
                        out.push(MATCH_END);
                        in_match = false;
                    }
                    _ => {}
                }
                i = end + 1;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    if in_match {
        out.push(MATCH_END);
    }
    out
}

/// A path as ugrep-fzf colours it: folders blue, slashes yellow, the name green.
pub fn path_spans(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut parts = text.split('/').peekable();
    let mut first = true;
    while let Some(part) = parts.next() {
        if !first {
            spans.push(Span::styled("/", Style::new().fg(theme::path_slash())));
        }
        first = false;
        if !part.is_empty() {
            let colour = if parts.peek().is_some() {
                theme::path_dir()
            } else {
                theme::path_name()
            };
            spans.push(Span::styled(part.to_string(), Style::new().fg(colour)));
        }
    }
    spans
}

/// `a.zip{member}` → `a.zip`, for the archive types in ARCHIVES; anything else unchanged.
fn archive_of(path: &[u8]) -> &[u8] {
    for (i, _) in path.iter().enumerate().filter(|&(_, &b)| b == b'{') {
        let before = &path[..i];
        let is_archive = ARCHIVES.iter().any(|ext| {
            before.len() > ext.len()
                && before.ends_with(ext.as_bytes())
                && before[before.len() - ext.len() - 1] == b'.'
        });
        if is_archive {
            return before;
        }
    }
    path
}

/// `path NUL [{member} SEP] line SEP text`, the matches in the text between MATCH_START and MATCH_END.
fn search_line(raw: &[u8]) -> Option<Item> {
    let nul = raw.iter().position(|&b| b == 0)?;
    let (path, mut rest) = (&raw[..nul], &raw[nul + 1..]);
    let mut member: &[u8] = b"";
    if rest.starts_with(b"{") {
        let end = rest.windows(2).position(|w| w == [b'}', SEP])?;
        member = &rest[..=end];
        rest = &rest[end + 2..];
    }
    let sep = rest.iter().position(|&b| b == SEP)?;
    let digits = &rest[..sep];
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let line: u32 = std::str::from_utf8(digits).ok()?.parse().ok()?;
    let mut text = &rest[sep + 1..];
    text = text.strip_suffix(b"\r").unwrap_or(text);
    let (text, cut) = match_text(&String::from_utf8_lossy(text));
    let member = printable(&String::from_utf8_lossy(member)).into_boxed_str();
    Some(Item {
        key: OsString::from_vec(path.to_vec()),
        line: Some(line),
        selectable: true,
        look: Look::Match { member, text, cut },
    })
}

/// `REV:PATH\0LINE\0TEXT` from `sf grep-versions`: REV a full hash, LINE digits.
fn version_line(raw: &[u8]) -> Option<Item> {
    let mut parts = raw.splitn(3, |&b| b == 0);
    let (key, digits, text) = (parts.next()?, parts.next()?, parts.next()?);
    let (rev, path) = (key.get(..40)?, key.get(41..)?);
    if key.get(40) != Some(&b':') || !rev.iter().all(u8::is_ascii_hexdigit) || path.is_empty() {
        return None;
    }
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let line: u32 = std::str::from_utf8(digits).ok()?.parse().ok()?;
    let text = text.strip_suffix(b"\r").unwrap_or(text);
    let (text, cut) = match_text(&String::from_utf8_lossy(text));
    Some(Item {
        key: OsString::from_vec(key.to_vec()),
        line: Some(line),
        selectable: true,
        look: Look::Version { text, cut },
    })
}

/// A matching line's text as it is kept: the markers stay, tabs become spaces, other control
/// characters U+FFFD; at most MAX_TEXT characters (`true` when cut).
fn match_text(text: &str) -> (Box<str>, bool) {
    let (start, end) = (char::from(MATCH_START), char::from(MATCH_END));
    let mut out = String::with_capacity(text.len().min(MAX_TEXT * 4));
    let mut shown = 0;
    for c in text.chars() {
        if c == start || c == end {
            out.push(c);
        } else if shown == MAX_TEXT {
            return (out.into_boxed_str(), true);
        } else {
            shown += 1;
            out.push(match c {
                '\t' => ' ',
                c if c.is_control() => '\u{fffd}',
                c => c,
            });
        }
    }
    (out.into_boxed_str(), false)
}

/// Kept text → spans: plain text bright, matches in hl.
fn match_spans(text: &str) -> Vec<Span<'static>> {
    let (start, end) = (char::from(MATCH_START), char::from(MATCH_END));
    let plain = Style::new().fg(theme::fg());
    let hit = Style::new().fg(theme::hl()).add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut in_match = false;
    for part in text.split_inclusive([start, end]) {
        let marker = part
            .ends_with([start, end])
            .then(|| part.chars().last().unwrap());
        let body = if marker.is_some() {
            &part[..part.len() - 1]
        } else {
            part
        };
        if !body.is_empty() {
            spans.push(Span::styled(
                body.to_string(),
                if in_match { hit } else { plain },
            ));
        }
        if let Some(m) = marker {
            in_match = m == start;
        }
    }
    spans
}

/// A `git log --graph` line: coloured display, then SEP and the full hash; a graph-only line has none.
/// Takes the line, so its bytes are kept without a copy.
pub fn git_row(mut raw: Vec<u8>, number: u32) -> Item {
    let mut key = OsString::new();
    let mut commit = None;
    if let Some(sep) = raw.iter().rposition(|&b| b == SEP) {
        if let Some(h) = std::str::from_utf8(&raw[sep + 1..])
            .ok()
            .filter(|h| is_hash(h))
        {
            key = h.into();
        }
        raw.truncate(sep);
        commit = Some(number);
    }
    let mut item = Item {
        selectable: !key.is_empty(),
        key,
        line: None,
        look: Look::Git {
            number: commit,
            raw: raw.into_boxed_slice(),
            text: Box::default(),
            lower: Box::default(),
        },
    };
    let shown: String = item
        .display()
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    if let Look::Git { text, lower, .. } = &mut item.look {
        *lower = shown.to_lowercase().into_boxed_str();
        *text = shown.into_boxed_str();
    }
    item
}

fn first_line(bytes: &[u8]) -> Line<'static> {
    ansi_text(bytes)
        .lines
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// Control characters (C0, DEL, C1) and the bidirectional embeddings, overrides and isolates as
/// U+FFFD, so nothing raw reaches the terminal and no name reads backwards ("rtl-\u{202e}txt.exe"
/// drawn as "rtl-exe.txt"). The marks LRM and RLM stay: Arabic and Hebrew text has them.
pub fn printable(s: &str) -> String {
    s.chars()
        .map(|c| if unshown(c) { '\u{fffd}' } else { c })
        .collect()
}

fn unshown(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// A file's type for Ctrl-S: its extension as written (`a.tar.gz` → "gz"), None when it has none
/// (`Makefile`, `.bashrc`).
pub fn file_type(key: &std::ffi::OsStr) -> Option<String> {
    std::path::Path::new(key)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
}

/// Coloured engine output as styled text: invalid UTF-8 replaced, tabs as spaces, carriage returns left
/// out, other control characters as U+FFFD.
pub fn ansi_text(bytes: &[u8]) -> Text<'static> {
    // ansi-to-tui breaks the line at every carriage return: a CRLF file, or a binary's match ending in
    // "\r\r\n", would get empty lines (the fzf previews ran `tr -d '\r'` for this)
    let without_cr: Vec<u8>;
    let bytes = if bytes.contains(&b'\r') {
        without_cr = bytes.iter().copied().filter(|&b| b != b'\r').collect();
        &without_cr[..]
    } else {
        bytes
    };
    let lossy = String::from_utf8_lossy(bytes);
    let mut text = lossy
        .as_bytes()
        .into_text()
        .unwrap_or_else(|_| Text::raw(lossy.into_owned()));
    for line in &mut text.lines {
        for span in &mut line.spans {
            if span.content.chars().any(unshown) {
                span.content = printable(&span.content.replace('\t', "    ")).into();
            }
            // a file's own ESC[8m (concealed) must not hide its text in the preview
            span.style = span.style.remove_modifier(Modifier::HIDDEN);
        }
    }
    text
}

/// Files in the Content tab's order (`ugrep --sort=name`): in each folder its own files first, then its
/// subfolders, each in byte order. The keys are unique, so an unstable sort will do.
pub fn sort_files(items: &mut [Item]) {
    items.sort_unstable_by(|a, b| tree_order(a.key.as_bytes(), b.key.as_bytes()));
}

/// Part by part in byte order; where two paths part, a file name (a last part) comes before a folder.
/// Works on the bytes after the common prefix: splitting every path per comparison doubled the sort time.
fn tree_order(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (ra, rb) = (&a[i..], &b[i..]);
    // a '/' after the common prefix: that path goes on into a subfolder of the folder both are in
    let in_folder = |r: &[u8]| r.contains(&b'/');
    in_folder(ra).cmp(&in_folder(rb)).then_with(|| {
        // the same kind: the part that ends here (at '/' or the path's end) is the shorter name
        let end = |r: &[u8]| r.first().is_none_or(|&c| c == b'/');
        end(rb)
            .cmp(&end(ra))
            .then_with(|| ra.first().cmp(&rb.first()))
    })
}

/// Folders by depth, then in byte order.
pub fn sort_folders(items: &mut [Item]) {
    let depth = |i: &Item| i.key.as_bytes().iter().filter(|&&b| b == b'/').count();
    items.sort_unstable_by(|a, b| {
        depth(a)
            .cmp(&depth(b))
            .then_with(|| a.key.as_bytes().cmp(b.key.as_bytes()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn each_match_rows_read_ugreps_colours_as_marks() {
        // `ugrep -o -B50 -A50 --color=always` with GREP_COLORS ms=7 and every other colour empty
        let coloured =
            b"f.txt\x1b[m\x001\x1b[m\x1f\x1b[maaaa \x1b[m\x1b[7mneedle\x1b[m bbbb needle\x1b[m...\x1b[m";
        let plain = b"f.txt\x001\x1faaaa \x01needle\x02 bbbb needle...";
        assert_eq!(
            parse(ParseKind::SearchMatches, coloured, 0),
            parse(ParseKind::SearchLines, plain, 0)
        );
    }

    fn files(raw: &[u8]) -> Item {
        parse(ParseKind::Files, raw, 0).unwrap()
    }

    /// (text, colour) of every span
    fn colours(line: &Line) -> Vec<(String, Option<ratatui::style::Color>)> {
        line.spans
            .iter()
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect()
    }

    #[test]
    fn paths_are_coloured_as_ugrep_fzf_does() {
        // folders blue, slashes yellow, the name green
        let it = files(b"./src/deep dir/a b&c.txt");
        assert_eq!(it.key, "src/deep dir/a b&c.txt");
        let (b, y, g) = (
            Some(theme::path_dir()),
            Some(theme::path_slash()),
            Some(theme::path_name()),
        );
        let want = [
            ("src", b),
            ("/", y),
            ("deep dir", b),
            ("/", y),
            ("a b&c.txt", g),
        ];
        assert_eq!(
            colours(&it.display()),
            want.map(|(t, c)| (t.to_string(), c))
        );
        assert!(it.selectable);
        assert_eq!(
            colours(&files(b"README.md").display()),
            [("README.md".to_string(), g)]
        );
    }

    #[test]
    fn path_spans_for_an_absolute_path() {
        let (b, y, g) = (
            Some(theme::path_dir()),
            Some(theme::path_slash()),
            Some(theme::path_name()),
        );
        let line = Line::from(path_spans("/home/x"));
        let want = [("/", y), ("home", b), ("/", y), ("x", g)];
        assert_eq!(colours(&line), want.map(|(t, c)| (t.to_string(), c)));
    }

    #[test]
    fn control_chars_only_in_display() {
        let it = files(b"./bad\x1bname\x1f.txt");
        assert_eq!(it.key, "bad\x1bname\x1f.txt");
        assert_eq!(text_of(&it.display()), "bad\u{fffd}name\u{fffd}.txt");
    }

    #[test]
    fn non_utf8_name_keeps_its_bytes() {
        let it = files(b"caf\xe9.txt");
        assert_eq!(it.key, OsString::from_vec(b"caf\xe9.txt".to_vec()));
        assert_eq!(text_of(&it.display()), "caf\u{fffd}.txt");
    }

    #[test]
    fn printable_replaces_c0_del_c1() {
        assert_eq!(
            printable("a\tb\x7fc\u{85}d\u{9b}e ğ"),
            "a\u{fffd}b\u{fffd}c\u{fffd}d\u{fffd}e ğ"
        );
    }

    #[test]
    fn folders_without_trailing_slash_and_their_name_green() {
        let it = parse(ParseKind::Folders, b"./src/deep dir/", 0).unwrap();
        assert_eq!(it.key, "src/deep dir");
        assert_eq!(text_of(&it.display()), "src/deep dir");
        assert_eq!(
            it.display().spans.last().unwrap().style.fg,
            Some(theme::path_name())
        );
    }

    #[test]
    fn empty_record_is_nothing() {
        assert_eq!(parse(ParseKind::Files, b"", 0), None);
    }

    #[test]
    fn a_versions_row_names_the_version_the_path_and_the_line() {
        let rev = "7b4ef6fd550f68d2d2bd2907c800fb95c06d973c";
        let raw = format!("{rev}:src/odd:name.txt\x003\x00the\tneedle\r");
        let it = parse(ParseKind::Versions, raw.as_bytes(), 0).unwrap();
        assert_eq!(
            it.key,
            format!("{rev}:src/odd:name.txt").as_str(),
            "the version and the path"
        );
        assert_eq!(it.line, Some(3));
        assert_eq!(
            text_of(&it.display()),
            "7b4ef6f src/odd:name.txt:3:the needle"
        );
        assert_eq!(
            it.display().spans[0].style.fg,
            Some(theme::path_dir()),
            "the version"
        );
        assert_eq!(it.version(), Some((rev, "src/odd:name.txt")));
        assert_eq!(&*it.haystack(), "7b4ef6f src/odd:name.txt:3:the needle");
        assert_eq!(parse(ParseKind::Versions, b"nohash:a\x001\x00x", 0), None);
        assert_eq!(
            parse(
                ParseKind::Versions,
                format!("{rev}:a\x00x\x00t").as_bytes(),
                0
            ),
            None,
            "no line number"
        );
    }

    #[test]
    fn a_versions_row_of_line_0_is_the_file_itself() {
        let rev = "7b4ef6fd550f68d2d2bd2907c800fb95c06d973c";
        let raw = format!("{rev}:src/a.c\x000\x00");
        let it = parse(ParseKind::Versions, raw.as_bytes(), 0).unwrap();
        assert_eq!(
            text_of(&it.display()),
            "7b4ef6f src/a.c",
            "no :0: and no text"
        );
        assert_eq!(it.version(), Some((rev, "src/a.c")));
    }

    #[test]
    fn search_line_with_colon_in_name() {
        let it = parse(
            ParseKind::SearchLines,
            b"src/odd:name.txt\x0012\x1f\x01needle\x02: colon",
            0,
        )
        .unwrap();
        assert_eq!(it.key, "src/odd:name.txt");
        assert_eq!(it.line, Some(12));
        assert_eq!(text_of(&it.display()), "src/odd:name.txt:12:needle: colon");
    }

    #[test]
    fn search_line_marks_matches_red() {
        let it = parse(
            ParseKind::SearchLines,
            b"a.c\x003\x1fx \x01ee\x02 y \x01e\x02\r",
            0,
        )
        .unwrap();
        assert_eq!(text_of(&it.display()), "a.c:3:x ee y e");
        let shown = it.display();
        let red: Vec<&str> = shown
            .spans
            .iter()
            .filter(|s| s.style.fg == Some(theme::hl()))
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(red, ["ee", "e"]);
        assert_eq!(
            it.display().spans[0].style.fg,
            Some(theme::path_name()),
            "a.c: no folder, so the name"
        );
    }

    #[test]
    fn search_line_text_with_0x1f_keeps_key_and_line() {
        let raw = b"src/evil.txt\x002\x1fevil\x1f!touch PWNED\x1ftail";
        let it = parse(ParseKind::SearchLines, raw, 0).unwrap();
        assert_eq!(it.key, "src/evil.txt");
        assert_eq!(it.line, Some(2));
        assert!(text_of(&it.display()).ends_with(":2:evil\u{fffd}!touch PWNED\u{fffd}tail"));
    }

    #[test]
    fn search_line_tab_becomes_space() {
        let it = parse(ParseKind::SearchLines, b"a\x001\x1f\tx", 0).unwrap();
        assert_eq!(text_of(&it.display()), "a:1: x");
    }

    #[test]
    fn search_line_without_number_is_dropped() {
        assert_eq!(parse(ParseKind::SearchLines, b"a\x00x\x1ftext", 0), None);
        assert_eq!(parse(ParseKind::SearchLines, b"no nul here", 0), None);
    }

    #[test]
    fn search_line_in_archive_member() {
        let it = parse(
            ParseKind::SearchLines,
            b"docs/arch.zip\x00{inzip.txt}\x1f1\x1f\x01needle\x02 inside",
            0,
        )
        .unwrap();
        assert_eq!(it.key, "docs/arch.zip");
        assert_eq!(it.line, Some(1));
        assert_eq!(
            text_of(&it.display()),
            "docs/arch.zip{inzip.txt}:1:needle inside"
        );
        let shown = it.display();
        assert_eq!(
            colours(&shown)[..3],
            [
                ("docs".to_string(), Some(theme::path_dir())),
                ("/".to_string(), Some(theme::path_slash())),
                ("arch.zip{inzip.txt}".to_string(), Some(theme::path_name())),
            ]
        );
    }

    #[test]
    fn long_line_text_is_cut() {
        let mut raw = b"a\x001\x1f".to_vec();
        raw.extend(std::iter::repeat_n(b'x', 5000));
        let it = parse(ParseKind::SearchLines, &raw, 0).unwrap();
        assert_eq!(
            text_of(&it.display()).chars().count(),
            "a:1:".len() + MAX_TEXT + 1
        );
    }

    #[test]
    fn cut_line_is_still_printable() {
        let mut raw = b"a\x001\x1f\x1b[2J".to_vec();
        raw.extend(std::iter::repeat_n(b'x', 5000));
        let it = parse(ParseKind::SearchLines, &raw, 0).unwrap();
        assert!(
            text_of(&it.display()).starts_with("a:1:\u{fffd}[2J"),
            "{:?}",
            text_of(&it.display()).get(..12)
        );
    }

    #[test]
    fn archive_member_key() {
        let it = parse(
            ParseKind::SearchFiles { archives: true },
            b"docs/arch.zip{inzip.txt}",
            0,
        )
        .unwrap();
        assert_eq!(it.key, "docs/arch.zip");
        assert_eq!(text_of(&it.display()), "docs/arch.zip");
        let it = parse(ParseKind::SearchFiles { archives: true }, b"x/a{b}.txt", 0).unwrap();
        assert_eq!(it.key, "x/a{b}.txt");
        let it = parse(
            ParseKind::SearchFiles { archives: false },
            b"docs/arch.zip{inzip.txt}",
            0,
        )
        .unwrap();
        assert_eq!(it.key, "docs/arch.zip{inzip.txt}");
    }

    #[test]
    fn git_line_takes_hash_from_last_field() {
        let hash = "374425a0123456789abcdef0123456789abcdef0";
        let raw = format!("* \x1b[1;34m374425a\x1b[m T \x1b[37msub\x1fject\x1b[m\x1f{hash}");
        let it = parse(ParseKind::Git, raw.as_bytes(), 7).unwrap();
        assert_eq!(it.key, hash);
        assert!(it.selectable);
        assert_eq!(text_of(&it.display()), "7 * 374425a T sub\u{fffd}ject");
        assert_eq!(it.display().spans[0].style.fg, Some(theme::info()));
    }

    #[test]
    fn git_graph_only_line_not_selectable() {
        let it = parse(ParseKind::Git, b"|\\", 0).unwrap();
        assert!(!it.selectable);
        assert_eq!(text_of(&it.display()), "|\\");
    }

    #[test]
    fn git_line_with_bad_hash_not_selectable() {
        let it = parse(ParseKind::Git, b"* x\x1f--output=/tmp/x", 3).unwrap();
        assert!(!it.selectable);
    }

    #[test]
    fn ansi_text_cleans_control_chars() {
        let t = ansi_text(b"\x1b[31mred\x1b[0m\ta\x07b\xff\nline2");
        assert_eq!(t.lines.len(), 2);
        assert_eq!(text_of(&t.lines[0]), "red    a\u{fffd}b\u{fffd}");
        assert_eq!(
            t.lines[0].spans[0].style.fg,
            Some(ratatui::style::Color::Red)
        );
    }

    #[test]
    fn ansi_text_breaks_lines_only_at_newlines() {
        // a binary's match as ugrep prints it ends in "\r\r\n"; a carriage return is no line of its own
        let t = ansi_text(b"1:MZ\x00This program\r\r\nx\ry\n");
        assert_eq!(
            t.lines.iter().map(text_of).collect::<Vec<_>>(),
            ["1:MZ\u{fffd}This program", "xy"]
        );
    }

    #[test]
    fn ansi_text_lets_no_escape_through() {
        // bat prints a file's bytes as they are: OSC 52 would write the clipboard, CSI 2J clear the
        // screen, DCS/APC talk to the terminal
        let evil: &[&[u8]] = &[
            b"a\x1b]52;c;SGVsbG8=\x07b",
            b"a\x1b]0;title\x1b\\b",
            b"a\x1b[2Jb\x1b[10;10Hc",
            b"a\x1bPq#0;2;0;0;0\x1b\\b",
            b"a\x1b_Gf=24;AAAA\x1b\\b",
            b"a\x9b2Jb\xc2\x9b2J",
            b"\x1b[31mred\x1b[0m \x1b[38;5;1m\x1b[",
        ];
        for bytes in evil {
            let t = ansi_text(bytes);
            for line in &t.lines {
                for span in &line.spans {
                    assert!(
                        !span.content.chars().any(char::is_control),
                        "{:?} → {:?}",
                        String::from_utf8_lossy(bytes),
                        span.content
                    );
                }
            }
        }
    }

    #[test]
    fn no_name_reads_backwards_and_no_text_is_concealed() {
        assert_eq!(printable("rtl-\u{202e}txt.exe"), "rtl-\u{fffd}txt.exe");
        assert_eq!(printable("a\u{2067}b\u{202a}c"), "a\u{fffd}b\u{fffd}c");
        assert_eq!(
            printable("قال\u{200f} 1.\u{200e}"),
            "قال\u{200f} 1.\u{200e}",
            "the marks of Arabic text stay"
        );
        assert_eq!(
            printable("çay\u{200b}"),
            "çay\u{200b}",
            "a zero-width space is no bidi control"
        );
        let t = ansi_text("x \x1b[8mHIDDEN\x1b[0m \u{202e}y".as_bytes());
        let spans = &t.lines[0].spans;
        assert!(
            spans
                .iter()
                .all(|s| !s.style.add_modifier.contains(Modifier::HIDDEN))
        );
        assert_eq!(text_of(&t.lines[0]), "x HIDDEN \u{fffd}y");
    }

    #[test]
    fn folders_sorted_by_depth() {
        let mut v: Vec<Item> = ["./b/c/", "./a/", "./b/", "./B/", "./a/z/y/", "./a/a/"]
            .iter()
            .map(|r| parse(ParseKind::Folders, r.as_bytes(), 0).unwrap())
            .collect();
        sort_folders(&mut v);
        let keys: Vec<_> = v.iter().map(|i| i.key.to_str().unwrap()).collect();
        assert_eq!(keys, ["B", "a", "b", "a/a", "b/c", "a/z/y"]);
    }

    #[test]
    fn files_sorted_folder_by_folder_files_first() {
        // the expected order is ugrep 7.8.5's `-rl --sort=name` on this tree; a-b and a/s catch a plain byte
        // compare ('-' sorts before '/', and the folder s is a prefix of sub)
        let mut v: Vec<Item> = [
            "b/y", "a/sub/z", "top", "a/x", "B", "a/B/w", "a/sub/A", "a.txt", "a-b/y", "ab/q",
            "a/xz", "a/s/q",
        ]
        .iter()
        .map(|r| files(r.as_bytes()))
        .collect();
        sort_files(&mut v);
        let keys: Vec<_> = v.iter().map(|i| i.key.to_str().unwrap()).collect();
        assert_eq!(
            keys,
            [
                "B", "a.txt", "top", "a/x", "a/xz", "a/B/w", "a/s/q", "a/sub/A", "a/sub/z",
                "a-b/y", "ab/q", "b/y"
            ]
        );
    }

    #[test]
    fn haystack_is_lowercase_display_text() {
        let it = files("Src/ÇAY.txt".as_bytes());
        assert_eq!(it.haystack(), "src/çay.txt");
    }

    /// Heap plus inline bytes per row for `n` rows made by `make`.
    fn bytes_per_row(n: usize, make: impl Fn(usize) -> Vec<u8>, kind: ParseKind) -> usize {
        let raws: Vec<Vec<u8>> = (0..n).map(make).collect();
        let before = crate::testutil::live_heap();
        let rows: Vec<Item> = raws.iter().map(|r| parse(kind, r, 0).unwrap()).collect();
        let heap = crate::testutil::live_heap() - before;
        let per_row = heap as usize / n;
        drop(rows);
        per_row
    }

    #[test]
    fn file_rows_are_compact() {
        // 47-byte paths, five folders deep: the key, the lower-case haystack and the row itself
        let per_row = bytes_per_row(
            10_000,
            |i| format!("./home/Projects/app/src/module/file_{i:05}.rs").into_bytes(),
            ParseKind::Files,
        );
        eprintln!("{per_row} bytes per file row");
        assert!(per_row <= 240, "{per_row} bytes per file row");
    }

    #[test]
    fn matching_line_rows_are_compact() {
        let per_row = bytes_per_row(
            10_000,
            |i| {
                format!("src/module/file.rs\x00{i}\x1f    let \x01value\x02 = compute(\x01value\x02, other);").into_bytes()
            },
            ParseKind::SearchLines,
        );
        eprintln!("{per_row} bytes per matching-line row");
        assert!(per_row <= 240, "{per_row} bytes per matching-line row");
    }

    #[test]
    fn git_rows_of_a_wide_graph_are_compact() {
        // git/git with --all: 60 graph columns, each in a colour of its own (843 bytes a row on
        // average); a span for each colour made a row take kilobytes (146,309 rows, 1.75 GB)
        let hash = "374425a0123456789abcdef0123456789abcdef0";
        let graph = "\x1b[31m|\x1b[m ".repeat(60);
        let raw = |i: usize| {
            format!("{graph}* \x1b[1;34m374425a\x1b[m T\x1b[1;33m (tag: v{i})\x1b[m subject {i}\x1f{hash}")
                .into_bytes()
        };
        let per_row = bytes_per_row(10_000, raw, ParseKind::Git);
        let len = raw(5000).len();
        eprintln!("{per_row} bytes per Git row of {len} bytes");
        // the raw bytes, the shown text as it is (Match case) and in lower case (the filter)
        assert!(per_row <= len + 500, "{per_row} bytes per Git row of {len}");
        let it = parse(ParseKind::Git, &raw(7), 3).unwrap();
        assert_eq!(it.key, hash);
        assert!(text_of(&it.display()).starts_with("3 | | "));
        assert!(it.haystack().ends_with("* 374425a t (tag: v7) subject 7"));
    }

    #[test]
    fn hint_error_and_parent_rows() {
        assert!(!hint("type a pattern").selectable);
        assert!(!error("boom").selectable);
        assert_eq!(
            error("bad\x1b[2J").display().spans[0].content,
            "bad\u{fffd}[2J"
        );
        let p = parent();
        assert_eq!(p.key, "..");
        assert!(p.selectable);
    }
}
