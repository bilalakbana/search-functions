//! The app's state and what keys do to it. Pure: it returns effects, the main loop carries them out.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsString;
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};

use crate::actions::ActionKind;
use crate::commands::{self, ListPlan, PreviewPlan};
use crate::engines::Engines;
use crate::filter::{Filter, TypeCounts};
use crate::find::{self, Finder, RowWords, Trouble};
use crate::items::{self, Item};
use crate::model::{Opt, Options, Tab};
use crate::preview::{Found, PreviewMsg};
use crate::runner::ListMsg;
use crate::theme;
use crate::versions::VersionRow;
use crate::views::ViewKind;

/// Search runs this long after the last keystroke.
pub const SEARCH_DELAY: Duration = Duration::from_millis(150);
const STATUS_OK: Duration = Duration::from_secs(3);
/// Deep's preview starts once the cursor rests this long: it may search a big binary or archive.
const PREVIEW_DELAY: Duration = Duration::from_millis(100);
/// A preview not here after this long says it is coming, in place of the previous row's.
const PREVIEW_NOTE: Duration = Duration::from_millis(250);
const STATUS_ERROR: Duration = Duration::from_secs(6);
/// Two clicks on the same row within this time are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Ctrl-F gives up after this long (a look-around backtracking on every line of a long preview).
const FIND_BUDGET: Duration = Duration::from_millis(500);
/// Rows (list) or lines (preview) one wheel step moves.
const WHEEL: usize = 3;

/// What the main loop is asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Run `App::list_plan` for `list_gen` (the list is already cleared).
    RunList,
    /// Request `App::preview_plan` for `preview_gen`.
    Preview,
    SaveConfig,
    Act(ActionKind),
    ChangeRoot(PathBuf),
    /// Request the open view's text for `view_gen`.
    View,
    /// Read the versions to choose from (`git log`) for `App::on_versions`.
    LoadVersions,
    Quit,
}

/// Ctrl-T: Content narrowed to one file, the lines with the typed words as its options read them;
/// Esc goes back (or, started so with `sf in FILE`, quits).
#[derive(Clone, Debug)]
pub struct Scope {
    /// The file, as its row named it (relative to the root).
    pub key: OsString,
    /// The tab and the query to go back to; none when sf started in the file (`sf in`): Esc quits.
    back: Option<(Tab, String)>,
}

/// A full-screen view (a diff, a history, a log, a commit) over the tabs; Esc goes back.
#[derive(Debug)]
pub struct View {
    pub kind: ViewKind,
    pub text: Text<'static>,
    /// The first line shown.
    pub scroll: usize,
    pub loading: bool,
    /// The width it was made for (delta lays out side by side to it).
    pub width: u16,
    /// What `/` looks for; `searching` while it is being typed.
    pub search: String,
    pub searching: bool,
    /// The line the search found last; None when it found nothing.
    pub found: Option<usize>,
}

impl View {
    fn line_has(&self, i: usize, needle: &str) -> bool {
        let line: String = self.text.lines[i]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        line.to_lowercase().contains(needle)
    }

    /// The next line holding the search after `from` (forward) or before it, None when none does.
    fn find(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        let needle = self.search.to_lowercase();
        if needle.is_empty() {
            return None;
        }
        let n = self.text.lines.len();
        if forward {
            let start = from.map_or(0, |f| f + 1);
            (start..n).find(|&i| self.line_has(i, &needle))
        } else {
            (0..from.unwrap_or(n))
                .rev()
                .find(|&i| self.line_has(i, &needle))
        }
    }
}

/// Where things were drawn, for the mouse; filled by `ui::draw`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreenLayout {
    pub tabs: Vec<(Tab, Rect)>,
    pub config: Rect,
    /// The rows of the list (inside its border), filled from the bottom: the notes, then the rows.
    pub list: Rect,
    /// The text area of the preview (inside its border); empty when hidden.
    pub preview: Rect,
    /// The popup with its border; empty when closed.
    pub popup: Rect,
    /// The text area of an open view.
    pub view: Rect,
    /// The popup's rows on screen: their index in `App::popup_rows` and where they are.
    pub popup_rows: Vec<(usize, Rect)>,
    /// The text area of the Help tab.
    pub help: Rect,
    /// The preview's and the list's scroll bars (their tracks), where a click or a drag scrolls to;
    /// empty when everything fits.
    pub preview_bar: Rect,
    pub list_bar: Rect,
}

/// Ctrl-F: what is looked for in the preview, everywhere it was found and the current one.
#[derive(Debug, Default)]
pub struct Find {
    pub text: String,
    /// Every match in the preview, in order: its line and the bytes of that line's text.
    pub hits: Vec<(usize, Range<usize>)>,
    /// The current match, an index into `hits`.
    pub current: Option<usize>,
    /// The regular expression typed does not parse, or backtracks past fancy-regex's limit
    /// (Regular expression is on).
    pub bad: bool,
    /// Finding took longer than `FIND_BUDGET`; nothing is shown.
    pub slow: bool,
}

impl Find {
    /// The line of the current match.
    pub fn found_line(&self) -> Option<usize> {
        self.current.map(|c| self.hits[c].0)
    }
}

/// The Help tab: the manual, how far it is scrolled, what is looked for in it.
pub struct Help {
    pub text: Text<'static>,
    /// The first line shown.
    pub scroll: usize,
    /// What typing looks for (the tabs' query is left alone).
    pub search: String,
    /// The line it was found on; None when it is nowhere.
    pub found: Option<usize>,
}

impl Help {
    /// The first line from `from` on, round to the start again, that holds the search; scrolled to.
    fn find(&mut self, from: usize) {
        let needle = self.search.to_lowercase();
        let n = self.text.lines.len();
        let holds = |i: usize| {
            let line: String = self.text.lines[i]
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect();
            line.to_lowercase().contains(&needle)
        };
        self.found = if needle.is_empty() || n == 0 {
            None
        } else {
            (0..n).map(|k| (from + k) % n).find(|&i| holds(i))
        };
        if let Some(i) = self.found {
            self.scroll = i.saturating_sub(2); // with two lines above it
        }
    }
}

/// Git, Ctrl-Alt-F: searching the files of every version of the repository, or of those chosen.
#[derive(Debug, Default)]
pub struct Versions {
    /// The pattern (the Git tab's filter is left alone).
    pub query: String,
    /// The versions to choose from, newest first (`git log`, read once the search starts).
    pub list: Vec<VersionRow>,
    /// Those turned off in the popup, by hash; none: every version.
    pub off: HashSet<String>,
}

/// Ctrl-P in Content and Deep: the files to search in, chosen from those listed; a tab without one
/// searches every file.
#[derive(Clone, Debug)]
pub struct Chosen {
    /// The files the popup lists (those listed when choosing began), in their order.
    pub files: Vec<OsString>,
    /// Those chosen.
    pub on: HashSet<OsString>,
}

/// A row of the Ctrl-S popup.
#[derive(Clone, Debug, PartialEq)]
pub enum PopupRow {
    /// One of the tab's options, and whether it is on.
    Opt(Opt, bool),
    /// Every file type at once: on while none is off.
    AllTypes(bool),
    /// A file type of the rows: its extension (None: none), how many rows have it, whether they show.
    Type(Option<String>, usize, bool),
    /// Every version at once: on while none is off (searching versions).
    AllVersions(bool),
    /// A version to search in, by its hash, and whether it is.
    Version(String, bool),
    /// Ctrl-P: every file at once, on while none is left out.
    AllFiles(bool),
    /// Ctrl-P: a file of the list and whether it is searched in.
    File(OsString, bool),
}

/// A preview that is wanted and not here yet.
#[derive(Debug, Default)]
struct PreviewWait {
    /// The first tick after the row changed.
    since: Option<Instant>,
    /// Its Effect::Preview is out (at once, or in Deep once the cursor rests).
    requested: bool,
    /// The "searching inside …" note stands in for it.
    noted: bool,
}

pub struct App {
    pub tab: Tab,
    pub root: PathBuf,
    /// Shared by all tabs: a filter in Files, Folders and Git, the pattern in Content and Deep.
    pub query: String,
    /// Ctrl-Alt-F in Content and Deep: the pattern stays as it is and these words filter the rows found
    /// (as advancedsearch's fzf did).
    pub result_filter: Option<String>,
    /// sf runs inside tmux: Ctrl-T opens a window of its own.
    pub tmux: bool,
    /// Ctrl-F: finding in the preview; typing goes there, not to the query.
    pub preview_find: Option<Find>,
    /// Git, Ctrl-Alt-F: searching every version; typing is its pattern.
    pub versions: Option<Versions>,
    /// Ctrl-P: each tab's files to search in (Content, Deep; Bilal: each its own).
    pub chosen: Vec<(Tab, Chosen)>,
    /// The popup is Ctrl-P's (the files), not Ctrl-S's.
    pub popup_files: bool,
    /// sf itself, which the versions' search runs as its list (`sf grep-versions`).
    pub self_exe: PathBuf,
    /// `sf history|log|diff FILE`: the view `start` opens instead of a list.
    start_with: Option<ViewKind>,
    /// sf is only that view (a tmux window of its own): Esc quits.
    view_alone: bool,
    pub help: Help,
    /// Engine options from the command line (`sf TAB -- OPTIONS`), for that tab only.
    pub extra: Option<(Tab, Vec<OsString>)>,
    /// The editor's name for the Ctrl-O key in the tab bar.
    pub editor: String,
    /// A diff, history, log or commit shown over the tabs.
    pub view: Option<View>,
    /// Ctrl-T's file, while Content is narrowed to it.
    pub scope: Option<Scope>,
    pub view_gen: u64,
    pub options: Options,
    pub engines: Engines,
    pub filter: Filter,
    /// Hints and engine messages, shown above the rows and never filtered.
    pub notes: Vec<Item>,
    /// Index of the current row among the matching ones.
    pub selected: usize,
    /// First row shown; kept in view by drawing.
    pub offset: usize,
    pub loading: bool,
    pub preview: Text<'static>,
    /// Content's and Deep's pattern with its count in the previewed file (the preview's top right).
    pub preview_found: Option<Found>,
    pub preview_scroll: u16,
    /// The tabs whose preview Ctrl-L has hidden; Files starts without one (Bilal).
    previews_hidden: Vec<Tab>,
    /// The current row's preview, while it is wanted and not here yet.
    preview_wait: Option<PreviewWait>,
    /// File types turned off in Ctrl-S: the tab (each has its own), the type (`items::file_type`) and
    /// how many rows had it then. Not saved; a new root turns every type on again (Bilal).
    types_off: Vec<(Tab, Option<String>, usize)>,
    /// `Filter::types` of the list run `.0` with `.1` rows: the popup asks for it every frame.
    types_seen: RefCell<Option<(u64, usize, TypeCounts)>>,
    /// The Config popup's cursor, while it is open.
    pub popup: Option<usize>,
    /// A message for a moment: text, is an error, since.
    pub status: Option<(String, bool, Instant)>,
    pub list_gen: u64,
    pub preview_gen: u64,
    pub search_due: Option<Instant>,
    pub layout: ScreenLayout,
    /// False when the config file could not be read: it is then left as it is.
    pub save_allowed: bool,
    /// The row the newest preview request is for.
    previewed: Option<(OsString, Option<u32>)>,
    /// The last left click on a list row, for the double click.
    last_click: Option<(usize, Instant)>,
}

impl App {
    pub fn new(
        root: PathBuf,
        tab: Tab,
        options: Options,
        engines: Engines,
        config_error: Option<String>,
        now: Instant,
    ) -> App {
        App {
            tab,
            root,
            query: String::new(),
            result_filter: None,
            tmux: false,
            preview_find: None,
            versions: None,
            chosen: Vec::new(),
            popup_files: false,
            self_exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sf")),
            start_with: None,
            view_alone: false,
            help: Help {
                text: crate::help::text(),
                scroll: 0,
                search: String::new(),
                found: None,
            },
            extra: None,
            editor: "nvim".into(),
            view: None,
            view_gen: 0,
            scope: None,
            options,
            engines,
            filter: Filter::new(),
            notes: Vec::new(),
            selected: 0,
            offset: 0,
            loading: false,
            preview: Text::default(),
            preview_found: None,
            preview_scroll: 0,
            previews_hidden: vec![Tab::Files],
            preview_wait: None,
            types_off: Vec::new(),
            types_seen: RefCell::new(None),
            popup: None,
            save_allowed: config_error.is_none(),
            status: config_error.map(|e| (e, true, now)),
            list_gen: 0,
            preview_gen: 0,
            search_due: None,
            layout: ScreenLayout::default(),
            previewed: None,
            last_click: None,
        }
    }

    /// The effects of starting up.
    pub fn start(&mut self) -> Vec<Effect> {
        if let Some(kind) = self.start_with.take() {
            self.view_alone = true;
            return self.open_view(kind);
        }
        vec![self.run_list()]
    }

    /// `sf history|log|diff FILE`: sf is that view, Esc quits.
    pub fn start_view(&mut self, kind: ViewKind) {
        self.start_with = Some(kind);
    }

    pub fn current(&self) -> Option<&Item> {
        self.filter.get(self.selected)
    }

    /// What the editor highlights (Ctrl-O, vim's search): words read as a Boolean query become any of
    /// them (a phrase in quotes as one, a `-word` left out); a pattern goes as it is. With Whole word,
    /// inside vim's word bounds (very magic `<(…)>`).
    pub fn editor_pattern(&self) -> String {
        let pattern = self.editor_words();
        if self.scope.is_none() && self.options.get(self.tab, Opt::WholeWord) && !pattern.is_empty()
        {
            format!("<({pattern})>")
        } else {
            pattern
        }
    }

    /// `editor_pattern` before Whole word. Without Regular expression each part is vim's very nomagic
    /// (`\V…\v`: only a backslash means something) and words are only apart by spaces, as ugrep gets them.
    fn editor_words(&self) -> String {
        let as_typed = |t: &str| format!("\\V{}\\v", t.replace('\\', "\\\\"));
        if let Some(w) = self.row_words() {
            // a list of matches: any of its words, each found on the line or in the path
            let terms: Vec<String> = w
                .words
                .iter()
                .map(|t| if w.regex { t.clone() } else { as_typed(t) })
                .collect();
            return terms.join("|");
        }
        let words = match self.scope {
            Some(_) => {
                let f = &self.options.in_file;
                f.words_on_a_line || f.words_in_the_file
            }
            None => {
                self.options.get(self.tab, Opt::WordsOnALine)
                    || self.options.get(self.tab, Opt::WordsInTheFile)
            }
        };
        let literal =
            self.scope.is_none() && self.tab.searches() && !self.options.get(self.tab, Opt::Regex);
        if literal {
            return if words {
                let terms: Vec<String> = self.query.split_whitespace().map(as_typed).collect();
                terms.join("|")
            } else if self.query.is_empty() {
                String::new()
            } else {
                as_typed(&self.query)
            };
        }
        if !words {
            return self.pattern();
        }
        let mut terms = Vec::new();
        let mut rest = self.query.trim_start();
        while !rest.is_empty() {
            let (term, after) = match rest.strip_prefix('"') {
                Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
                None => rest.split_once(char::is_whitespace).unwrap_or((rest, "")),
            };
            let term = term.trim_matches(|c| c == '(' || c == ')');
            if !term.is_empty() && !term.starts_with('-') {
                terms.push(term);
            }
            rest = after.trim_start();
        }
        terms.join("|")
    }

    /// What the editor gets (Ctrl-O): `editor_pattern` with vim's case as the options say, `\c` case
    /// aside or `\C` the case as typed; with smart case nothing (actions then gives `\c` to a pattern
    /// without an upper-case letter).
    pub fn editor_query(&self) -> String {
        let pattern = self.editor_pattern();
        let (match_case, smart_case) = match self.scope {
            Some(_) => (
                self.options.in_file.match_case,
                self.options.in_file.smart_case,
            ),
            None => (
                self.options.get(self.tab, Opt::MatchCase),
                self.options.get(self.tab, Opt::SmartCase),
            ),
        };
        if pattern.is_empty() || (smart_case && !match_case) {
            return pattern;
        }
        let case = if match_case { "\\C" } else { "\\c" };
        format!("{case}{pattern}")
    }

    /// Content's and Deep's lists of matches: the words each row keeps on its line or in its path.
    pub fn row_words(&self) -> Option<RowWords> {
        if self.scope.is_some() {
            return None;
        }
        commands::row_words(self.tab, &self.options, &self.query)
    }

    /// What ugrep searches for: the query, in a file (Ctrl-T) as its options read the words.
    pub fn pattern(&self) -> String {
        match self.scope {
            Some(_) => commands::in_file_pattern(&self.query, &self.options.in_file),
            None => self.query.clone(),
        }
    }

    pub fn list_plan(&self) -> ListPlan {
        if let Some(v) = &self.versions {
            let chosen = (!v.off.is_empty()).then(|| {
                v.list
                    .iter()
                    .filter(|r| !v.off.contains(&r.hash))
                    .map(|r| r.hash.clone())
                    .collect()
            });
            return commands::versions_plan(
                &self.self_exe,
                &v.query,
                &self.options.versions,
                chosen,
            );
        }
        if let Some(scope) = &self.scope {
            return commands::in_file_plan(
                &scope.key,
                &self.query,
                &self.options.in_file,
                &self.engines,
            );
        }
        let mut extra = self.extra_options().to_vec();
        if self.tab.searches() {
            // a type turned off is not searched; one whose name a glob would read otherwise is only
            // left out of the rows
            let special = |c: char| "[]*?{}\\,!".contains(c);
            for (_, ty, _) in self.types_off.iter().filter(|(t, _, _)| *t == self.tab) {
                if let Some(ext) = ty.as_ref().filter(|e| !e.contains(special)) {
                    extra.push(format!("--exclude=*.{ext}").into());
                }
            }
        }
        let plan = commands::list_plan_with(
            self.tab,
            &self.options,
            &self.query,
            &self.engines,
            &extra,
            false,
        );
        // Ctrl-P: ugrep searches the files chosen instead of the folder (its last argument, ".")
        match (
            plan,
            self.chosen_of(self.tab).filter(|_| self.tab.searches()),
        ) {
            (
                ListPlan::Run {
                    mut cmd,
                    kind,
                    keep,
                },
                Some(c),
            ) if cmd.args.last().is_some_and(|a| a == ".") => {
                if c.on.is_empty() {
                    return ListPlan::Hint("No file chosen: ^P, All files".into());
                }
                cmd.args.pop();
                cmd.args.push("--".into());
                cmd.args
                    .extend(c.files.iter().filter(|f| c.on.contains(*f)).cloned());
                ListPlan::Run { cmd, kind, keep }
            }
            (plan, _) => plan,
        }
    }

    /// The command line's engine options when they are the current tab's.
    pub fn extra_options(&self) -> &[OsString] {
        match &self.extra {
            Some((tab, extra)) if *tab == self.tab => extra,
            _ => &[],
        }
    }

    pub fn preview_plan(&self) -> PreviewPlan {
        // in a file (Ctrl-T) the rows are always lines: bat at the line, whatever the options say
        let mut options = self.options.clone();
        if self.scope.is_some() {
            let f = &self.options.in_file;
            let c = &mut options.content;
            c.matching_lines = true;
            c.archives = false;
            (c.words_on_a_line, c.words_in_the_file, c.match_case) =
                (f.words_on_a_line, f.words_in_the_file, f.match_case);
        }
        match self.current() {
            Some(item) => commands::preview_plan_with(
                self.tab,
                &options,
                &self.pattern(),
                item,
                self.layout.preview.width.max(20),
                &self.engines,
                &self.root,
                self.extra_options(),
            ),
            None => PreviewPlan::Nothing,
        }
    }

    /// The list plan was a hint: nothing runs, the hint is the list.
    pub fn show_hint(&mut self, hint: &str) {
        self.notes = vec![items::hint(hint)];
        self.loading = false;
        self.preview = Text::default();
        self.preview_found = None;
    }

    pub fn set_status(&mut self, text: &str, error: bool, now: Instant) {
        self.status = Some((text.to_string(), error, now));
    }

    pub fn set_root(&mut self, root: PathBuf) -> Vec<Effect> {
        self.root = root;
        self.types_off.clear();
        self.chosen.clear();
        self.query.clear();
        self.preview = Text::default();
        self.preview_found = None;
        vec![self.run_list()]
    }

    pub fn handle_key(&mut self, key: KeyEvent, now: Instant) -> Vec<Effect> {
        if key.kind == KeyEventKind::Release {
            return Vec::new();
        }
        if self.view.is_some() {
            return self.view_key(key);
        }
        if self.popup.is_some() {
            return self.popup_key(key, now);
        }
        if self.scope.is_some() {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => return self.leave_scope(),
                KeyCode::Char('t') if ctrl => return Vec::new(), // already in it
                _ => {}
            }
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.preview_find.is_some()
            && let Some(effects) = self.find_key(key)
        {
            return effects;
        }
        if self.tab == Tab::Help
            && let Some(effects) = self.help_key(key)
        {
            return effects;
        }
        if self.versions.is_some() {
            let alt = key.modifiers.contains(KeyModifiers::ALT);
            match key.code {
                KeyCode::Esc => return self.leave_versions(),
                KeyCode::Char('f') if ctrl && alt => return self.leave_versions(),
                _ => {}
            }
        }
        if self.tab == Tab::Git && self.scope.is_none() {
            // a file's keys are not bound in Git (Bilal): nothing happens, nothing is said
            let alt = key.modifiers.contains(KeyModifiers::ALT);
            let file_key = match key.code {
                KeyCode::Char('o' | 't' | 'u' | 'g' | 'e') if ctrl && !alt => true,
                KeyCode::Char('g' | 'u') => alt && !ctrl,
                _ => false,
            };
            if file_key {
                return Vec::new();
            }
        }
        if self.result_filter.is_some() {
            match key.code {
                KeyCode::Esc => return self.leave_result_filter(),
                KeyCode::Char('f') if ctrl && key.modifiers.contains(KeyModifiers::ALT) => {
                    return self.leave_result_filter();
                }
                _ => {}
            }
        }
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let page = (self.layout.list.height as usize)
            .saturating_sub(self.notes.len())
            .max(1);
        let preview_page = i32::from(self.layout.preview.height.max(1));
        match key.code {
            KeyCode::Tab => self.switch_tab(self.tab.next()),
            KeyCode::BackTab => self.switch_tab(self.tab.prev()),
            KeyCode::Esc => vec![Effect::Quit],
            KeyCode::Enter => self.enter(now),
            KeyCode::Up if alt => self.scroll_preview(-preview_page),
            KeyCode::Down if alt => self.scroll_preview(preview_page),
            // as fzf: the first row is at the bottom, so Up goes to the next one, Home to the last
            KeyCode::Up => self.select(self.selected + 1),
            KeyCode::Down => self.select(self.selected.saturating_sub(1)),
            KeyCode::PageUp => self.select(self.selected + page),
            KeyCode::PageDown => self.select(self.selected.saturating_sub(page)),
            KeyCode::Home => self.select(usize::MAX),
            KeyCode::End => self.select(0),
            KeyCode::Backspace if alt => self.edit(delete_word, now),
            KeyCode::Backspace => self.edit(
                |q| {
                    q.pop();
                },
                now,
            ),
            KeyCode::Char('h') if ctrl && alt => self.edit(String::clear, now),
            // Ctrl-Alt-F, not Alt-F: Bilal's KDE opens rofi on Alt+F
            KeyCode::Char('f') if ctrl && alt => self.enter_result_filter(),
            KeyCode::Char(c) if ctrl => match c {
                'w' | 'h' => self.edit(delete_word, now),
                'l' => self.toggle_preview(),
                'f' => self.enter_find(now),
                'r' => vec![self.run_list()],
                's' => {
                    self.popup_files = false;
                    self.popup = Some(0);
                    Vec::new()
                }
                'p' => self.open_files(),
                'o' => self.act(ActionKind::Editor, now),
                't' => self.act(ActionKind::InFile, now),
                'u' => self.act(ActionKind::DiffSplit, now),
                'g' => self.act(ActionKind::Diff, now),
                'e' => self.act(ActionKind::Reveal, now),
                'c' => self.act(ActionKind::Copy, now),
                _ => Vec::new(),
            },
            KeyCode::Char(c) if alt => match c {
                'g' => self.act(ActionKind::DiffShort, now),
                'u' => self.act(ActionKind::Log, now),
                _ => Vec::new(),
            },
            KeyCode::Char(c) => self.edit(|q| q.push(c), now),
            _ => Vec::new(),
        }
    }

    /// Clicks and the wheel, on the places `ui::draw` recorded in `layout`.
    pub fn handle_mouse(&mut self, m: MouseEvent, now: Instant) -> Vec<Effect> {
        let at = Position::new(m.column, m.row);
        let left_click = m.kind == MouseEventKind::Down(MouseButton::Left);
        if let Some(v) = self.view.as_mut() {
            let last = v
                .text
                .lines
                .len()
                .saturating_sub(self.layout.view.height as usize);
            match m.kind {
                MouseEventKind::ScrollDown => v.scroll = (v.scroll + 3).min(last),
                MouseEventKind::ScrollUp => v.scroll = v.scroll.saturating_sub(3),
                _ => {}
            }
            return Vec::new();
        }
        if let Some(cursor) = self.popup {
            // the wheel moves the cursor through the rows; the popup scrolls with it
            let last = self.popup_rows().len().saturating_sub(1);
            match m.kind {
                MouseEventKind::ScrollDown => self.popup = Some((cursor + WHEEL).min(last)),
                MouseEventKind::ScrollUp => self.popup = Some(cursor.saturating_sub(WHEEL)),
                _ => {}
            }
            if !left_click {
                return Vec::new();
            }
            if let Some(&(i, _)) = self.layout.popup_rows.iter().find(|(_, r)| r.contains(at)) {
                self.popup = Some(i);
                return self.toggle_row(i, now);
            }
            if !self.layout.popup.contains(at) {
                self.popup = None;
            }
            return Vec::new();
        }
        // a scroll bar clicked or dragged: there
        let on_bar = matches!(
            m.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
        );
        if on_bar {
            let fraction = |bar: Rect| {
                f64::from(m.row.saturating_sub(bar.y))
                    / f64::from(bar.height.saturating_sub(1).max(1))
            };
            if self.layout.preview_bar.contains(at) {
                let last = self
                    .preview
                    .lines
                    .len()
                    .saturating_sub(self.layout.preview.height as usize);
                let to =
                    (fraction(self.layout.preview_bar).min(1.0) * last as f64).round() as usize;
                self.preview_scroll = u16::try_from(to).unwrap_or(u16::MAX);
                return Vec::new();
            }
            if self.layout.list_bar.contains(at) {
                // the rows run upwards: the bar's top is the last row
                let last = self.filter.len().saturating_sub(1);
                let from_top =
                    (fraction(self.layout.list_bar).min(1.0) * last as f64).round() as usize;
                return self.select(last - from_top.min(last));
            }
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(tab, _)) = self.layout.tabs.iter().find(|(_, r)| r.contains(at)) {
                    return if tab == self.tab {
                        Vec::new()
                    } else {
                        self.switch_tab(tab)
                    };
                }
                if self.layout.config.contains(at) {
                    self.popup_files = false;
                    self.popup = Some(0);
                    return Vec::new();
                }
                let Some(i) = self.row_at(at) else {
                    return Vec::new();
                };
                let double = self
                    .last_click
                    .is_some_and(|(j, t)| j == i && now.duration_since(t) <= DOUBLE_CLICK);
                if double {
                    self.last_click = None;
                    return self.enter(now);
                }
                self.last_click = Some((i, now));
                self.select(i)
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = m.kind == MouseEventKind::ScrollDown;
                if self.tab == Tab::Help && self.layout.help.contains(at) {
                    let h = &mut self.help;
                    let page = (self.layout.help.height as usize).max(1);
                    let last = h.text.lines.len().saturating_sub(page);
                    h.scroll = if down {
                        (h.scroll + WHEEL).min(last)
                    } else {
                        h.scroll.saturating_sub(WHEEL)
                    };
                    Vec::new()
                } else if self.layout.preview.contains(at) {
                    self.scroll_preview(if down { WHEEL as i32 } else { -(WHEEL as i32) })
                } else if self.layout.list.contains(at) {
                    // the rows run upwards: the wheel up goes to later rows
                    self.select(if down {
                        self.selected.saturating_sub(WHEEL)
                    } else {
                        self.selected + WHEEL
                    })
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    /// The index of the row under `at` among the matching rows (not a note, not below the last row).
    fn row_at(&self, at: Position) -> Option<usize> {
        if !self.layout.list.contains(at) {
            return None;
        }
        let row = usize::from(self.layout.list.bottom() - 1 - at.y); // counted from the bottom
        let notes = self.notes.len().min(usize::from(self.layout.list.height));
        let i = self.offset + row.checked_sub(notes)?;
        (i < self.filter.len()).then_some(i)
    }

    /// Fires the Search delay and lets the status expire.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        if let Some((_, error, since)) = &self.status {
            if now.duration_since(*since) >= if *error { STATUS_ERROR } else { STATUS_OK } {
                self.status = None;
            }
        }
        let mut effects = self.wait_for_preview(now);
        if matches!(self.search_due, Some(due) if now >= due) {
            effects.push(self.run_list());
        }
        effects
    }

    /// Ctrl-F: finding in the preview (Bilal: Ctrl-F, as in his Claude panes; it was Alt-F first).
    fn enter_find(&mut self, now: Instant) -> Vec<Effect> {
        if self.preview_visible() && self.tab != Tab::Help {
            self.preview_find = Some(Find::default());
        } else {
            self.set_status("No preview to find in: Ctrl-L shows it", true, now);
        }
        Vec::new()
    }

    /// Ctrl-F's keys: type to find, Enter or Down the next match, Up the one before, Ctrl-S Find's
    /// options, Esc or Ctrl-F back to the list; None for Tab and Shift-Tab, which end it and switch,
    /// and for Alt-↑↓, which scroll the preview.
    fn find_key(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let f = self.preview_find.as_mut()?;
        match key.code {
            KeyCode::Esc => self.preview_find = None,
            KeyCode::Char('f') if ctrl => self.preview_find = None,
            KeyCode::Char('s') if ctrl => {
                self.popup_files = false;
                self.popup = Some(0);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.preview_find = None;
                return None;
            }
            // Alt-↑↓ scroll the preview, as outside Ctrl-F; what is found stays
            KeyCode::Up | KeyCode::Down if alt => return None,
            KeyCode::Enter | KeyCode::Down => self.next_match(true),
            KeyCode::Up => self.next_match(false),
            KeyCode::Backspace => {
                f.text.pop();
                self.find_in_preview();
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                f.text.push(c);
                self.find_in_preview();
            }
            _ => {}
        }
        Some(Vec::new())
    }

    /// Every match of what Ctrl-F looks for in the preview, by Find's options; the first is the current
    /// one, scrolled to.
    fn find_in_preview(&mut self) {
        let Some(f) = self.preview_find.as_mut() else {
            return;
        };
        f.hits.clear();
        f.current = None;
        (f.bad, f.slow) = (false, false);
        match Finder::new(&f.text, &self.options.find) {
            Ok(Some(finder)) => {
                let lines = self.preview.lines.iter().map(|line| {
                    line.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                });
                match find::find_all(&finder, lines, FIND_BUDGET) {
                    Ok(hits) => f.hits = hits,
                    // past the backtracking limit: as Notepad++, the expression is "bad"
                    Err(Trouble::Bad) => f.bad = true,
                    Err(Trouble::Slow) => f.slow = true,
                }
            }
            Ok(None) => {}
            Err(_) => f.bad = true,
        }
        if !f.hits.is_empty() {
            f.current = Some(0);
        }
        self.scroll_to_match();
    }

    /// The next match (forward, round to the first) or the one before (round to the last).
    fn next_match(&mut self, forward: bool) {
        let Some(f) = self.preview_find.as_mut() else {
            return;
        };
        let n = f.hits.len();
        if let Some(c) = f.current {
            f.current = Some(if forward {
                (c + 1) % n
            } else {
                (c + n - 1) % n
            });
        }
        self.scroll_to_match();
    }

    /// The current match's line with two lines above it.
    fn scroll_to_match(&mut self) {
        if let Some(i) = self.preview_find.as_ref().and_then(Find::found_line) {
            self.preview_scroll = u16::try_from(i.saturating_sub(2)).unwrap_or(u16::MAX);
        }
    }

    /// The Help tab's keys: scroll, type to find, Enter the next one, Esc clears then quits; None for
    /// the keys every tab has (Tab, Shift-Tab).
    fn help_key(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let page = (self.layout.help.height as usize).max(1);
        let h = &mut self.help;
        let last = h.text.lines.len().saturating_sub(page);
        let ctrl = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => return None,
            KeyCode::Esc if h.search.is_empty() => return Some(vec![Effect::Quit]),
            KeyCode::Esc => {
                h.search.clear();
                h.found = None;
            }
            KeyCode::Up => h.scroll = h.scroll.saturating_sub(1),
            KeyCode::Down => h.scroll = (h.scroll + 1).min(last),
            KeyCode::PageUp => h.scroll = h.scroll.saturating_sub(page),
            KeyCode::PageDown => h.scroll = (h.scroll + page).min(last),
            KeyCode::Home => h.scroll = 0,
            KeyCode::End => h.scroll = last,
            KeyCode::Enter => h.find(h.found.map_or(0, |f| f + 1)),
            KeyCode::Backspace => {
                h.search.pop();
                h.find(0);
            }
            KeyCode::Char(c) if !ctrl => {
                h.search.push(c);
                h.find(0);
            }
            _ => {}
        }
        Some(Vec::new())
    }

    /// Starts Deep's preview once the cursor rests; a preview slow to come says so.
    fn wait_for_preview(&mut self, now: Instant) -> Vec<Effect> {
        let what = if self.tab.searches() {
            "searching inside"
        } else {
            "loading"
        };
        let note = self.current().map(|i| {
            let name = items::printable(&String::from_utf8_lossy(i.key.as_encoded_bytes()));
            format!("{what} {name}…")
        });
        let Some(wait) = &mut self.preview_wait else {
            return Vec::new();
        };
        let waited = now.duration_since(*wait.since.get_or_insert(now));
        let mut effects = Vec::new();
        if !wait.requested && waited >= PREVIEW_DELAY {
            wait.requested = true;
            effects.push(Effect::Preview);
        }
        if !wait.noted && waited >= PREVIEW_NOTE {
            wait.noted = true;
            if let Some(note) = note {
                self.preview =
                    Text::from(Line::from(Span::styled(note, Style::new().fg(theme::DIM))));
                self.preview_scroll = 0;
            }
        }
        effects
    }

    pub fn on_list(&mut self, msg: ListMsg) -> Vec<Effect> {
        match msg {
            ListMsg::Items(g, v) if g == self.list_gen => {
                self.filter.push(v);
                return self.preview_if_changed();
            }
            ListMsg::Error(g, e) if g == self.list_gen => self.notes.push(items::error(&e)),
            ListMsg::Done(g) if g == self.list_gen => {
                self.loading = false;
                // the old preview stays while a list runs (no flicker), but not over an empty one
                if self.filter.len() == 0 {
                    self.preview = Text::default();
                    self.preview_found = None;
                }
                if self.tab.searches() && self.filter.total() == 0 && self.notes.is_empty() {
                    let hint = match self.tab {
                        _ if !self.query.is_empty() => self.no_match(),
                        Tab::Deep => "No files here",
                        _ => "No text files here",
                    };
                    self.notes.push(items::hint(hint));
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// What a search that found nothing says: the option that searches further, while it is off.
    fn no_match(&self) -> &'static str {
        match self.tab {
            Tab::Content if !self.options.content.archives => {
                "No match. Archives & documents (⚙ Config) also searches inside zip, PDF and Office files"
            }
            Tab::Deep if !self.options.deep.documents => {
                "No match. Documents (⚙ Config) also searches inside PDF and Office files"
            }
            _ => "No match",
        }
    }

    pub fn on_preview(&mut self, msg: PreviewMsg) {
        if msg.generation != self.preview_gen {
            return;
        }
        self.preview_wait = None;
        self.preview_found = msg.found;
        if self.preview_find.is_some() {
            // another row's preview while finding: looked for in it from the top
            self.preview = msg.text;
            self.find_in_preview();
            return;
        }
        self.preview = msg.text;
        // a matching line about a third down the preview, below the header
        let third = 1 + self.layout.preview.height as usize / 3;
        let at = self
            .current()
            .and_then(|i| i.line)
            .map_or(0, |n| (n as usize).saturating_sub(third));
        self.preview_scroll = u16::try_from(at).unwrap_or(u16::MAX);
    }

    /// Starts the current tab's list afresh.
    fn run_list(&mut self) -> Effect {
        self.list_gen += 1;
        self.filter.clear();
        self.filter
            .set_match_case(self.options.get(self.tab, Opt::MatchCase));
        self.filter.set_query(match &self.result_filter {
            Some(words) => words,
            None if self.tab.searches() || self.versions.is_some() => "",
            None => &self.query,
        });
        self.filter.set_hidden_types(self.hidden_types());
        self.filter.set_only(self.only());
        self.notes.clear();
        self.selected = 0;
        self.offset = 0;
        self.loading = true;
        self.search_due = None;
        self.previewed = None;
        self.preview_wait = None;
        Effect::RunList
    }

    fn switch_tab(&mut self, tab: Tab) -> Vec<Effect> {
        self.tab = tab;
        self.preview_find = None;
        self.result_filter = None;
        self.versions = None;
        if tab == Tab::Help {
            return Vec::new(); // the manual: no list, no preview
        }
        self.preview = Text::default();
        self.preview_found = None;
        self.preview_scroll = 0;
        vec![self.run_list()]
    }

    fn edit(&mut self, f: impl FnOnce(&mut String), now: Instant) -> Vec<Effect> {
        if let Some(v) = &mut self.versions {
            f(&mut v.query);
            self.search_due = Some(now + SEARCH_DELAY);
            return Vec::new();
        }
        if let Some(words) = &mut self.result_filter {
            f(words);
            self.filter.set_query(words);
            self.selected = 0;
            return self.preview_if_changed();
        }
        f(&mut self.query);
        if self.tab.searches() {
            self.search_due = Some(now + SEARCH_DELAY);
            return Vec::new();
        }
        self.filter.set_query(&self.query);
        self.selected = 0;
        self.preview_if_changed()
    }

    fn select(&mut self, i: usize) -> Vec<Effect> {
        self.selected = i.min(self.filter.len().saturating_sub(1));
        self.preview_if_changed()
    }

    /// A preview request when the current row is another one than the last request's.
    fn preview_if_changed(&mut self) -> Vec<Effect> {
        if !self.preview_visible() {
            self.preview_wait = None;
            return Vec::new();
        }
        let id = self.current().map(|i| (i.key.clone(), i.line));
        if id == self.previewed {
            return Vec::new();
        }
        self.previewed = id;
        self.preview_gen += 1;
        self.preview_scroll = 0;
        if self.previewed.is_none() {
            self.preview = Text::default();
            self.preview_found = None;
            self.preview_wait = None;
            return Vec::new();
        }
        // Deep's preview may search a big binary or archive: it waits for the cursor to rest
        let requested = self.tab != Tab::Deep;
        self.preview_wait = Some(PreviewWait {
            requested,
            ..PreviewWait::default()
        });
        if requested {
            vec![Effect::Preview]
        } else {
            Vec::new()
        }
    }

    /// Requests the current row's preview again (the preview's width changed).
    pub fn refresh_preview(&mut self) -> Vec<Effect> {
        self.previewed = None;
        self.preview_if_changed()
    }

    /// Whether the current tab shows its preview.
    pub fn preview_visible(&self) -> bool {
        !self.previews_hidden.contains(&self.tab)
    }

    fn toggle_preview(&mut self) -> Vec<Effect> {
        match self.previews_hidden.iter().position(|&t| t == self.tab) {
            Some(i) => {
                self.previews_hidden.remove(i);
            }
            None => self.previews_hidden.push(self.tab),
        }
        self.previewed = None;
        self.preview_if_changed()
    }

    fn scroll_preview(&mut self, by: i32) -> Vec<Effect> {
        let last = self.preview.lines.len().saturating_sub(1) as i32;
        self.preview_scroll = (i32::from(self.preview_scroll) + by).clamp(0, last.max(0)) as u16;
        Vec::new()
    }

    fn enter(&mut self, now: Instant) -> Vec<Effect> {
        let Some(item) = self.current().filter(|i| i.selectable) else {
            self.set_status("Nothing selected", true, now);
            return Vec::new();
        };
        match self.tab {
            Tab::Folders if item.key == ".." => {
                vec![Effect::ChangeRoot(
                    self.root.parent().unwrap_or(&self.root).to_path_buf(),
                )]
            }
            Tab::Folders => vec![Effect::ChangeRoot(self.root.join(&item.key))],
            Tab::Git if item.version().is_some() => {
                let (rev, path) = item.version().unwrap_or_default();
                let kind = ViewKind::Version {
                    root: self.root.clone(),
                    rev: rev.to_string(),
                    path: path.to_string(),
                    line: item.line.unwrap_or(1),
                };
                self.open_view(kind)
            }
            Tab::Git => match item.key.to_str().filter(|h| commands::is_hash(h)) {
                Some(hash) => self.open_view(ViewKind::Commit {
                    root: self.root.clone(),
                    hash: hash.to_string(),
                    whole: self.options.git.whole_files,
                }),
                None => {
                    self.set_status("Not a commit", true, now);
                    Vec::new()
                }
            },
            _ => vec![Effect::Act(ActionKind::Open)],
        }
    }

    fn act(&mut self, kind: ActionKind, now: Instant) -> Vec<Effect> {
        let Some(item) = self.current().filter(|i| i.selectable) else {
            self.set_status("Nothing selected", true, now);
            return Vec::new();
        };
        if kind == ActionKind::InFile && matches!(self.tab, Tab::Files | Tab::Content | Tab::Deep) {
            if self.tmux {
                return vec![Effect::Act(kind)]; // a window of its own (Bilal)
            }
            let key = item.key.clone();
            return self.enter_scope(key);
        }
        let path = self.root.join(&item.key);
        let view = match (self.tab, kind) {
            (Tab::Git, _) => None, // the commit's keys; the others say they are not for Git
            (_, ActionKind::Diff) => Some(ViewKind::Diff { path, full: true }),
            (_, ActionKind::DiffShort) => Some(ViewKind::Diff { path, full: false }),
            (_, ActionKind::DiffSplit) => Some(ViewKind::History {
                path,
                whole: self.options.git.whole_files, // Git's option (Bilal)
            }),
            (_, ActionKind::Log) => Some(ViewKind::Log { path }),
            _ => None,
        };
        match view {
            Some(_) if self.tmux => vec![Effect::Act(kind)], // a window of its own (Bilal)
            Some(kind) => self.open_view(kind),
            None => vec![Effect::Act(kind)],
        }
    }

    fn enter_scope(&mut self, key: OsString) -> Vec<Effect> {
        self.result_filter = None;
        self.scope = Some(Scope {
            key,
            back: Some((self.tab, self.query.clone())),
        });
        if !self.tab.searches() {
            self.query.clear(); // a filter of names is no pattern for the lines
        }
        self.tab = Tab::Content;
        self.preview = Text::default();
        self.preview_found = None;
        self.preview_scroll = 0;
        vec![self.run_list()]
    }

    /// `sf in FILE WORDS`: inside the file from the start; Esc quits.
    pub fn start_in_file(&mut self, file: OsString, query: String) {
        self.tab = Tab::Content;
        self.query = query;
        self.scope = Some(Scope {
            key: file,
            back: None,
        });
    }

    /// What Ctrl-T's window searches the file for: Content's or Deep's pattern; a filter of names
    /// is none.
    pub fn in_file_query(&self) -> &str {
        if self.tab.searches() { &self.query } else { "" }
    }

    /// The query a start from the command line gives: a pattern for Content or Deep is kept and what
    /// is typed filters the rows found (Bilal: as `advancedsearch PATTERN`).
    pub fn start_query(&mut self, query: String) {
        if self.tab.searches() && !query.is_empty() {
            self.result_filter = Some(String::new());
        }
        self.query = query;
    }

    /// Ctrl-Alt-F: the pattern stays, typing filters the rows found.
    fn enter_result_filter(&mut self) -> Vec<Effect> {
        if self.tab == Tab::Git && self.scope.is_none() {
            return self.enter_versions(); // Bilal: Ctrl-(Shift-)Alt-F in Git
        }
        if self.tab.searches() && self.scope.is_none() {
            self.result_filter = Some(String::new());
        }
        Vec::new()
    }

    /// Git, Ctrl-Alt-F: typing searches every version; the versions to choose from are read.
    fn enter_versions(&mut self) -> Vec<Effect> {
        self.versions = Some(Versions::default());
        self.preview = Text::default();
        self.preview_found = None;
        vec![Effect::LoadVersions, self.run_list()]
    }

    /// Esc or Ctrl-Alt-F again: the commits, with the Git tab's filter, again.
    fn leave_versions(&mut self) -> Vec<Effect> {
        self.versions = None;
        self.preview = Text::default();
        self.preview_found = None;
        vec![self.run_list()]
    }

    /// The versions to choose from, newest first.
    pub fn on_versions(&mut self, list: Vec<VersionRow>) {
        if let Some(v) = &mut self.versions {
            v.list = list;
        }
    }

    /// Esc or Ctrl-Alt-F again: typing edits the pattern again; every row found is back.
    fn leave_result_filter(&mut self) -> Vec<Effect> {
        self.result_filter = None;
        self.filter.set_query("");
        self.selected = 0;
        self.preview_if_changed()
    }

    fn leave_scope(&mut self) -> Vec<Effect> {
        let Some(scope) = self.scope.take() else {
            return Vec::new();
        };
        let Some(back) = scope.back else {
            return vec![Effect::Quit]; // `sf in FILE`, Ctrl-T's tmux window
        };
        (self.tab, self.query) = back;
        self.preview = Text::default();
        self.preview_found = None;
        self.preview_scroll = 0;
        vec![self.run_list()]
    }

    fn open_view(&mut self, kind: ViewKind) -> Vec<Effect> {
        self.view_gen += 1;
        self.view = Some(View {
            kind,
            text: Text::default(),
            scroll: 0,
            loading: true,
            width: 0,
            search: String::new(),
            searching: false,
            found: None,
        });
        vec![Effect::View]
    }

    /// A view's text arrived; an empty one says why.
    pub fn on_view(&mut self, msg: PreviewMsg) {
        let generation = self.view_gen;
        let Some(v) = self.view.as_mut().filter(|_| msg.generation == generation) else {
            return;
        };
        v.text = if msg.text.lines.is_empty() {
            Text::styled(
                v.kind.empty_note(),
                ratatui::style::Style::new().fg(crate::theme::DIM),
            )
        } else {
            msg.text
        };
        v.loading = false;
        // a file as it was in a version: the line found a third down, as in the preview
        if let ViewKind::Version { line, .. } = v.kind {
            let third = self.layout.view.height as usize / 3;
            v.scroll = (line as usize).saturating_sub(1).saturating_sub(third);
        }
    }

    /// Keys while a view is open: they never reach the tabs.
    fn view_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let page = (self.layout.view.height as usize).max(1);
        let alone = self.view_alone;
        let Some(v) = self.view.as_mut() else {
            return Vec::new();
        };
        if v.searching {
            match key.code {
                KeyCode::Esc => {
                    v.searching = false;
                    v.search.clear();
                }
                KeyCode::Enter => {
                    v.searching = false;
                    v.found = v.find(v.scroll.checked_sub(1), true);
                }
                KeyCode::Backspace => {
                    v.search.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    v.search.push(c)
                }
                _ => {}
            }
            v.scroll = v.found.unwrap_or(v.scroll);
            return Vec::new();
        }
        let last = v.text.lines.len().saturating_sub(page);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') if alone => return vec![Effect::Quit],
            KeyCode::Esc | KeyCode::Char('q') => {
                self.view = None;
                return Vec::new();
            }
            KeyCode::Up | KeyCode::Char('k') => v.scroll = v.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => v.scroll = (v.scroll + 1).min(last),
            KeyCode::PageUp | KeyCode::Char('b') => v.scroll = v.scroll.saturating_sub(page),
            KeyCode::PageDown | KeyCode::Char(' ') => v.scroll = (v.scroll + page).min(last),
            KeyCode::Home | KeyCode::Char('g') => v.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => v.scroll = last,
            KeyCode::Char('/') => {
                v.searching = true;
                v.search.clear();
            }
            KeyCode::Char(c @ ('n' | 'N')) => {
                if let Some(i) = v.find(v.found, c == 'n') {
                    v.found = Some(i);
                    v.scroll = i;
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn popup_key(&mut self, key: KeyEvent, now: Instant) -> Vec<Effect> {
        let Some(cursor) = self.popup else {
            return Vec::new();
        };
        let n = self.popup_rows().len();
        match key.code {
            KeyCode::Esc => self.popup = None,
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.popup = None
            }
            KeyCode::Up => self.popup = Some((cursor + n - 1) % n),
            KeyCode::Down => self.popup = Some((cursor + 1) % n),
            // across the columns: the row on the same line of the next or previous one
            KeyCode::Left | KeyCode::Right => {
                let rows = &self.layout.popup_rows;
                if let Some(&(_, here)) = rows.iter().find(|(i, _)| *i == cursor) {
                    let right = key.code == KeyCode::Right;
                    let next = rows
                        .iter()
                        .filter(|(_, r)| r.y == here.y && (r.x > here.x) == right && r.x != here.x)
                        .min_by_key(|(_, r)| r.x.abs_diff(here.x));
                    if let Some(&(i, _)) = next {
                        self.popup = Some(i);
                    }
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => return self.toggle_row(cursor, now),
            KeyCode::Char('a') => {
                if let Some(i) = self.popup_rows().iter().position(|r| {
                    matches!(
                        r,
                        PopupRow::AllTypes(_) | PopupRow::AllVersions(_) | PopupRow::AllFiles(_)
                    )
                }) {
                    return self.toggle_row(i, now);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// The popup's rows: the tab's options, then (Files, Content, Deep) every file type of the rows,
    /// the most first, and those turned off.
    pub fn popup_rows(&self) -> Vec<PopupRow> {
        if self.popup_files {
            let chosen = self.chosen_of(self.tab);
            let files = chosen.map_or_else(|| self.filter.files(), |c| c.files.clone());
            let mut rows = vec![PopupRow::AllFiles(chosen.is_none())];
            rows.extend(files.into_iter().map(|f| {
                let on = chosen.is_none_or(|c| c.on.contains(&f));
                PopupRow::File(f, on)
            }));
            return rows;
        }
        let options = match self.scope {
            _ if self.preview_find.is_some() => self.options.find_list(),
            _ if self.versions.is_some() => self.options.versions_list(),
            Some(_) => self.options.in_file_list(),
            None => self.options.list(self.tab),
        };
        let mut rows: Vec<PopupRow> = options
            .into_iter()
            .map(|(opt, on)| PopupRow::Opt(opt, on))
            .collect();
        if let Some(v) = self
            .versions
            .as_ref()
            .filter(|_| self.preview_find.is_none())
        {
            if !v.list.is_empty() {
                rows.push(PopupRow::AllVersions(v.off.is_empty()));
                rows.extend(
                    v.list
                        .iter()
                        .map(|r| PopupRow::Version(r.hash.clone(), !v.off.contains(&r.hash))),
                );
            }
            return rows;
        }
        if !matches!(self.tab, Tab::Files | Tab::Content | Tab::Deep)
            || self.scope.is_some()
            || self.preview_find.is_some()
        {
            return rows;
        }
        let mut counts = self.types_of_rows();
        let off: Vec<(Option<String>, usize)> = self
            .types_off
            .iter()
            .filter(|(t, _, _)| *t == self.tab)
            .map(|(_, ty, n)| (ty.clone(), *n))
            .collect();
        for (ty, n) in &off {
            counts.entry(ty.clone()).or_insert(*n); // searched no more: the count from before
        }
        if counts.is_empty() {
            return rows;
        }
        let mut types: Vec<(Option<String>, usize)> = counts.into_iter().collect();
        types.sort_by(|(a, n), (b, m)| m.cmp(n).then(a.is_none().cmp(&b.is_none())).then(a.cmp(b)));
        rows.push(PopupRow::AllTypes(off.is_empty()));
        rows.extend(types.into_iter().map(|(ty, n)| {
            let on = !off.iter().any(|(o, _)| *o == ty);
            PopupRow::Type(ty, n, on)
        }));
        rows
    }

    /// `Filter::types`, counted again only when the list changed.
    fn types_of_rows(&self) -> TypeCounts {
        let key = (self.list_gen, self.filter.total());
        let mut seen = self.types_seen.borrow_mut();
        match &*seen {
            Some((g, n, types)) if (*g, *n) == key => types.clone(),
            _ => {
                let types = self.filter.types();
                *seen = Some((key.0, key.1, types.clone()));
                types
            }
        }
    }

    /// How many of the current tab's file types are turned off (the input line says so).
    pub fn types_off_count(&self) -> usize {
        self.hidden_types().len()
    }

    /// Ctrl-P: the popup of the files to search in (Content and Deep, not inside one file).
    fn open_files(&mut self) -> Vec<Effect> {
        if self.tab.searches() && self.scope.is_none() {
            self.popup_files = true;
            self.popup = Some(0);
        }
        Vec::new()
    }

    /// `tab`'s files to search in, when some are chosen.
    pub fn chosen_of(&self, tab: Tab) -> Option<&Chosen> {
        self.chosen.iter().find(|(t, _)| *t == tab).map(|(_, c)| c)
    }

    /// The rows the current tab's chosen files allow (None: every one).
    fn only(&self) -> Option<HashSet<OsString>> {
        if self.scope.is_some() || !self.tab.searches() {
            return None;
        }
        self.chosen_of(self.tab).map(|c| c.on.clone())
    }

    /// The files chosen changed: the rows of the others leave at once (the next search is in the
    /// chosen ones only, `list_plan`).
    fn files_changed(&mut self) -> Vec<Effect> {
        self.filter.set_only(self.only());
        self.selected = 0;
        self.preview_if_changed()
    }

    /// The current tab's file types turned off (none inside one file, Ctrl-T).
    fn hidden_types(&self) -> Vec<Option<String>> {
        if self.scope.is_some() {
            return Vec::new();
        }
        self.types_off
            .iter()
            .filter(|(t, _, _)| *t == self.tab)
            .map(|(_, ty, _)| ty.clone())
            .collect()
    }

    /// Space or a click on the popup's row `index`.
    pub fn toggle_row(&mut self, index: usize, now: Instant) -> Vec<Effect> {
        let tab = self.tab;
        match self.popup_rows().get(index).cloned() {
            Some(PopupRow::Opt(..)) => return self.toggle_option(index, now),
            Some(PopupRow::AllFiles(all)) => {
                if all {
                    // none chosen: nothing is searched until one is
                    let files = self
                        .chosen_of(tab)
                        .map_or_else(|| self.filter.files(), |c| c.files.clone());
                    self.chosen.retain(|(t, _)| *t != tab);
                    let on = HashSet::new();
                    self.chosen.push((tab, Chosen { files, on }));
                    return self.files_changed();
                }
                // every file again: their rows were never found, so the search runs again
                self.chosen.retain(|(t, _)| *t != tab);
                return vec![self.run_list()];
            }
            Some(PopupRow::File(file, on)) => {
                let files = self.filter.files();
                let i = match self.chosen.iter().position(|(t, _)| *t == tab) {
                    Some(i) => i,
                    None => {
                        let on = files.iter().cloned().collect();
                        self.chosen.push((tab, Chosen { files, on }));
                        self.chosen.len() - 1
                    }
                };
                let c = &mut self.chosen[i].1;
                if on {
                    // left out: its rows leave at once
                    c.on.remove(&file);
                    return self.files_changed();
                }
                // chosen again: searched again (its rows were not looked for)
                c.on.insert(file);
                if c.on.len() == c.files.len() {
                    self.chosen.remove(i); // every one again: as before
                }
                return vec![self.run_list()];
            }
            Some(PopupRow::AllVersions(all)) => {
                if let Some(v) = &mut self.versions {
                    v.off = if all {
                        v.list.iter().map(|r| r.hash.clone()).collect()
                    } else {
                        HashSet::new()
                    };
                }
                return vec![self.run_list()];
            }
            Some(PopupRow::Version(hash, on)) => {
                if let Some(v) = &mut self.versions {
                    if on {
                        v.off.insert(hash);
                    } else {
                        v.off.remove(&hash);
                    }
                }
                return vec![self.run_list()];
            }
            Some(PopupRow::AllTypes(true)) => {
                // every type off, each with its count
                let rows = self.popup_rows();
                for row in rows {
                    if let PopupRow::Type(ty, n, true) = row {
                        self.types_off.push((tab, ty, n));
                    }
                }
            }
            Some(PopupRow::AllTypes(false)) => self.types_off.retain(|(t, _, _)| *t != tab),
            Some(PopupRow::Type(ty, n, true)) => self.types_off.push((tab, ty, n)),
            Some(PopupRow::Type(ty, _, false)) => {
                self.types_off.retain(|(t, o, _)| !(*t == tab && *o == ty))
            }
            None => return Vec::new(),
        }
        if self.tab.searches() {
            // the types stand above the files chosen (Bilal): a change of them ends the choice
            self.chosen.retain(|(t, _)| *t != tab);
            return vec![self.run_list()]; // ugrep leaves them out (`list_plan`)
        }
        self.filter.set_hidden_types(self.hidden_types());
        self.selected = 0;
        self.preview_if_changed()
    }

    /// Toggles the current tab's option at `index` (popup order); while finding, Find's.
    pub fn toggle_option(&mut self, index: usize, now: Instant) -> Vec<Effect> {
        if self.preview_find.is_some() {
            let Some(&(opt, _)) = self.options.find_list().get(index) else {
                return Vec::new();
            };
            self.options.toggle_find(opt);
            self.find_in_preview();
            return self.save_options(Vec::new(), now);
        }
        if self.versions.is_some() {
            let Some(&(opt, _)) = self.options.versions_list().get(index) else {
                return Vec::new();
            };
            self.options.toggle_versions(opt);
            let fx = vec![self.run_list()];
            return self.save_options(fx, now);
        }
        if self.scope.is_some() {
            let Some(&(opt, _)) = self.options.in_file_list().get(index) else {
                return Vec::new();
            };
            self.options.toggle_in_file(opt);
        } else {
            let Some(&(opt, _)) = self.options.list(self.tab).get(index) else {
                return Vec::new();
            };
            self.options.toggle(self.tab, opt);
        }
        let fx = vec![self.run_list()];
        self.save_options(fx, now)
    }

    /// `fx` and the options saved, or a note that they cannot be.
    fn save_options(&mut self, mut fx: Vec<Effect>, now: Instant) -> Vec<Effect> {
        if self.save_allowed {
            fx.push(Effect::SaveConfig);
        } else {
            self.set_status(
                "Options not saved: the config file could not be read",
                true,
                now,
            );
        }
        fx
    }
}

/// Deletes the word before the end and the spaces after it (unix-word-rubout).
fn delete_word(q: &mut String) {
    q.truncate(q.trim_end().len());
    let start = q
        .rfind(char::is_whitespace)
        .map_or(0, |i| i + q[i..].chars().next().map_or(1, char::len_utf8));
    q.truncate(start);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::commands::ParseKind;
    use ratatui::text::Line;

    pub fn engines() -> Engines {
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

    pub fn app(tab: Tab) -> (App, Instant) {
        let now = Instant::now();
        let mut a = App::new(
            PathBuf::from("/r/p"),
            tab,
            Options::default(),
            engines(),
            None,
            now,
        );
        assert_eq!(a.start(), [Effect::RunList]);
        (a, now)
    }

    /// An app whose tab shows its preview whatever the tab's default (Files starts without one).
    pub fn app_with_preview(tab: Tab) -> (App, Instant) {
        let (mut a, now) = app(tab);
        a.previews_hidden.clear();
        (a, now)
    }

    #[test]
    fn engine_options_go_to_their_tab_only() {
        let (mut a, now) = app(Tab::Files);
        a.extra = Some((Tab::Files, vec!["-e".into(), "rs".into()]));
        let ListPlan::Run { cmd, .. } = a.list_plan() else {
            panic!()
        };
        assert_eq!(
            &cmd.args[cmd.args.len() - 2..],
            ["-e", "rs"].map(OsString::from)
        );
        a.handle_key(key(KeyCode::Tab), now);
        assert_eq!(a.tab, Tab::Content);
        let ListPlan::Run { cmd, .. } = a.list_plan() else {
            panic!()
        };
        assert!(!cmd.args.contains(&"rs".into()), "{:?}", cmd.args);
    }

    /// A view's text of `n` lines "line 0" .. "line n-1", arrived for the current view.
    fn view_text(a: &mut App, n: usize) {
        let text = Text::from(
            (0..n)
                .map(|i| Line::from(format!("line {i}")))
                .collect::<Vec<_>>(),
        );
        a.on_view(PreviewMsg {
            generation: a.view_gen,
            text,
            found: None,
        });
    }

    #[test]
    fn git_keys_open_views_of_the_selected_file() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["src/a.c"]);
        let path = PathBuf::from("/r/p/src/a.c");
        for (k, kind) in [
            (
                ctrl('g'),
                ViewKind::Diff {
                    path: path.clone(),
                    full: true,
                },
            ),
            (
                alt(KeyCode::Char('g')),
                ViewKind::Diff {
                    path: path.clone(),
                    full: false,
                },
            ),
            (
                ctrl('u'),
                ViewKind::History {
                    path: path.clone(),
                    whole: false,
                },
            ),
            (
                alt(KeyCode::Char('u')),
                ViewKind::Log { path: path.clone() },
            ),
        ] {
            a.view = None;
            assert_eq!(a.handle_key(k, now), [Effect::View]);
            let v = a.view.as_ref().unwrap();
            assert_eq!((&v.kind, v.loading), (&kind, true));
        }
    }

    #[test]
    fn enter_in_git_views_the_commit() {
        let (mut a, now) = app(Tab::Git);
        a.on_list(ListMsg::Items(
            a.list_gen,
            rows(ParseKind::Git, &["* x\x1fabcdef1"]),
        ));
        assert_eq!(a.handle_key(key(KeyCode::Enter), now), [Effect::View]);
        assert_eq!(
            a.view.as_ref().unwrap().kind,
            ViewKind::Commit {
                root: "/r/p".into(),
                hash: "abcdef1".into(),
                whole: false
            }
        );
    }

    #[test]
    fn a_view_scrolls_by_line_page_and_end_and_esc_closes_it() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        a.handle_key(ctrl('g'), now);
        a.layout.view = Rect::new(1, 1, 80, 10);
        view_text(&mut a, 100);
        let scroll = |a: &App| a.view.as_ref().unwrap().scroll;
        a.handle_key(key(KeyCode::Down), now);
        assert_eq!(scroll(&a), 1);
        a.handle_key(key(KeyCode::PageDown), now);
        assert_eq!(scroll(&a), 11);
        a.handle_key(key(KeyCode::End), now);
        assert_eq!(scroll(&a), 90, "the last page");
        a.handle_key(key(KeyCode::PageDown), now);
        assert_eq!(scroll(&a), 90);
        a.handle_key(key(KeyCode::Home), now);
        assert_eq!(scroll(&a), 0);
        assert!(
            a.handle_key(key(KeyCode::Esc), now).is_empty(),
            "Esc leaves the view, not sf"
        );
        assert!(a.view.is_none());
    }

    #[test]
    fn in_tmux_the_git_keys_are_windows_of_their_own() {
        for (k, kind) in [
            (ctrl('u'), ActionKind::DiffSplit),
            (alt(KeyCode::Char('u')), ActionKind::Log),
            (ctrl('g'), ActionKind::Diff),
            (alt(KeyCode::Char('g')), ActionKind::DiffShort),
        ] {
            let (mut a, now) = app(Tab::Files);
            a.tmux = true;
            feed(&mut a, &["a.c"]);
            assert_eq!(a.handle_key(k, now), [Effect::Act(kind)], "{kind:?}");
            assert!(a.view.is_none());
        }
    }

    #[test]
    fn sf_started_on_a_view_quits_with_it() {
        let (mut a, now) = app(Tab::Files);
        let kind = ViewKind::Log {
            path: "/r/p/a.c".into(),
        };
        a.start_view(kind.clone());
        assert_eq!(a.start(), [Effect::View], "the view, no list");
        assert_eq!(a.view.as_ref().unwrap().kind, kind);
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::Quit]);
    }

    #[test]
    fn ctrl_f_finds_in_the_preview() {
        let (mut a, now) = app_with_preview(Tab::Git);
        feed(&mut a, &["abcdef1"]);
        a.layout.preview = Rect::new(1, 2, 80, 10);
        let lines: Vec<Line> = (0..60)
            .map(|i| {
                Line::raw(if i % 20 == 7 {
                    format!("{i} Needle")
                } else {
                    format!("{i}")
                })
            })
            .collect();
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::from(lines),
        });
        assert_eq!(a.handle_key(ctrl('f'), now), []);
        assert!(a.preview_find.is_some());
        typed(&mut a, "needle", now);
        assert_eq!(a.query, "", "the list's filter is left alone");
        assert_eq!(a.preview_find.as_ref().unwrap().found_line(), Some(7));
        assert_eq!(a.preview_scroll, 5, "with two lines above it");
        a.handle_key(key(KeyCode::Enter), now);
        assert_eq!(
            a.preview_find.as_ref().unwrap().found_line(),
            Some(27),
            "Enter: the next"
        );
        a.handle_key(key(KeyCode::Down), now);
        assert_eq!(a.preview_find.as_ref().unwrap().found_line(), Some(47));
        a.handle_key(key(KeyCode::Up), now);
        assert_eq!(
            a.preview_find.as_ref().unwrap().found_line(),
            Some(27),
            "Up: the one before"
        );
        assert_eq!(
            a.handle_key(key(KeyCode::Esc), now),
            [],
            "Esc: back to the list"
        );
        assert!(a.preview_find.is_none());
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::Quit]);
    }

    /// Git with a commit and `lines` as its preview, finding (Ctrl-F) in it.
    fn finding(lines: &[&str]) -> (App, Instant) {
        let (mut a, now) = app_with_preview(Tab::Git);
        feed(&mut a, &["abcdef1"]);
        a.layout.preview = Rect::new(1, 2, 80, 10);
        let text: Vec<Line> = lines.iter().map(|l| Line::raw(l.to_string())).collect();
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::from(text),
        });
        a.handle_key(ctrl('f'), now);
        (a, now)
    }

    /// The current match: its line and byte range.
    fn current(a: &App) -> Option<(usize, std::ops::Range<usize>)> {
        let f = a.preview_find.as_ref().unwrap();
        f.current.map(|c| f.hits[c].clone())
    }

    #[test]
    fn ctrl_f_goes_from_match_to_match_round_and_counts_them() {
        let (mut a, now) = finding(&["a x x", "b", "X"]);
        typed(&mut a, "x", now);
        assert_eq!(a.preview_find.as_ref().unwrap().hits.len(), 3);
        assert_eq!(current(&a), Some((0, 2..3)), "the first, from the top");
        a.handle_key(key(KeyCode::Enter), now);
        assert_eq!(current(&a), Some((0, 4..5)), "the next on the same line");
        a.handle_key(key(KeyCode::Down), now);
        assert_eq!(current(&a), Some((2, 0..1)), "case aside");
        a.handle_key(key(KeyCode::Enter), now);
        assert_eq!(current(&a), Some((0, 2..3)), "round to the first");
        a.handle_key(key(KeyCode::Up), now);
        assert_eq!(current(&a), Some((2, 0..1)), "round to the last");
        a.handle_key(key(KeyCode::Backspace), now);
        let f = a.preview_find.as_ref().unwrap();
        assert!(f.hits.is_empty() && f.current.is_none(), "nothing typed");
    }

    #[test]
    fn ctrl_s_while_finding_shows_finds_options_and_finds_again() {
        let (mut a, now) = finding(&["config configs", "reconfig config"]);
        typed(&mut a, "config", now);
        assert_eq!(a.preview_find.as_ref().unwrap().hits.len(), 4);
        assert_eq!(a.handle_key(ctrl('s'), now), []);
        assert_eq!(
            a.popup_rows(),
            [
                PopupRow::Opt(Opt::WholeWord, false),
                PopupRow::Opt(Opt::Regex, false),
                PopupRow::Opt(Opt::SmartCase, false),
                PopupRow::Opt(Opt::MatchCase, false)
            ],
            "Find's options, no file types"
        );
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::SaveConfig],
            "saved; the list does not run again"
        );
        assert!(a.options.find.whole_word && !a.options.git.match_case);
        let f = a.preview_find.as_ref().unwrap();
        assert_eq!(f.hits, [(0, 0..6), (1, 9..15)], "found again, whole words");
        assert_eq!(f.current, Some(0));
        a.handle_key(key(KeyCode::Esc), now);
        assert!(
            a.popup.is_none() && a.preview_find.is_some(),
            "Esc closes the popup only"
        );
        assert_eq!(a.preview_find.as_ref().unwrap().text, "config");
    }

    #[test]
    fn a_regular_expression_that_does_not_parse_finds_nothing_and_says_so() {
        let (mut a, now) = finding(&["load looad"]);
        a.options.find.regex = true;
        typed(&mut a, "lo+ad", now);
        assert_eq!(a.preview_find.as_ref().unwrap().hits.len(), 2);
        typed(&mut a, "(", now);
        let f = a.preview_find.as_ref().unwrap();
        assert!(f.bad && f.hits.is_empty() && f.current.is_none());
        a.handle_key(key(KeyCode::Enter), now);
        a.handle_key(key(KeyCode::Backspace), now);
        let f = a.preview_find.as_ref().unwrap();
        assert!(!f.bad && f.hits.len() == 2, "{:?}", f.hits);
    }

    #[test]
    fn alt_arrows_scroll_the_preview_while_finding() {
        let lines: Vec<String> = (0..100).map(|i| format!("{i} x")).collect();
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (mut a, now) = finding(&lines);
        typed(&mut a, "x", now);
        let (scroll, current) = (a.preview_scroll, a.preview_find.as_ref().unwrap().current);
        a.handle_key(alt(KeyCode::Down), now);
        assert!(a.preview_scroll > scroll, "a page down");
        assert_eq!(
            a.preview_find.as_ref().unwrap().current,
            current,
            "the same match"
        );
        a.handle_key(alt(KeyCode::Up), now);
        assert_eq!(a.preview_scroll, scroll);
        assert!(a.preview_find.is_some(), "still finding");
    }

    #[test]
    fn a_regular_expression_too_hard_to_finish_says_so() {
        let long = "a".repeat(40);
        let (mut a, now) = finding(&["ab", &long]);
        a.options.find.regex = true;
        typed(&mut a, r"(a|aa)+\1b", now);
        let f = a.preview_find.as_ref().unwrap();
        assert!(
            f.bad && f.hits.is_empty() && f.current.is_none(),
            "{:?}",
            f.hits
        );
    }

    #[test]
    fn another_rows_preview_is_found_in_from_the_top() {
        let (mut a, now) = finding(&["needle"]);
        typed(&mut a, "needle", now);
        a.handle_key(key(KeyCode::Enter), now);
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::from(vec![Line::raw("a"), Line::raw("b needle needle")]),
        });
        let f = a.preview_find.as_ref().unwrap();
        assert_eq!((f.hits.len(), current(&a)), (2, Some((1, 2..8))));
    }

    #[test]
    fn find_options_not_saved_when_the_config_could_not_be_read() {
        let (mut a, now) = finding(&["x"]);
        a.save_allowed = false;
        a.handle_key(ctrl('s'), now);
        assert_eq!(a.handle_key(key(KeyCode::Char(' ')), now), []);
        assert!(a.options.find.whole_word, "on for this run");
        assert!(a.status.as_ref().unwrap().0.contains("not saved"));
    }

    #[test]
    fn ctrl_f_without_a_preview_says_so() {
        let (mut a, now) = app(Tab::Files); // Files starts without its preview
        a.handle_key(ctrl('f'), now);
        assert!(a.preview_find.is_none());
        assert!(
            a.status.as_ref().unwrap().0.contains("Ctrl-L"),
            "{:?}",
            a.status
        );
    }

    #[test]
    fn ctrl_t_in_a_view_does_nothing() {
        // / searches a view (Bilal: Ctrl-T there is not needed)
        for open in [
            ctrl('u'),
            alt(KeyCode::Char('u')),
            ctrl('g'),
            alt(KeyCode::Char('g')),
        ] {
            let (mut a, now) = app(Tab::Files);
            feed(&mut a, &["a"]);
            a.handle_key(open, now);
            assert_eq!(
                a.handle_key(ctrl('t'), now),
                [],
                "no window, no other effect"
            );
            let v = a.view.as_ref().expect("the view stays");
            assert!(!v.searching, "{:?}: no search", v.kind);
        }
    }

    #[test]
    fn slash_searches_the_view_and_n_goes_on() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        a.handle_key(ctrl('g'), now);
        a.layout.view = Rect::new(1, 1, 80, 10);
        view_text(&mut a, 100);
        a.handle_key(key(KeyCode::Char('/')), now);
        for c in "LINE 4".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        a.handle_key(key(KeyCode::Enter), now);
        let v = a.view.as_ref().unwrap();
        assert_eq!(
            (v.scroll, v.searching),
            (4, false),
            "case-insensitive, from the top"
        );
        a.handle_key(key(KeyCode::Char('n')), now);
        assert_eq!(a.view.as_ref().unwrap().scroll, 40, "line 40 is the next");
        a.handle_key(key(KeyCode::Char('N')), now);
        assert_eq!(a.view.as_ref().unwrap().scroll, 4);
        a.handle_key(key(KeyCode::Char('q')), now);
        assert!(a.view.is_none(), "q also leaves");
    }

    #[test]
    fn an_empty_view_says_why_and_old_results_are_dropped() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["src/a.c"]);
        a.handle_key(ctrl('g'), now);
        let stale = a.view_gen;
        a.handle_key(key(KeyCode::Esc), now);
        a.handle_key(ctrl('g'), now);
        a.on_view(PreviewMsg {
            generation: stale,
            text: Text::from("old"),
            found: None,
        });
        assert!(
            a.view.as_ref().unwrap().loading,
            "an older view's text is dropped"
        );
        a.on_view(PreviewMsg {
            generation: a.view_gen,
            text: Text::default(),
            found: None,
        });
        let v = a.view.as_ref().unwrap();
        let shown: String = v.text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(shown.contains("No local changes") && !v.loading, "{shown}");
    }

    pub fn rows(kind: ParseKind, raws: &[&str]) -> Vec<Item> {
        raws.iter()
            .map(|r| items::parse(kind, r.as_bytes(), 1).unwrap())
            .collect()
    }

    pub fn feed(a: &mut App, names: &[&str]) -> Vec<Effect> {
        let kind = match a.tab {
            Tab::Folders => ParseKind::Folders,
            Tab::Content => ParseKind::SearchFiles { archives: false },
            Tab::Deep => ParseKind::SearchFiles { archives: true },
            _ => ParseKind::Files,
        };
        a.on_list(ListMsg::Items(a.list_gen, rows(kind, names)))
    }

    pub fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    pub fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    pub fn ctrl_alt(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL | KeyModifiers::ALT)
    }

    pub fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    fn typed(a: &mut App, s: &str, now: Instant) -> Vec<Effect> {
        s.chars()
            .flat_map(|c| a.handle_key(key(KeyCode::Char(c)), now))
            .collect()
    }

    fn current_key(a: &App) -> String {
        a.current().unwrap().key.to_string_lossy().into_owned()
    }

    #[test]
    fn tab_and_backtab_cycle() {
        let (mut a, now) = app(Tab::Files);
        assert_eq!(a.handle_key(key(KeyCode::Tab), now), [Effect::RunList]);
        assert_eq!(a.tab, Tab::Content);
        a.handle_key(key(KeyCode::BackTab), now);
        a.handle_key(key(KeyCode::BackTab), now);
        assert_eq!(a.tab, Tab::Help);
        a.handle_key(key(KeyCode::Tab), now);
        assert_eq!(a.tab, Tab::Files);
    }

    #[test]
    fn switching_tab_starts_a_new_list() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        let g0 = a.list_gen;
        a.handle_key(key(KeyCode::Tab), now);
        assert_eq!(a.list_gen, g0 + 1);
        assert_eq!(a.filter.total(), 0);
    }

    #[test]
    fn first_rows_request_a_preview() {
        let (mut a, _) = app_with_preview(Tab::Files);
        let g0 = a.preview_gen;
        assert_eq!(feed(&mut a, &["a", "b"]), [Effect::Preview]);
        assert_eq!(a.preview_gen, g0 + 1);
        assert_eq!(feed(&mut a, &["c"]), []);
    }

    #[test]
    fn typing_in_files_filters_at_once() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["src/main.rs", "lib.rs", "src/x.md"]);
        let effects = typed(&mut a, "rs", now);
        assert!(!effects.contains(&Effect::RunList));
        assert_eq!(a.filter.len(), 2);
        typed(&mut a, " src", now);
        assert_eq!(a.filter.len(), 1);
        assert_eq!(current_key(&a), "src/main.rs");
    }

    #[test]
    fn current_row_change_requests_a_preview() {
        let (mut a, now) = app_with_preview(Tab::Files);
        feed(&mut a, &["a", "b"]);
        // as fzf: the first row at the bottom, Up goes to the next one
        assert_eq!(a.handle_key(key(KeyCode::Up), now), [Effect::Preview]);
        assert_eq!(current_key(&a), "b");
        assert_eq!(a.handle_key(key(KeyCode::Up), now), []);
        assert_eq!(a.handle_key(key(KeyCode::End), now), [Effect::Preview]);
        assert_eq!(current_key(&a), "a", "End: the bottom row, the first");
        a.handle_key(key(KeyCode::Home), now);
        assert_eq!(current_key(&a), "b", "Home: the top row, the last");
        a.handle_key(key(KeyCode::Down), now);
        assert_eq!(current_key(&a), "a");
    }

    #[test]
    fn page_keys_move_by_the_list_height() {
        let (mut a, now) = app(Tab::Files);
        let names: Vec<String> = (0..50).map(|i| format!("f{i:02}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        a.layout.list = Rect::new(1, 10, 40, 10);
        a.handle_key(key(KeyCode::PageUp), now);
        assert_eq!(current_key(&a), "f10");
        a.handle_key(key(KeyCode::PageDown), now);
        assert_eq!(current_key(&a), "f00");
    }

    #[test]
    fn typing_in_search_waits_150ms() {
        let (mut a, now) = app(Tab::Content);
        assert!(!typed(&mut a, "nee", now).contains(&Effect::RunList));
        assert_eq!(a.tick(now + Duration::from_millis(100)), []);
        typed(&mut a, "d", now + Duration::from_millis(100));
        assert_eq!(a.tick(now + Duration::from_millis(200)), []);
        assert_eq!(a.tick(now + Duration::from_millis(260)), [Effect::RunList]);
        assert_eq!(a.tick(now + Duration::from_millis(400)), []);
        assert_eq!(
            a.list_plan(),
            commands::list_plan(Tab::Content, &a.options, "need", &a.engines)
        );
    }

    #[test]
    fn search_rows_are_not_filtered_by_the_pattern() {
        let (mut a, now) = app(Tab::Content);
        typed(&mut a, "zzz", now);
        a.tick(now + Duration::from_secs(1));
        feed(&mut a, &["a.txt"]);
        assert_eq!(a.filter.len(), 1);
    }

    #[test]
    fn query_is_shared_by_the_tabs() {
        let (mut a, now) = app(Tab::Content);
        typed(&mut a, "x", now);
        a.handle_key(key(KeyCode::BackTab), now);
        assert_eq!(a.tab, Tab::Files);
        feed(&mut a, &["x1", "y"]);
        assert_eq!(a.filter.len(), 1);
    }

    #[test]
    fn word_and_line_deletion() {
        let (mut a, now) = app(Tab::Files);
        typed(&mut a, "foo bar  ", now);
        a.handle_key(ctrl('w'), now);
        assert_eq!(a.query, "foo ");
        a.handle_key(alt(KeyCode::Backspace), now);
        assert_eq!(a.query, "");
        typed(&mut a, "ab cd", now);
        a.handle_key(ctrl('h'), now);
        assert_eq!(a.query, "ab ");
        a.handle_key(key(KeyCode::Backspace), now);
        assert_eq!(a.query, "ab");
        a.handle_key(
            KeyEvent::new(
                KeyCode::Char('h'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            now,
        );
        assert_eq!(a.query, "");
    }

    #[test]
    fn shifted_chars_are_typed_and_releases_ignored() {
        let (mut a, now) = app(Tab::Files);
        a.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT), now);
        let mut release = key(KeyCode::Char('b'));
        release.kind = KeyEventKind::Release;
        a.handle_key(release, now);
        assert_eq!(a.query, "A");
    }

    #[test]
    fn toggle_runs_list_and_saves() {
        let (mut a, now) = app(Tab::Content);
        assert_eq!(a.handle_key(ctrl('s'), now), []);
        assert_eq!(a.popup, Some(0));
        a.handle_key(key(KeyCode::Down), now);
        a.handle_key(key(KeyCode::Down), now);
        assert_eq!(a.popup, Some(2));
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::RunList, Effect::SaveConfig]
        );
        assert!(a.options.content.archives);
        assert_eq!(a.query, "", "space in the popup is no typing");
        for _ in 0..3 {
            a.handle_key(key(KeyCode::Up), now);
        }
        assert_eq!(a.popup, Some(10), "the cursor wraps");
        a.handle_key(key(KeyCode::Enter), now);
        assert!(a.options.content.match_case, "Match case, the last row");
    }

    #[test]
    fn popup_takes_no_other_keys() {
        let (mut a, now) = app(Tab::Files);
        a.handle_key(ctrl('s'), now);
        assert_eq!(a.handle_key(key(KeyCode::Char('x')), now), []);
        assert_eq!(a.handle_key(key(KeyCode::Tab), now), []);
        assert_eq!((a.query.as_str(), a.tab), ("", Tab::Files));
        a.handle_key(ctrl('s'), now);
        assert_eq!(a.popup, None);
    }

    #[test]
    fn config_error_stops_saving() {
        let now = Instant::now();
        let mut a = App::new(
            "/r".into(),
            Tab::Files,
            Options::default(),
            engines(),
            Some("bad file".into()),
            now,
        );
        a.start();
        assert_eq!(
            a.status.as_ref().map(|s| (s.0.as_str(), s.1)),
            Some(("bad file", true))
        );
        a.handle_key(ctrl('s'), now);
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::RunList]
        );
        assert!(a.status.as_ref().unwrap().0.contains("not saved"));
    }

    #[test]
    fn esc_closes_popup_before_quitting() {
        let (mut a, now) = app(Tab::Files);
        a.handle_key(ctrl('s'), now);
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), []);
        assert_eq!(a.popup, None);
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::Quit]);
    }

    #[test]
    fn enter_in_folders_changes_root() {
        let (mut a, now) = app(Tab::Folders);
        a.on_list(ListMsg::Items(a.list_gen, vec![items::parent()]));
        feed(&mut a, &["./src/deep dir/"]);
        assert_eq!(
            a.handle_key(key(KeyCode::Enter), now),
            [Effect::ChangeRoot("/r".into())]
        );
        a.handle_key(key(KeyCode::Up), now);
        assert_eq!(
            a.handle_key(key(KeyCode::Enter), now),
            [Effect::ChangeRoot("/r/p/src/deep dir".into())]
        );
    }

    #[test]
    fn set_root_clears_query_and_reruns() {
        let (mut a, now) = app(Tab::Folders);
        typed(&mut a, "src", now);
        a.preview = Text::raw("the old folder");
        assert_eq!(a.set_root("/r/p/src".into()), [Effect::RunList]);
        assert_eq!(a.preview, Text::default());
        assert_eq!(
            (a.root.to_str(), a.query.as_str(), a.tab),
            (Some("/r/p/src"), "", Tab::Folders)
        );
    }

    #[test]
    fn enter_elsewhere_opens_the_file() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        assert_eq!(
            a.handle_key(key(KeyCode::Enter), now),
            [Effect::Act(ActionKind::Open)]
        );
    }

    #[test]
    fn action_keys() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        // Ctrl-U, Ctrl-G, Alt-G and Alt-U open views (git_keys_open_views_of_the_selected_file),
        // Ctrl-T searches inside the file (ctrl_t_searches_inside_the_file_with_any_of_the_words)
        let cases = [
            (ctrl('o'), ActionKind::Editor),
            (ctrl('e'), ActionKind::Reveal),
            (ctrl('c'), ActionKind::Copy),
        ];
        for (k, kind) in cases {
            assert_eq!(a.handle_key(k, now), [Effect::Act(kind)], "{k:?}");
        }
    }

    #[test]
    fn action_without_item_says_nothing_selected() {
        let (mut a, now) = app(Tab::Files);
        assert_eq!(a.handle_key(ctrl('o'), now), []);
        assert_eq!(
            a.status.as_ref().map(|s| (s.0.as_str(), s.1)),
            Some(("Nothing selected", true))
        );
        let (mut g, now) = app(Tab::Git);
        g.on_list(ListMsg::Items(g.list_gen, rows(ParseKind::Git, &["|\\"])));
        assert_eq!(g.handle_key(key(KeyCode::Enter), now), []);
    }

    #[test]
    fn status_expires() {
        let (mut a, now) = app(Tab::Files);
        a.set_status("Copied", false, now);
        a.tick(now + Duration::from_secs(2));
        assert!(a.status.is_some());
        a.tick(now + Duration::from_secs(4));
        assert!(a.status.is_none());
    }

    #[test]
    fn stale_generation_ignored() {
        let (mut a, now) = app_with_preview(Tab::Files);
        let old = a.list_gen;
        a.handle_key(ctrl('r'), now);
        a.on_list(ListMsg::Items(old, rows(ParseKind::Files, &["stale"])));
        a.on_list(ListMsg::Error(old, "stale error".into()));
        assert_eq!((a.filter.total(), a.notes.len()), (0, 0));
        feed(&mut a, &["a"]);
        let g0 = a.preview_gen;
        a.on_preview(PreviewMsg {
            found: None,
            generation: g0 - 1,
            text: Text::raw("old"),
        });
        assert_eq!(a.preview, Text::default());
        a.on_preview(PreviewMsg {
            found: None,
            generation: g0,
            text: Text::raw("new"),
        });
        assert_eq!(a.preview, Text::raw("new"));
    }

    #[test]
    fn errors_and_hints_are_notes() {
        let (mut a, now) = app(Tab::Files);
        a.on_list(ListMsg::Error(a.list_gen, "fd: boom".into()));
        feed(&mut a, &["a"]);
        typed(&mut a, "zzz", now);
        assert_eq!(a.notes.len(), 1);
        assert_eq!(a.filter.len(), 0);
        a.show_hint("fd is not installed");
        assert!(!a.loading);
        assert_eq!(a.notes.len(), 1);
    }

    #[test]
    fn empty_search_says_no_match() {
        let (mut a, now) = app(Tab::Content);
        typed(&mut a, "zzz", now);
        a.tick(now + Duration::from_secs(1));
        assert!(a.loading);
        a.on_list(ListMsg::Done(a.list_gen));
        assert!(!a.loading);
        let note: String = a.notes[0]
            .display()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(note.starts_with("No match"), "{note}");
    }

    #[test]
    fn no_match_clears_the_old_preview() {
        let (mut a, now) = app(Tab::Content);
        typed(&mut a, "needle", now);
        a.tick(now + Duration::from_secs(1));
        feed(&mut a, &["a.txt"]);
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::raw("a.txt with needle"),
        });
        typed(&mut a, "zzz", now + Duration::from_secs(2));
        a.tick(now + Duration::from_secs(3));
        a.on_list(ListMsg::Done(a.list_gen));
        assert_eq!(a.preview, Text::default());
    }

    #[test]
    fn a_hint_list_clears_the_old_preview() {
        let (mut a, _) = app(Tab::Files);
        feed(&mut a, &["a.txt"]);
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::raw("content"),
        });
        a.handle_key(ctrl('r'), Instant::now());
        a.show_hint("fd is not installed");
        assert_eq!(a.preview, Text::default());
    }

    #[test]
    fn empty_search_without_files_says_so() {
        let (mut a, _) = app(Tab::Content);
        a.on_list(ListMsg::Done(a.list_gen));
        let note: String = a.notes[0]
            .display()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(note, "No text files here");
    }

    #[test]
    fn matching_line_preview_scrolls_to_the_line() {
        let (mut a, now) = app(Tab::Content);
        a.options.content.matching_lines = true;
        typed(&mut a, "x", now);
        a.tick(now + Duration::from_secs(1));
        a.layout.preview = Rect::new(1, 2, 80, 30);
        a.on_list(ListMsg::Items(
            a.list_gen,
            rows(ParseKind::SearchLines, &["a\x0050\x1fx"]),
        ));
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::raw("t"),
        });
        assert_eq!(a.preview_scroll, 39);
    }

    #[test]
    fn the_pattern_and_count_come_with_the_preview_and_go_with_the_tab() {
        let (mut a, now) = app(Tab::Content);
        a.options.content.matching_lines = true;
        typed(&mut a, "x", now);
        a.tick(now + Duration::from_secs(1));
        a.layout.preview = Rect::new(1, 2, 80, 30);
        a.on_list(ListMsg::Items(
            a.list_gen,
            rows(ParseKind::SearchLines, &["a\x0050\x1fx"]),
        ));
        let found = crate::preview::Found {
            pattern: "x".into(),
            count: Some(2),
        };
        a.on_preview(PreviewMsg {
            found: Some(found.clone()),
            generation: a.preview_gen,
            text: Text::raw("t"),
        });
        assert_eq!(
            a.preview_scroll, 39,
            "no header lines above the file any more"
        );
        assert_eq!(a.preview_found, Some(found));
        a.switch_tab(Tab::Files);
        assert_eq!(a.preview_found, None, "another tab's preview has none");
    }

    /// Content with the pattern "needle" searched and two files found.
    fn found(tab: Tab) -> (App, Instant) {
        let (mut a, now) = app(tab);
        typed(&mut a, "needle", now);
        assert_eq!(a.tick(now + Duration::from_secs(1)), [Effect::RunList]);
        feed(&mut a, &["src/a.c", "docs/b.md"]);
        (a, now + Duration::from_secs(1))
    }

    /// The paths `cmd` searches in: what follows the pattern (`.`, or `--` and the files chosen).
    fn searched_in(a: &App) -> Vec<String> {
        let ListPlan::Run { cmd, .. } = a.list_plan() else {
            panic!("{:?}", a.list_plan())
        };
        let args: Vec<String> = cmd
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let e = args.iter().rposition(|a| a == "-e").expect("-e");
        args[e + 2..].to_vec()
    }

    #[test]
    fn ctrl_p_chooses_the_files_to_search_in() {
        let (mut a, now) = found(Tab::Content);
        let file = |p: &str, on| PopupRow::File(p.into(), on);
        assert_eq!(a.handle_key(ctrl('p'), now), []);
        assert_eq!(
            a.popup_rows(),
            [
                PopupRow::AllFiles(true),
                file("src/a.c", true),
                file("docs/b.md", true)
            ],
            "the files listed, all chosen"
        );
        assert_eq!(searched_in(&a), ["."]);
        a.popup = Some(2);
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [],
            "no search: rows leave"
        );
        assert_eq!(a.filter.len(), 1, "only the chosen ones show");
        assert_eq!(a.current().unwrap().key, "src/a.c");
        assert_eq!(a.popup_rows()[0], PopupRow::AllFiles(false));
        assert_eq!(
            a.popup_rows()[2],
            file("docs/b.md", false),
            "still there to choose again"
        );
        assert_eq!(
            searched_in(&a),
            ["--", "src/a.c"],
            "the next search in them only"
        );
        a.handle_key(key(KeyCode::Esc), now);
        a.handle_key(ctrl('w'), now);
        typed(&mut a, "other", now);
        assert_eq!(searched_in(&a), ["--", "src/a.c"], "a new pattern too");
        a.handle_key(ctrl('p'), now);
        assert_eq!(
            a.handle_key(key(KeyCode::Char('a')), now),
            [Effect::RunList],
            "files come back: searched again, their rows were never found"
        );
        assert_eq!(searched_in(&a), ["."], "all files: as before");
        a.handle_key(key(KeyCode::Esc), now);
        feed(&mut a, &["src/a.c", "docs/b.md"]); // that search's rows
        a.popup = None;
        a.handle_key(ctrl('p'), now);
        a.popup = Some(1);
        a.handle_key(key(KeyCode::Char(' ')), now);
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::RunList],
            "chosen again: searched again"
        );
        feed(&mut a, &["src/a.c", "docs/b.md"]);
        a.handle_key(key(KeyCode::Char(' ')), now);
        a.handle_key(key(KeyCode::Esc), now);
        a.switch_tab(Tab::Deep);
        assert_eq!(searched_in(&a), ["."], "Deep's own (Bilal)");
        a.switch_tab(Tab::Content);
        assert_eq!(searched_in(&a), ["--", "docs/b.md"]);
        a.set_root(PathBuf::from("/elsewhere"));
        typed(&mut a, "x", now);
        assert_eq!(searched_in(&a), ["."], "a new root: every file again");
    }

    #[test]
    fn file_types_stand_above_the_files_chosen() {
        // Bilal: .cpp on in Ctrl-S, a few files chosen with Ctrl-P, then All types showed .cpp only
        let (mut a, now) = found(Tab::Content);
        a.handle_key(ctrl('p'), now);
        a.popup = Some(2);
        a.handle_key(key(KeyCode::Char(' ')), now);
        a.handle_key(key(KeyCode::Esc), now);
        assert_eq!(searched_in(&a), ["--", "src/a.c"]);
        a.handle_key(ctrl('s'), now);
        let all_types = a
            .popup_rows()
            .iter()
            .position(|r| matches!(r, PopupRow::AllTypes(_)))
            .unwrap();
        a.popup = Some(all_types);
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::RunList]
        );
        assert!(
            a.chosen_of(Tab::Content).is_none(),
            "a change of types ends the choice"
        );
        assert_eq!(searched_in(&a), ["."], "every file again");
    }

    #[test]
    fn ctrl_p_is_for_content_and_deep() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        a.handle_key(ctrl('p'), now);
        assert!(a.popup.is_none());
    }

    fn version_row(hash: &str) -> crate::versions::VersionRow {
        crate::versions::VersionRow {
            hash: hash.into(),
            date: "2026-10-17".into(),
            author: "T".into(),
            branch: "main".into(),
            subject: "s".into(),
            name: "2026-10-17".into(),
        }
    }

    /// The `-e` pattern and the versions after `--` of a versions list plan.
    fn versions_run(a: &App) -> (String, Vec<String>) {
        let ListPlan::Run { cmd, kind, .. } = a.list_plan() else {
            panic!("{:?}", a.list_plan())
        };
        assert_eq!(kind, ParseKind::Versions);
        assert_eq!(cmd.program, a.self_exe, "sf itself: sf grep-versions");
        let q = crate::versions::parse_args(&cmd.args[1..]).unwrap();
        (q.pattern, q.revs)
    }

    #[test]
    fn ctrl_alt_f_in_git_searches_every_version() {
        let (mut a, now) = app(Tab::Git);
        a.query = "feat".into();
        let fx = a.handle_key(ctrl_alt('f'), now);
        assert!(
            fx.contains(&Effect::LoadVersions) && fx.contains(&Effect::RunList),
            "{fx:?}"
        );
        assert_eq!(
            versions_run(&a),
            (String::new(), vec![]),
            "nothing typed: every file's history"
        );
        typed(&mut a, "needle", now);
        assert_eq!(a.versions.as_ref().unwrap().query, "needle");
        assert_eq!(a.query, "feat", "the Git tab's filter is left alone");
        assert_eq!(a.tick(now + Duration::from_secs(1)), [Effect::RunList]);
        assert_eq!(versions_run(&a), ("needle".into(), vec![]), "every version");
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::RunList]);
        assert!(
            a.versions.is_none() && a.query == "feat",
            "back to the commits"
        );
        a.handle_key(ctrl_alt('f'), now);
        assert!(a.versions.is_some());
        assert_eq!(
            a.handle_key(ctrl_alt('f'), now),
            [Effect::RunList],
            "Ctrl-Alt-F again: back"
        );
        assert!(a.versions.is_none());
    }

    #[test]
    fn the_versions_popup_chooses_options_and_versions() {
        let (mut a, now) = app(Tab::Git);
        a.handle_key(ctrl_alt('f'), now);
        typed(&mut a, "x", now);
        a.on_versions(vec![version_row("aaa"), version_row("bbb")]);
        a.handle_key(ctrl('s'), now);
        let v = |h: &str, on| PopupRow::Version(h.into(), on);
        assert_eq!(
            a.popup_rows(),
            [
                PopupRow::Opt(Opt::WholeWord, false),
                PopupRow::Opt(Opt::Regex, true),
                PopupRow::Opt(Opt::Binary, false),
                PopupRow::Opt(Opt::Documents, false),
                PopupRow::Opt(Opt::SmartCase, false),
                PopupRow::Opt(Opt::MatchCase, false),
                PopupRow::AllVersions(true),
                v("aaa", true),
                v("bbb", true)
            ]
        );
        a.popup = Some(7);
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::RunList]
        );
        assert_eq!(versions_run(&a).1, ["bbb"], "only the versions chosen");
        assert_eq!(a.popup_rows()[6], PopupRow::AllVersions(false));
        a.popup = Some(6);
        a.handle_key(key(KeyCode::Char(' ')), now);
        assert!(versions_run(&a).1.is_empty(), "all of them again");
        a.handle_key(key(KeyCode::Char(' ')), now);
        assert!(matches!(a.list_plan(), ListPlan::Hint(_)), "none chosen");
        a.handle_key(key(KeyCode::Char('a')), now);
        assert!(versions_run(&a).1.is_empty(), "a: all of them");
        a.popup = Some(2);
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::RunList, Effect::SaveConfig]
        );
        assert!(a.options.versions.binary && !a.options.git.match_case);
    }

    #[test]
    fn enter_on_a_versions_row_shows_the_file_as_it_was() {
        let (mut a, now) = app(Tab::Git);
        a.handle_key(ctrl_alt('f'), now);
        let rev = "0123456789abcdef0123456789abcdef01234567";
        a.on_list(ListMsg::Items(
            a.list_gen,
            rows(
                ParseKind::Versions,
                &[&format!("{rev}:src/a.c\x009\x00the x")],
            ),
        ));
        assert_eq!(a.handle_key(key(KeyCode::Enter), now), [Effect::View]);
        let kind = &a.view.as_ref().unwrap().kind;
        let want = ViewKind::Version {
            root: a.root.clone(),
            rev: rev.into(),
            path: "src/a.c".into(),
            line: 9,
        };
        assert_eq!(kind, &want);
        a.handle_key(key(KeyCode::Esc), now);
        assert!(
            a.view.is_none() && a.versions.is_some(),
            "Esc: back to the versions found"
        );
        assert_eq!(a.handle_key(ctrl('g'), now), []);
        assert!(
            a.status.is_none(),
            "a file's keys do nothing here: {:?}",
            a.status
        );
    }

    #[test]
    fn a_files_keys_do_nothing_in_git() {
        // Bilal: they need not be bound there (they said "Not available in Git")
        let (mut a, now) = app(Tab::Git);
        feed(&mut a, &["abcdef1"]);
        for k in [
            ctrl('t'),
            ctrl('u'),
            alt(KeyCode::Char('u')),
            ctrl('g'),
            alt(KeyCode::Char('g')),
            ctrl('o'),
            ctrl('e'),
        ] {
            assert_eq!(a.handle_key(k, now), [], "{k:?}");
            assert!(
                a.status.is_none() && a.view.is_none(),
                "{k:?}: {:?}",
                a.status
            );
        }
        assert_eq!(
            a.handle_key(ctrl('c'), now),
            [Effect::Act(ActionKind::Copy)],
            "copying the hash stays"
        );
    }

    #[test]
    fn the_file_as_it_was_opens_at_the_line_found() {
        let (mut a, now) = app(Tab::Git);
        a.handle_key(ctrl_alt('f'), now);
        let rev = "0123456789abcdef0123456789abcdef01234567";
        a.on_list(ListMsg::Items(
            a.list_gen,
            rows(ParseKind::Versions, &[&format!("{rev}:a.c\x0050\x00x")]),
        ));
        a.layout.view = Rect::new(0, 0, 80, 30);
        a.handle_key(key(KeyCode::Enter), now);
        let text = Text::from(
            (1..=100)
                .map(|i| Line::raw(i.to_string()))
                .collect::<Vec<_>>(),
        );
        a.on_view(PreviewMsg {
            generation: a.view_gen,
            text,
            found: None,
        });
        assert_eq!(
            a.view.as_ref().unwrap().scroll,
            39,
            "line 50 a third down (index 49 - 10)"
        );
    }

    #[test]
    fn ctrl_alt_f_keeps_the_pattern_and_filters_the_rows() {
        let (mut a, now) = found(Tab::Content);
        assert_eq!(a.handle_key(ctrl_alt('f'), now), []);
        assert_eq!(a.result_filter.as_deref(), Some(""));
        typed(&mut a, "SRC", now);
        assert_eq!((a.query.as_str(), a.filter.len()), ("needle", 1));
        assert_eq!(a.current().unwrap().key, "src/a.c");
        assert_eq!(
            a.tick(now + Duration::from_secs(1)),
            [],
            "the search does not run again"
        );
        a.handle_key(key(KeyCode::Backspace), now);
        assert_eq!(a.result_filter.as_deref(), Some("SR"));
        assert_eq!(a.handle_key(ctrl('r'), now), [Effect::RunList]);
        feed(&mut a, &["src/a.c", "docs/b.md"]);
        assert_eq!(a.filter.len(), 1, "a new run keeps the filter");
        a.handle_key(key(KeyCode::Esc), now);
        assert_eq!(a.result_filter, None, "Esc goes back to the pattern");
        assert_eq!((a.query.as_str(), a.filter.len()), ("needle", 2));
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::Quit]);
    }

    #[test]
    fn alt_f_alone_does_nothing() {
        // Bilal's KDE opens rofi on Alt+F: sf leaves the key alone
        let (mut a, now) = found(Tab::Content);
        assert_eq!(a.handle_key(alt(KeyCode::Char('f')), now), []);
        assert!(a.result_filter.is_none() && a.preview_find.is_none());
    }

    #[test]
    fn ctrl_alt_f_again_or_another_tab_ends_the_filter() {
        let (mut a, now) = found(Tab::Deep);
        a.handle_key(ctrl_alt('f'), now);
        a.handle_key(ctrl_alt('f'), now);
        assert_eq!(a.result_filter, None);
        a.handle_key(ctrl_alt('f'), now);
        a.handle_key(key(KeyCode::Tab), now);
        assert_eq!((a.tab, a.result_filter.as_deref()), (Tab::Folders, None));
    }

    #[test]
    fn ctrl_alt_f_is_for_a_pattern_only() {
        let (mut a, now) = app(Tab::Files);
        a.handle_key(ctrl_alt('f'), now);
        assert_eq!(a.result_filter, None);
        let (mut a, now) = found(Tab::Content);
        a.handle_key(ctrl('t'), now);
        a.handle_key(ctrl_alt('f'), now);
        assert_eq!(a.result_filter, None, "not inside a file");
    }

    #[test]
    fn a_pattern_from_the_command_line_starts_the_filter() {
        let (mut a, _) = app(Tab::Deep);
        a.start_query("needle".into());
        assert_eq!(
            (a.query.as_str(), a.result_filter.as_deref()),
            ("needle", Some(""))
        );
        let (mut a, _) = app(Tab::Files);
        a.start_query("main".into());
        assert_eq!(
            (a.query.as_str(), a.result_filter.as_deref()),
            ("main", None)
        );
        let (mut a, _) = app(Tab::Content);
        a.start_query(String::new());
        assert_eq!(a.result_filter, None, "no pattern to keep");
    }

    fn ty(ext: Option<&str>, count: usize, on: bool) -> PopupRow {
        PopupRow::Type(ext.map(str::to_string), count, on)
    }

    #[test]
    fn ctrl_s_lists_the_file_types_of_the_rows() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a.rs", "b.rs", "c.dll", "Makefile", "d.md"]);
        a.handle_key(ctrl('s'), now);
        assert_eq!(
            a.popup_rows(),
            [
                PopupRow::Opt(Opt::Hidden, false),
                PopupRow::Opt(Opt::Ignored, true),
                PopupRow::Opt(Opt::MatchCase, false),
                PopupRow::AllTypes(true),
                ty(Some("rs"), 2, true),
                ty(Some("dll"), 1, true),
                ty(Some("md"), 1, true),
                ty(None, 1, true),
            ],
            "the most first; no extension last among equals"
        );
        let (a, _) = app(Tab::Folders);
        assert_eq!(a.popup_rows().len(), 3, "Folders: its options only");
    }

    #[test]
    fn a_file_type_turned_off_leaves_the_list_and_stays_in_the_popup() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a.rs", "b.rs", "c.dll", "Makefile"]);
        a.handle_key(ctrl('s'), now);
        for _ in 0..5 {
            a.handle_key(key(KeyCode::Down), now);
        }
        assert_eq!(a.popup_rows()[5], ty(Some("dll"), 1, true));
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [],
            "Files: no new run, nothing saved"
        );
        assert_eq!(a.filter.len(), 3);
        assert_eq!(a.popup_rows()[5], ty(Some("dll"), 1, false));
        assert_eq!(a.popup_rows()[3], PopupRow::AllTypes(false));
        a.handle_key(key(KeyCode::Char('a')), now);
        assert_eq!(a.filter.len(), 4, "a: all on");
        a.handle_key(key(KeyCode::Char('a')), now);
        assert_eq!(a.filter.len(), 0, "a again: all off");
        a.popup = Some(3);
        a.handle_key(key(KeyCode::Enter), now);
        assert_eq!(a.filter.len(), 4, "All types toggles them too");
    }

    #[test]
    fn in_content_and_deep_a_type_turned_off_is_not_searched() {
        for tab in [Tab::Content, Tab::Deep] {
            let (mut a, now) = found(tab); // src/a.c, docs/b.md
            let at = a
                .popup_rows()
                .iter()
                .position(|r| *r == ty(Some("md"), 1, true))
                .unwrap();
            a.handle_key(ctrl('s'), now);
            a.popup = Some(at);
            assert_eq!(
                a.handle_key(key(KeyCode::Char(' ')), now),
                [Effect::RunList],
                "{tab:?}"
            );
            let ListPlan::Run { cmd, .. } = a.list_plan() else {
                panic!("{tab:?}")
            };
            assert!(
                cmd.args.contains(&"--exclude=*.md".into()),
                "{tab:?}: {:?}",
                cmd.args
            );
            feed(&mut a, &["src/a.c"]);
            assert!(
                a.popup_rows().contains(&ty(Some("md"), 1, false)),
                "kept with its count"
            );
        }
    }

    #[test]
    fn each_tab_has_its_own_types_and_a_new_root_turns_them_on() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a.rs", "c.dll"]);
        a.handle_key(ctrl('s'), now);
        a.popup = Some(4); // .dll: one each, so by name
        assert_eq!(a.popup_rows()[4], ty(Some("dll"), 1, true));
        a.handle_key(key(KeyCode::Char(' ')), now);
        a.handle_key(key(KeyCode::Esc), now);
        a.handle_key(key(KeyCode::Tab), now);
        feed(&mut a, &["a.rs", "c.dll"]);
        assert_eq!(a.filter.len(), 2, "Content has its own");
        a.handle_key(key(KeyCode::BackTab), now);
        feed(&mut a, &["a.rs", "c.dll"]);
        assert_eq!(a.filter.len(), 1, "Files keeps its own");
        a.set_root(PathBuf::from("/elsewhere"));
        feed(&mut a, &["a.rs", "c.dll"]);
        assert_eq!(a.filter.len(), 2, "another folder: all on again");
    }

    #[test]
    fn deep_previews_once_the_cursor_rests() {
        let ms = Duration::from_millis;
        let (mut a, t0) = app(Tab::Deep);
        assert_eq!(feed(&mut a, &["a.dll", "b.dll"]), [], "not at once");
        assert_eq!(a.tick(t0), []);
        assert_eq!(a.tick(t0 + ms(60)), []);
        assert_eq!(
            a.handle_key(key(KeyCode::Up), t0 + ms(70)),
            [],
            "another row: the wait starts again"
        );
        assert_eq!(a.tick(t0 + ms(80)), []);
        assert_eq!(a.tick(t0 + ms(150)), []);
        assert_eq!(a.tick(t0 + ms(190)), [Effect::Preview]);
        assert_eq!(a.tick(t0 + ms(400)), [], "once");
    }

    #[test]
    fn a_slow_preview_says_it_is_searching() {
        let ms = Duration::from_millis;
        let shown = |a: &App| -> String {
            a.preview
                .lines
                .iter()
                .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
                .collect()
        };
        let (mut a, t0) = app_with_preview(Tab::Content);
        a.preview = Text::raw("the previous file");
        assert_eq!(feed(&mut a, &["src/a.c"]), [Effect::Preview]);
        a.tick(t0);
        a.tick(t0 + ms(200));
        assert_eq!(
            shown(&a),
            "the previous file",
            "a quick preview just takes its place"
        );
        a.tick(t0 + ms(300));
        assert_eq!(shown(&a), "searching inside src/a.c…");
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::raw("a.c's matches"),
        });
        a.tick(t0 + ms(600));
        assert_eq!(shown(&a), "a.c's matches");
    }

    #[test]
    fn an_empty_list_puts_up_no_searching_note() {
        let ms = Duration::from_millis;
        let (mut a, t0) = app_with_preview(Tab::Content);
        feed(&mut a, &["src/a.c"]);
        a.handle_key(ctrl('r'), t0);
        a.on_list(ListMsg::Done(a.list_gen));
        a.tick(t0);
        a.tick(t0 + ms(400));
        assert_eq!(a.preview, Text::default());
    }

    #[test]
    fn files_starts_without_a_preview_the_others_with_one() {
        let (mut a, now) = app(Tab::Files);
        assert!(!a.preview_visible());
        a.handle_key(key(KeyCode::Tab), now);
        assert_eq!(a.tab, Tab::Content);
        assert!(a.preview_visible());
        a.handle_key(ctrl('l'), now);
        assert!(!a.preview_visible(), "Ctrl-L hides Content's");
        a.handle_key(key(KeyCode::BackTab), now);
        a.handle_key(ctrl('l'), now);
        assert!(a.preview_visible(), "Ctrl-L shows Files'");
        a.handle_key(key(KeyCode::Tab), now);
        assert!(!a.preview_visible(), "each tab keeps its own");
    }

    #[test]
    fn preview_scroll_and_toggle() {
        let (mut a, now) = app(Tab::Content);
        feed(&mut a, &["a"]);
        a.layout.preview = Rect::new(1, 2, 80, 10);
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::raw("x\n".repeat(100)),
        });
        a.handle_key(alt(KeyCode::Down), now);
        assert_eq!(a.preview_scroll, 10);
        a.handle_key(alt(KeyCode::Up), now);
        a.handle_key(alt(KeyCode::Up), now);
        assert_eq!(a.preview_scroll, 0);
        assert_eq!(a.handle_key(ctrl('l'), now), []);
        assert!(!a.preview_visible());
        assert_eq!(a.handle_key(ctrl('l'), now), [Effect::Preview]);
    }

    fn drawn(a: &mut App) {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| crate::ui::draw(f, a)).unwrap();
    }

    fn mouse(kind: MouseEventKind, r: Rect) -> MouseEvent {
        MouseEvent {
            kind,
            column: r.x + r.width / 2,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn click(r: Rect) -> MouseEvent {
        mouse(MouseEventKind::Down(MouseButton::Left), r)
    }

    /// The `i`-th row of the list counted from the bottom (the first row is at the bottom, as in fzf).
    fn list_row(a: &App, i: u16) -> Rect {
        Rect::new(
            a.layout.list.x,
            a.layout.list.bottom() - 1 - i,
            a.layout.list.width,
            1,
        )
    }

    #[test]
    fn click_tab_switches() {
        let (mut a, now) = app(Tab::Files);
        drawn(&mut a);
        let search = a.layout.tabs[1].1;
        assert_eq!(a.handle_mouse(click(search), now), [Effect::RunList]);
        assert_eq!(a.tab, Tab::Content);
        drawn(&mut a);
        assert_eq!(
            a.handle_mouse(click(a.layout.tabs[1].1), now),
            [],
            "the current tab again: nothing"
        );
    }

    #[test]
    fn click_config_opens_popup() {
        let (mut a, now) = app(Tab::Files);
        drawn(&mut a);
        a.handle_mouse(click(a.layout.config), now);
        assert_eq!(a.popup, Some(0));
        drawn(&mut a);
        a.handle_mouse(click(a.layout.config), now);
        assert_eq!(a.popup, None);
    }

    #[test]
    fn click_row_selects() {
        let (mut a, now) = app_with_preview(Tab::Files);
        feed(&mut a, &["a", "b", "c"]);
        drawn(&mut a);
        assert_eq!(
            a.handle_mouse(click(list_row(&a, 2)), now),
            [Effect::Preview]
        );
        assert_eq!(current_key(&a), "c");
        assert_eq!(
            a.handle_mouse(click(list_row(&a, 9)), now + Duration::from_secs(1)),
            []
        );
        assert_eq!(
            current_key(&a),
            "c",
            "a click below the rows changes nothing"
        );
    }

    #[test]
    fn click_on_a_note_selects_nothing() {
        let (mut a, now) = app(Tab::Files);
        a.on_list(ListMsg::Error(a.list_gen, "boom".into()));
        feed(&mut a, &["a", "b"]);
        drawn(&mut a);
        a.handle_mouse(click(list_row(&a, 0)), now);
        assert_eq!(current_key(&a), "a");
        a.handle_mouse(click(list_row(&a, 2)), now + Duration::from_secs(1));
        assert_eq!(current_key(&a), "b");
    }

    #[test]
    fn double_click_acts() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a", "b"]);
        drawn(&mut a);
        a.handle_mouse(click(list_row(&a, 1)), now);
        assert_eq!(
            a.handle_mouse(click(list_row(&a, 1)), now + Duration::from_millis(300)),
            [Effect::Act(ActionKind::Open)]
        );
        assert_eq!(
            a.handle_mouse(click(list_row(&a, 1)), now + Duration::from_millis(1000)),
            []
        );
        a.handle_mouse(click(list_row(&a, 0)), now + Duration::from_millis(2000));
        assert_ne!(
            a.handle_mouse(click(list_row(&a, 1)), now + Duration::from_millis(2100)),
            [Effect::Act(ActionKind::Open)],
            "two clicks on two rows are no double click"
        );
    }

    #[test]
    fn match_case_filters_by_the_case_typed() {
        let (mut a, now) = app(Tab::Files);
        a.popup = Some(2);
        assert_eq!(a.popup_rows()[2], PopupRow::Opt(Opt::MatchCase, false));
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now)[0],
            Effect::RunList
        );
        a.handle_key(key(KeyCode::Esc), now);
        feed(&mut a, &["README.md", "readme.txt"]);
        typed(&mut a, "README", now);
        assert_eq!(a.filter.len(), 1);
        assert_eq!(a.current().unwrap().key, "README.md");
    }

    #[test]
    fn the_help_tab_scrolls_and_finds() {
        let (mut a, now) = app(Tab::Git);
        assert_eq!(a.handle_key(key(KeyCode::Tab), now), [], "no list to run");
        assert_eq!(a.tab, Tab::Help);
        a.layout.help = Rect::new(1, 2, 80, 10);
        a.handle_key(key(KeyCode::Down), now);
        assert_eq!(a.help.scroll, 1);
        let lines_with = |a: &App, needle: &str| -> Vec<usize> {
            (0..a.help.text.lines.len())
                .filter(|&i| {
                    let line: String = a.help.text.lines[i]
                        .spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect();
                    line.to_lowercase().contains(needle)
                })
                .collect()
        };
        let matches = lines_with(&a, "match");
        assert!(matches.len() > 2);
        typed(&mut a, "MATCH", now);
        assert_eq!(a.query, "", "the manual's search is its own");
        assert_eq!(a.help.found, Some(matches[0]));
        a.handle_key(key(KeyCode::Enter), now);
        assert_eq!(a.help.found, Some(matches[1]), "Enter: the next one");
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), []);
        assert_eq!((a.help.search.as_str(), a.help.found), ("", None));
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::Quit]);
        assert_eq!(a.handle_key(key(KeyCode::Tab), now), [Effect::RunList]);
        assert_eq!(a.tab, Tab::Files);
    }

    #[test]
    fn the_wheel_scrolls_the_manual() {
        let (mut a, now) = app(Tab::Help);
        a.layout.help = Rect::new(1, 2, 80, 10);
        a.handle_mouse(mouse(MouseEventKind::ScrollDown, a.layout.help), now);
        assert_eq!(a.help.scroll, WHEEL);
        a.handle_mouse(mouse(MouseEventKind::ScrollUp, a.layout.help), now);
        assert_eq!(a.help.scroll, 0);
    }

    #[test]
    fn in_tmux_ctrl_t_is_a_window_of_its_own() {
        for tab in [Tab::Files, Tab::Content, Tab::Deep] {
            let (mut a, now) = app(tab);
            a.tmux = true;
            feed(&mut a, &["src/a.c"]);
            assert_eq!(
                a.handle_key(ctrl('t'), now),
                [Effect::Act(ActionKind::InFile)],
                "{tab:?}"
            );
            assert!(a.scope.is_none());
        }
        let (mut a, now) = found(Tab::Content);
        assert_eq!(a.in_file_query(), "needle", "Content passes its pattern");
        a.handle_key(key(KeyCode::BackTab), now);
        assert_eq!(a.in_file_query(), "", "a Files filter is no pattern");
    }

    #[test]
    fn sf_in_a_file_starts_inside_it_and_esc_quits() {
        let (mut a, now) = app(Tab::Files);
        a.start_in_file("src/a b.c".into(), "ankara izmir".into());
        assert_eq!((a.tab, a.query.as_str()), (Tab::Content, "ankara izmir"));
        let ListPlan::Run { cmd, kind, .. } = a.list_plan() else {
            panic!()
        };
        assert_eq!(kind, ParseKind::SearchLines);
        assert_eq!(cmd.args.last(), Some(&OsString::from("src/a b.c")));
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::Quit]);
    }

    #[test]
    fn ctrl_s_in_a_file_holds_its_own_options() {
        let (mut a, now) = found(Tab::Content);
        a.handle_key(ctrl('t'), now);
        a.handle_key(ctrl('s'), now);
        assert_eq!(a.popup, Some(0));
        assert_eq!(
            a.popup_rows(),
            [
                PopupRow::Opt(Opt::WordsOnALine, true),
                PopupRow::Opt(Opt::WordsInTheFile, false),
                PopupRow::Opt(Opt::SmartCase, false),
                PopupRow::Opt(Opt::MatchCase, false)
            ]
        );
        a.handle_key(key(KeyCode::Down), now);
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::RunList, Effect::SaveConfig]
        );
        assert!(a.options.in_file.words_in_the_file && !a.options.in_file.words_on_a_line);
        assert!(
            !a.options.content.words_in_the_file,
            "Content's stay as they are"
        );
        let ListPlan::Run { cmd, .. } = a.list_plan() else {
            panic!()
        };
        assert!(cmd.args.contains(&"--files".into()), "{:?}", cmd.args);
    }

    #[test]
    fn the_editor_follows_the_case_options() {
        // vim: \c case aside, \C the case counts; none: actions' smart case (\c for lower case)
        let (mut a, _) = app(Tab::Content);
        a.query = "Needle".into();
        assert_eq!(a.editor_query(), "\\cNeedle", "case aside");
        a.options.toggle(Tab::Content, Opt::SmartCase);
        assert_eq!(a.editor_query(), "Needle", "smart case");
        a.options.toggle(Tab::Content, Opt::MatchCase);
        assert_eq!(a.editor_query(), "\\CNeedle", "match case");
        a.query.clear();
        assert_eq!(a.editor_query(), "", "nothing to look for");
    }

    #[test]
    fn the_editor_highlights_any_of_the_words() {
        let (mut a, _) = app(Tab::Content);
        a.query = "ankara -izmir \"x y\"".into();
        assert_eq!(a.editor_pattern(), a.query, "a pattern goes as it is");
        a.options.toggle(Tab::Content, Opt::WordsOnALine);
        assert_eq!(
            a.editor_pattern(),
            "ankara|x y",
            "words: any of them, not the excluded one"
        );
    }

    #[test]
    fn the_editor_finds_whole_words_when_the_tab_does() {
        let (mut a, _) = app(Tab::Deep);
        a.query = "config".into();
        a.options.toggle(Tab::Deep, Opt::WholeWord);
        assert_eq!(
            a.editor_pattern(),
            "<(config)>",
            "vim's very magic word bounds"
        );
        a.options.toggle(Tab::Deep, Opt::WordsOnALine);
        a.query = "ankara izmir".into();
        assert_eq!(a.editor_pattern(), "<(ankara|izmir)>");
        a.switch_tab(Tab::Content);
        assert_eq!(a.editor_pattern(), "ankara izmir", "Content's own options");
    }

    #[test]
    fn the_editor_finds_the_text_as_typed_without_regular_expression() {
        let (mut a, _) = app(Tab::Content);
        a.options.toggle(Tab::Content, Opt::Regex);
        a.query = r"a.b\x".into();
        assert_eq!(
            a.editor_pattern(),
            r"\Va.b\\x\v",
            "vim's very nomagic, then back"
        );
        a.options.toggle(Tab::Content, Opt::WholeWord);
        assert_eq!(a.editor_pattern(), r"<(\Va.b\\x\v)>");
        a.options.toggle(Tab::Content, Opt::WholeWord);
        a.options.toggle(Tab::Content, Opt::WordsOnALine);
        a.query = "a.b c(".into();
        assert_eq!(
            a.editor_pattern(),
            r"\Va.b\v|\Vc(\v",
            "any of the words, each as typed"
        );
    }

    #[test]
    fn the_editor_gets_any_of_the_words_when_matches_are_listed() {
        let (mut a, _) = app(Tab::Content);
        a.options.toggle(Tab::Content, Opt::MatchingLines);
        a.query = r#"CODE "of x""#.into();
        assert_eq!(a.editor_pattern(), "CODE|of x");
        assert_eq!(
            a.row_words().map(|w| w.words),
            Some(vec!["CODE".to_string(), "of x".to_string()])
        );
        a.options.toggle(Tab::Content, Opt::MatchingLines);
        assert_eq!(a.row_words(), None, "listing files: a pattern");
    }

    #[test]
    fn ctrl_t_searches_inside_the_file_for_the_words() {
        let (mut a, now) = app(Tab::Files);
        a.query = "a.c".into();
        feed(&mut a, &["src/a.c"]);
        assert_eq!(a.handle_key(ctrl('t'), now), [Effect::RunList]);
        assert_eq!(
            (a.tab, a.query.as_str()),
            (Tab::Content, ""),
            "a Files filter is no pattern"
        );
        let ListPlan::Run { cmd, kind, .. } = a.list_plan() else {
            panic!()
        };
        assert_eq!(kind, ParseKind::SearchLines);
        assert_eq!(cmd.args.last(), Some(&OsString::from("src/a.c")));
        for c in "foo  bar".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        // all of them on a line (Bilal: as the fzf window); turned off, any of them
        assert_eq!(a.pattern(), "foo  bar");
        let ListPlan::Run { cmd, .. } = a.list_plan() else {
            panic!()
        };
        assert!(cmd.args.contains(&"--bool".into()), "{:?}", cmd.args);
        a.options.toggle_in_file(Opt::WordsOnALine);
        assert_eq!(a.pattern(), "foo|bar");
        let ListPlan::Run { cmd, .. } = a.list_plan() else {
            panic!()
        };
        assert!(cmd.args.contains(&"foo|bar".into()), "{:?}", cmd.args);
        // Esc goes back to the tab and its filter; the next one quits
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::RunList]);
        assert_eq!(
            (a.tab, a.query.as_str(), a.scope.is_none()),
            (Tab::Files, "a.c", true)
        );
        assert_eq!(a.handle_key(key(KeyCode::Esc), now), [Effect::Quit]);
    }

    #[test]
    fn deep_is_searched_as_content_is() {
        let (mut a, now) = app(Tab::Deep);
        assert!(!typed(&mut a, "need", now).contains(&Effect::RunList));
        assert_eq!(a.tick(now + Duration::from_millis(260)), [Effect::RunList]);
        assert_eq!(
            a.list_plan(),
            commands::list_plan(Tab::Deep, &a.options, "need", &a.engines)
        );
        feed(&mut a, &["a.dll"]);
        assert_eq!(
            a.filter.len(),
            1,
            "the rows are not filtered by the pattern"
        );
    }

    #[test]
    fn deep_without_a_match_points_to_documents() {
        let note = |a: &App| -> String {
            a.notes[0]
                .display()
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect()
        };
        let (mut a, now) = app(Tab::Deep);
        typed(&mut a, "zzz", now);
        a.tick(now + Duration::from_secs(1));
        a.on_list(ListMsg::Done(a.list_gen));
        assert!(
            note(&a).starts_with("No match. Documents (⚙ Config)"),
            "{}",
            note(&a)
        );
        a.options.deep.documents = true;
        a.handle_key(ctrl('r'), now);
        a.on_list(ListMsg::Done(a.list_gen));
        assert_eq!(note(&a), "No match");
    }

    #[test]
    fn ctrl_t_in_deep_keeps_the_pattern_and_goes_back_to_deep() {
        let (mut a, now) = app(Tab::Deep);
        a.query = "needle".into();
        feed(&mut a, &["src/a.c"]);
        a.handle_key(ctrl('t'), now);
        assert!(a.scope.is_some());
        assert_eq!(a.query, "needle");
        a.handle_key(key(KeyCode::Esc), now);
        assert_eq!((a.tab, a.scope.is_none()), (Tab::Deep, true));
    }

    #[test]
    fn ctrl_t_in_content_keeps_the_pattern() {
        let (mut a, now) = app(Tab::Content);
        a.query = "needle".into();
        feed(&mut a, &["src/a.c"]);
        a.handle_key(ctrl('t'), now);
        assert_eq!(a.query, "needle");
        assert_eq!(a.handle_key(ctrl('t'), now), [], "already in it");
        assert_eq!(
            a.handle_key(key(KeyCode::Tab), now),
            [Effect::RunList],
            "Tab leaves too"
        );
        assert_eq!(a.tab, Tab::Content);
        assert!(a.scope.is_none());
    }

    #[test]
    fn the_wheel_moves_through_an_open_popup() {
        // Bilal: in Ctrl-P's list of files the wheel did nothing
        let (mut a, now) = app(Tab::Content);
        let names: Vec<String> = (0..20).map(|i| format!("f{i}.txt")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        feed(&mut a, &names);
        a.handle_key(ctrl('p'), now);
        a.layout.popup = Rect::new(60, 2, 30, 10);
        let over = |kind| mouse(kind, Rect::new(70, 5, 1, 1));
        a.handle_mouse(over(MouseEventKind::ScrollDown), now);
        assert_eq!(a.popup, Some(WHEEL));
        a.handle_mouse(over(MouseEventKind::ScrollUp), now);
        a.handle_mouse(over(MouseEventKind::ScrollUp), now);
        assert_eq!(a.popup, Some(0), "not past the first row");
        for _ in 0..20 {
            a.handle_mouse(over(MouseEventKind::ScrollDown), now);
        }
        assert_eq!(
            a.popup,
            Some(20),
            "nor past the last (All files and 20 files)"
        );
        assert!(a.popup_files, "the popup stays open");
    }

    #[test]
    fn a_scroll_bar_dragged_or_clicked_scrolls_there() {
        let (mut a, now) = app_with_preview(Tab::Content);
        let names: Vec<String> = (0..50).map(|i| format!("f{i}.txt")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        feed(&mut a, &names);
        a.layout.list = Rect::new(1, 20, 80, 10);
        a.layout.list_bar = Rect::new(81, 20, 1, 10);
        a.layout.preview = Rect::new(1, 2, 80, 10);
        a.layout.preview_bar = Rect::new(81, 2, 1, 10);
        let lines: Vec<Line> = (0..100).map(|i| Line::raw(i.to_string())).collect();
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::from(lines),
        });
        let at = |kind, r: Rect, dy: u16| MouseEvent {
            kind,
            column: r.x,
            row: r.y + dy,
            modifiers: KeyModifiers::NONE,
        };
        let (down, drag) = (
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
        );
        a.handle_mouse(at(down, a.layout.preview_bar, 9), now);
        assert_eq!(a.preview_scroll, 90, "the bottom: the last page");
        a.handle_mouse(at(drag, a.layout.preview_bar, 3), now);
        assert_eq!(a.preview_scroll, 30, "dragged a third of the way");
        a.handle_mouse(at(down, a.layout.list_bar, 0), now);
        assert_eq!(
            a.selected, 49,
            "the top: the last row (the list runs upwards)"
        );
        a.handle_mouse(at(drag, a.layout.list_bar, 9), now);
        assert_eq!(a.selected, 0, "the bottom: the first row");
    }

    #[test]
    fn wheel_scrolls_an_open_view() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        a.handle_key(ctrl('g'), now);
        a.layout.view = Rect::new(1, 1, 80, 10);
        view_text(&mut a, 100);
        a.handle_mouse(mouse(MouseEventKind::ScrollDown, a.layout.view), now);
        assert_eq!(a.view.as_ref().unwrap().scroll, 3);
        a.handle_mouse(mouse(MouseEventKind::ScrollUp, a.layout.view), now);
        assert_eq!(a.view.as_ref().unwrap().scroll, 0);
    }

    #[test]
    fn wheel_over_list_moves() {
        let (mut a, now) = app(Tab::Files);
        let names: Vec<String> = (0..10).map(|i| format!("f{i}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        drawn(&mut a);
        a.handle_mouse(mouse(MouseEventKind::ScrollUp, a.layout.list), now);
        assert_eq!(
            current_key(&a),
            "f3",
            "the wheel up goes up the screen, to later rows"
        );
        a.handle_mouse(mouse(MouseEventKind::ScrollDown, a.layout.list), now);
        assert_eq!(current_key(&a), "f0");
    }

    #[test]
    fn wheel_over_preview_scrolls() {
        let (mut a, now) = app_with_preview(Tab::Files);
        feed(&mut a, &["a"]);
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::raw("x\n".repeat(100)),
        });
        drawn(&mut a);
        a.handle_mouse(mouse(MouseEventKind::ScrollDown, a.layout.preview), now);
        assert_eq!(a.preview_scroll, 3);
        assert_eq!(current_key(&a), "a");
    }

    #[test]
    fn click_popup_row_toggles() {
        let (mut a, now) = app(Tab::Content);
        a.handle_key(ctrl('s'), now);
        drawn(&mut a);
        let row = a.layout.popup_rows[2].1; // Matching lines, Each match, Archives & documents
        assert_eq!(
            a.handle_mouse(click(row), now),
            [Effect::RunList, Effect::SaveConfig]
        );
        assert!(a.options.content.archives);
        assert_eq!(a.popup, Some(2));
    }

    #[test]
    fn click_outside_popup_closes() {
        let (mut a, now) = app(Tab::Content);
        feed(&mut a, &["a"]);
        a.handle_key(ctrl('s'), now);
        drawn(&mut a);
        assert_eq!(a.handle_mouse(click(Rect::new(0, 29, 1, 1)), now), []);
        assert_eq!(a.popup, None);
        assert_eq!(a.tab, Tab::Content);
    }

    #[test]
    fn refresh_preview_requests_the_same_row_again() {
        let (mut a, _) = app_with_preview(Tab::Files);
        feed(&mut a, &["a"]);
        let g0 = a.preview_gen;
        assert_eq!(a.refresh_preview(), [Effect::Preview]);
        assert_eq!(a.preview_gen, g0 + 1);
        let (mut empty, _) = app(Tab::Files);
        assert_eq!(empty.refresh_preview(), []);
    }

    #[test]
    fn empty_list_clears_the_preview() {
        let (mut a, now) = app_with_preview(Tab::Files);
        feed(&mut a, &["a"]);
        a.on_preview(PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::raw("content"),
        });
        typed(&mut a, "zzz", now);
        assert_eq!(a.preview, Text::default());
    }
}
