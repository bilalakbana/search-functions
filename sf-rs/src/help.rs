//! The Help tab: what the tabs, keys, options, patterns and commands do, with examples. Short rows in
//! columns; the tests keep it in step with the options and the command line.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme;

/// The tabs: name, what it lists, the engine, what typing does.
const TABS: &[(&str, &str, &str, &str)] = &[
    (
        "Files",
        "every file below the folder",
        "fd",
        "filters the names",
    ),
    (
        "Content",
        "the text files that hold a pattern",
        "ugrep",
        "is the pattern",
    ),
    (
        "Deep",
        "the same in every file: binaries, archives",
        "ugrep(+)",
        "is the pattern",
    ),
    (
        "Folders",
        "every folder; Enter goes inside",
        "fd",
        "filters the names",
    ),
    (
        "Git",
        "the commit graph; Enter shows a commit",
        "git, delta",
        "filters the commits",
    ),
    ("Help", "this manual", "", "finds in it"),
];

/// The keys and what they do.
const KEYS: &[(&str, &str)] = &[
    (
        "Enter",
        "open with its application · Folders: go inside · Git: the commit",
    ),
    ("Ctrl-O", "the editor, at the line, the pattern marked"),
    (
        "Ctrl-T",
        "search inside the file (in tmux a window of its own)",
    ),
    (
        "Ctrl-G  Alt-G",
        "the file's changes against HEAD: whole file / short (in tmux a window)",
    ),
    (
        "Ctrl-U  Alt-U",
        "the file's history with its changes / its log (in tmux a window)",
    ),
    (
        "  in a view",
        "/ or Ctrl-T search, n N the next / the one before, Esc back",
    ),
    ("Ctrl-E", "show it in the file manager"),
    ("Ctrl-C", "copy the path (Git: the hash)"),
    ("Ctrl-S", "options and file types (⚙ Config)"),
    (
        "Ctrl-F",
        "find in the preview: Enter ↓ the next, ↑ the one before, Ctrl-S its options, Esc back",
    ),
    (
        "Ctrl-Alt-F",
        "Content, Deep: keep the pattern, typing filters the files found",
    ),
    ("Ctrl-L", "hide or show the preview"),
    ("Ctrl-R", "run the list again"),
    ("Alt-↑  Alt-↓", "scroll the preview"),
    ("Tab  Shift-Tab", "next / previous tab"),
    ("Esc", "back (a filter, a file, a view), else quit"),
];

/// The options of Ctrl-S and where they are.
const OPTIONS: &[(&str, &str)] = &[
    (
        "Hidden files",
        "names starting with a dot (Folders: Hidden folders)",
    ),
    (
        "Ignored files",
        "the .gitignore'd ones too (Folders: Ignored folders)",
    ),
    (
        "Matching lines",
        "Content: a row per matching line, not per file",
    ),
    (
        "Each match",
        "Content, Deep: a row per match, the text around it",
    ),
    (
        "Archives & documents",
        "Content: inside zip, tar, 7z, PDF, Office (ugrep+)",
    ),
    (
        "Documents",
        "Deep: PDF, Office, e-books as text (ugrep+, slower)",
    ),
    ("All words on a line", "ankara izmir: the lines with both"),
    (
        "All words in the file",
        "ankara izmir: the files with both, on any lines",
    ),
    (
        "Match case",
        "A is not a (else lower case finds both; Ctrl-F: else case aside)",
    ),
    (
        "Whole word",
        "Ctrl-F: not inside a longer word (config, not reconfig)",
    ),
    (
        "Regular expression",
        "Ctrl-F: what is typed is a regex (Rust's: no look-around)",
    ),
    (
        "All branches",
        "Git: every branch, not only the current one",
    ),
    (
        "Whole files",
        "Git, Ctrl-U: the changed files whole, not only around the changes",
    ),
    (
        "File types",
        "Files, Content, Deep: Space turns a type off, a: all",
    ),
];

/// Patterns in Content and Deep (ugrep, smart case).
const PATTERNS: &[(&str, &str)] = &[
    ("needle", "the text anywhere"),
    ("foo|bar", "either"),
    ("^import", "at the start of a line"),
    ("TODO.*fix", "TODO, then fix later on the line"),
    ("\\bword\\b", "the whole word (or: -- -w)"),
    ("[0-9]{4}-[0-9]{2}", "four digits, a dash, two digits"),
];

/// With "All words on a line" or "All words in the file" (ugrep's Boolean query).
const WORDS: &[(&str, &str)] = &[
    ("ankara izmir", "both"),
    ("ankara|izmir", "either"),
    ("ankara -izmir", "ankara, not izmir"),
    ("\"ankara izmir\"", "the phrase"),
];

/// The command line examples: the command and what it does.
pub const EXAMPLES: &[(&str, &str)] = &[
    ("sf", "Files in this folder"),
    (
        "sf content TODO",
        "the files with TODO; typing filters them, Esc: the pattern",
    ),
    (
        "sf deep needle -- -i",
        "every file, binaries and archives too, any case",
    ),
    (
        "sf content 'fn main' -- -t rust",
        "Rust files only (ugrep -tlist: the types)",
    ),
    (
        "sf content TODO -- -w --exclude-dir=target",
        "the whole word, target/ left out",
    ),
    (
        "sf content '(?<=id=)[0-9]+' -- -P",
        "a Perl regex: the number after id=",
    ),
    (
        "sf files -- -e pdf --changed-within 2weeks",
        "PDFs changed in two weeks (fd)",
    ),
    (
        "sf files -- -E node_modules -d 3",
        "three levels deep, node_modules left out",
    ),
    (
        "sf git -- --author=bilal --since=1.month",
        "a month of one author's commits",
    ),
    ("sf in src/main.rs 'ankara izmir'", "inside one file"),
    (
        "sf history src/main.rs",
        "its commits with their changes (also: log, diff, diff --short)",
    ),
    (
        "sf content TODO | wc -l",
        "into a pipe the list goes out: here counted",
    ),
    ("sf files rs --print", "the list on the terminal, no window"),
    (
        "sf open PATH",
        "open it (also: reveal, copy-path; copy-text TEXT)",
    ),
    (
        "sf doctor",
        "what sf found here, how it opens, reveals, copies",
    ),
];

/// The engines sf runs.
const ENGINES: &[(&str, &str)] = &[
    ("fd", "lists the files and folders"),
    (
        "ugrep",
        "searches inside them; ugrep+ reads PDF, Word, Office, e-books too",
    ),
    ("bat", "the preview, with line numbers"),
    ("git, delta", "history, changes, commits"),
];

pub fn text() -> Text<'static> {
    let mut lines = vec![
        Line::from(Span::styled(
            "sf — the search functions in one window",
            Style::new().fg(theme::MAUVE).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    heading(&mut lines, "Tabs", "Tab / Shift-Tab or a click");
    for &(tab, lists, engine, typing) in TABS {
        lines.push(Line::from(vec![
            Span::styled(format!("  {tab:<9}"), style(theme::SUB)),
            Span::styled(format!("{lists:<45}"), style(theme::TEXT)),
            Span::styled(format!("{engine:<12}"), style(theme::GREEN)),
            Span::styled(format!("typing {typing}"), style(theme::DIM)),
        ]));
    }
    table(
        &mut lines,
        "Keys",
        "for the selected row",
        KEYS,
        17,
        theme::SUB,
    );
    table(
        &mut lines,
        "Options",
        "Ctrl-S; Space or a click, ←→ across its columns",
        OPTIONS,
        24,
        theme::SUB,
    );
    table(
        &mut lines,
        "Patterns",
        "Content, Deep: ugrep, smart case",
        PATTERNS,
        22,
        theme::PEACH,
    );
    table(
        &mut lines,
        "Words",
        "with All words on a line / in the file",
        WORDS,
        22,
        theme::PEACH,
    );
    table(
        &mut lines,
        "Command line",
        "sf --help for the rest",
        EXAMPLES,
        46,
        theme::GREEN,
    );
    lines.push(Line::from(Span::styled(
        "  Options that change what an engine prints (-l, -c, --color, --json, …) are refused.",
        style(theme::DIM),
    )));
    table(
        &mut lines,
        "Engines",
        "sf doctor: which ones, where",
        ENGINES,
        12,
        theme::GREEN,
    );
    Text::from(lines)
}

fn style(fg: ratatui::style::Color) -> Style {
    Style::new().fg(fg)
}

fn heading(lines: &mut Vec<Line<'static>>, title: &str, note: &str) {
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled(
            title.to_string(),
            Style::new().fg(theme::MAUVE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("   {note}"), style(theme::DIM)),
    ]));
}

fn table(
    lines: &mut Vec<Line<'static>>,
    title: &str,
    note: &str,
    rows: &[(&str, &str)],
    width: usize,
    left: ratatui::style::Color,
) {
    heading(lines, title, note);
    for &(what, does) in rows {
        lines.push(Line::from(vec![
            Span::styled(format!("  {what:<width$} "), style(left)),
            Span::styled(does.to_string(), style(theme::TEXT)),
        ]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Options, Tab};

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

    /// Shell words: split at spaces, single quotes keep theirs.
    fn words(cmd: &str) -> Vec<String> {
        let (mut words, mut word, mut quoted) = (Vec::new(), String::new(), false);
        for c in cmd.chars() {
            match c {
                '\'' => quoted = !quoted,
                ' ' if !quoted => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
                c => word.push(c),
            }
        }
        if !word.is_empty() {
            words.push(word);
        }
        words
    }

    #[test]
    fn every_example_is_a_command_sf_takes() {
        for (cmd, _) in EXAMPLES {
            let sf_part = cmd.split(" | ").next().unwrap();
            let args: Vec<std::ffi::OsString> =
                words(sf_part).into_iter().skip(1).map(Into::into).collect();
            assert!(
                crate::cli::parse(&args).is_ok(),
                "{cmd}: {:?}",
                crate::cli::parse(&args)
            );
        }
    }

    #[test]
    fn the_manual_names_every_tab_and_option() {
        let manual = plain(&text());
        let o = Options::default();
        for tab in Tab::ALL {
            assert!(manual.contains(&format!("  {}", tab.title())), "{tab:?}");
            let lists = o.list(tab).into_iter().chain(o.in_file_list());
            for (opt, _) in lists.chain(o.find_list()) {
                let label = opt.label(tab);
                assert!(manual.contains(label), "{tab:?}: {label}");
            }
        }
    }
}
