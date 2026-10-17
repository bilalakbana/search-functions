//! The app's state and what keys do to it. Pure: it returns effects, the main loop carries them out.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::text::Text;

use crate::actions::ActionKind;
use crate::commands::{self, ListPlan, PreviewPlan};
use crate::engines::Engines;
use crate::filter::Filter;
use crate::items::{self, Item};
use crate::model::{Opt, Options, Tab};
use crate::preview::PreviewMsg;
use crate::runner::ListMsg;
use crate::views::ViewKind;

/// Search runs this long after the last keystroke.
pub const SEARCH_DELAY: Duration = Duration::from_millis(150);
const STATUS_OK: Duration = Duration::from_secs(3);
const STATUS_ERROR: Duration = Duration::from_secs(6);
/// Two clicks on the same row within this time are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
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
    Quit,
}

/// Ctrl-T: Content narrowed to one file, the lines with any of the typed words; Esc goes back.
#[derive(Clone, Debug)]
pub struct Scope {
    /// The file, as its row named it (relative to the root).
    pub key: OsString,
    /// The tab and the query to go back to.
    back: (Tab, String),
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
    pub popup_rows: Vec<(Opt, Rect)>,
}

pub struct App {
    pub tab: Tab,
    pub root: PathBuf,
    /// Shared by all tabs: a filter in Files, Folders and Git, the pattern in Search.
    pub query: String,
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
    pub preview_scroll: u16,
    pub preview_visible: bool,
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
            preview_scroll: 0,
            preview_visible: true,
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
        vec![self.run_list()]
    }

    pub fn current(&self) -> Option<&Item> {
        self.filter.get(self.selected)
    }

    /// What ugrep searches for: the query, or in a file (Ctrl-T) any of its words.
    pub fn pattern(&self) -> String {
        match self.scope {
            Some(_) => self.query.split_whitespace().collect::<Vec<_>>().join("|"),
            None => self.query.clone(),
        }
    }

    pub fn list_plan(&self) -> ListPlan {
        if let Some(scope) = &self.scope {
            return commands::in_file_plan(&scope.key, &self.pattern(), &self.engines);
        }
        commands::list_plan_with(
            self.tab,
            &self.options,
            &self.query,
            &self.engines,
            self.extra_options(),
            false,
        )
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
            options.content.matching_lines = true;
            options.content.archives = false;
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
    }

    pub fn set_status(&mut self, text: &str, error: bool, now: Instant) {
        self.status = Some((text.to_string(), error, now));
    }

    pub fn set_root(&mut self, root: PathBuf) -> Vec<Effect> {
        self.root = root;
        self.query.clear();
        self.preview = Text::default();
        vec![self.run_list()]
    }

    pub fn handle_key(&mut self, key: KeyEvent, now: Instant) -> Vec<Effect> {
        if key.kind == KeyEventKind::Release {
            return Vec::new();
        }
        if self.view.is_some() {
            return self.view_key(key);
        }
        if self.scope.is_some() {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => return self.leave_scope(),
                // no options in a file; already in it
                KeyCode::Char('s' | 't') if ctrl => return Vec::new(),
                _ => {}
            }
        }
        if self.popup.is_some() {
            return self.popup_key(key, now);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
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
            KeyCode::Char(c) if ctrl => match c {
                'w' | 'h' => self.edit(delete_word, now),
                'l' => self.toggle_preview(),
                'r' => vec![self.run_list()],
                's' => {
                    self.popup = Some(0);
                    Vec::new()
                }
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
        if self.popup.is_some() {
            if !left_click {
                return Vec::new();
            }
            if let Some(i) = self
                .layout
                .popup_rows
                .iter()
                .position(|(_, r)| r.contains(at))
            {
                self.popup = Some(i);
                return self.toggle_option(i, now);
            }
            if !self.layout.popup.contains(at) {
                self.popup = None;
            }
            return Vec::new();
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
                if self.layout.preview.contains(at) {
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
        match self.search_due {
            Some(due) if now >= due => vec![self.run_list()],
            _ => Vec::new(),
        }
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
        self.preview = msg.text;
        // a matching line about a third down the preview, below the header
        let third = 1 + self.layout.preview.height as usize / 3;
        let at = self
            .current()
            .and_then(|i| i.line)
            .map_or(0, |n| (msg.header_lines + n as usize).saturating_sub(third));
        self.preview_scroll = u16::try_from(at).unwrap_or(u16::MAX);
    }

    /// Starts the current tab's list afresh.
    fn run_list(&mut self) -> Effect {
        self.list_gen += 1;
        self.filter.clear();
        self.filter
            .set_query(if self.tab.searches() { "" } else { &self.query });
        self.notes.clear();
        self.selected = 0;
        self.offset = 0;
        self.loading = true;
        self.search_due = None;
        self.previewed = None;
        Effect::RunList
    }

    fn switch_tab(&mut self, tab: Tab) -> Vec<Effect> {
        self.tab = tab;
        self.preview = Text::default();
        self.preview_scroll = 0;
        vec![self.run_list()]
    }

    fn edit(&mut self, f: impl FnOnce(&mut String), now: Instant) -> Vec<Effect> {
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
        if !self.preview_visible {
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
            return Vec::new();
        }
        vec![Effect::Preview]
    }

    /// Requests the current row's preview again (the preview's width changed).
    pub fn refresh_preview(&mut self) -> Vec<Effect> {
        self.previewed = None;
        self.preview_if_changed()
    }

    fn toggle_preview(&mut self) -> Vec<Effect> {
        self.preview_visible = !self.preview_visible;
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
            Tab::Git => match item.key.to_str().filter(|h| commands::is_hash(h)) {
                Some(hash) => self.open_view(ViewKind::Commit {
                    root: self.root.clone(),
                    hash: hash.to_string(),
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
            let key = item.key.clone();
            return self.enter_scope(key);
        }
        let path = self.root.join(&item.key);
        let view = match (self.tab, kind) {
            (Tab::Git, _) => None, // the commit's keys; the others say they are not for Git
            (_, ActionKind::Diff) => Some(ViewKind::Diff { path, full: true }),
            (_, ActionKind::DiffShort) => Some(ViewKind::Diff { path, full: false }),
            (_, ActionKind::DiffSplit) => Some(ViewKind::History { path }),
            (_, ActionKind::Log) => Some(ViewKind::Log { path }),
            _ => None,
        };
        match view {
            Some(kind) => self.open_view(kind),
            None => vec![Effect::Act(kind)],
        }
    }

    fn enter_scope(&mut self, key: OsString) -> Vec<Effect> {
        self.scope = Some(Scope {
            key,
            back: (self.tab, self.query.clone()),
        });
        if !self.tab.searches() {
            self.query.clear(); // a filter of names is no pattern for the lines
        }
        self.tab = Tab::Content;
        self.preview = Text::default();
        self.preview_scroll = 0;
        vec![self.run_list()]
    }

    fn leave_scope(&mut self) -> Vec<Effect> {
        let Some(scope) = self.scope.take() else {
            return Vec::new();
        };
        (self.tab, self.query) = scope.back;
        self.preview = Text::default();
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
    }

    /// Keys while a view is open: they never reach the tabs.
    fn view_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let page = (self.layout.view.height as usize).max(1);
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
        let n = self.options.list(self.tab).len();
        match key.code {
            KeyCode::Esc => self.popup = None,
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.popup = None
            }
            KeyCode::Up => self.popup = Some((cursor + n - 1) % n),
            KeyCode::Down => self.popup = Some((cursor + 1) % n),
            KeyCode::Char(' ') | KeyCode::Enter => return self.toggle_option(cursor, now),
            _ => {}
        }
        Vec::new()
    }

    /// Toggles the current tab's option at `index` (popup order).
    pub fn toggle_option(&mut self, index: usize, now: Instant) -> Vec<Effect> {
        let Some(&(opt, _)) = self.options.list(self.tab).get(index) else {
            return Vec::new();
        };
        self.options.toggle(self.tab, opt);
        let mut fx = vec![self.run_list()];
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
            header_lines: 0,
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
            (ctrl('u'), ViewKind::History { path: path.clone() }),
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
                hash: "abcdef1".into()
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
            header_lines: 0,
        });
        assert!(
            a.view.as_ref().unwrap().loading,
            "an older view's text is dropped"
        );
        a.on_view(PreviewMsg {
            generation: a.view_gen,
            text: Text::default(),
            header_lines: 0,
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
        assert_eq!(a.tab, Tab::Git);
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
        let (mut a, _) = app(Tab::Files);
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
        let (mut a, now) = app(Tab::Files);
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
        assert_eq!(a.popup, Some(4), "the cursor wraps");
        a.handle_key(key(KeyCode::Enter), now);
        assert!(!a.options.content.ignored, "on by default, so off now");
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
        let (mut a, now) = app(Tab::Files);
        let old = a.list_gen;
        a.handle_key(ctrl('r'), now);
        a.on_list(ListMsg::Items(old, rows(ParseKind::Files, &["stale"])));
        a.on_list(ListMsg::Error(old, "stale error".into()));
        assert_eq!((a.filter.total(), a.notes.len()), (0, 0));
        feed(&mut a, &["a"]);
        let g0 = a.preview_gen;
        a.on_preview(PreviewMsg {
            header_lines: 0,
            generation: g0 - 1,
            text: Text::raw("old"),
        });
        assert_eq!(a.preview, Text::default());
        a.on_preview(PreviewMsg {
            header_lines: 0,
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
            header_lines: 0,
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
            header_lines: 0,
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
            header_lines: 0,
            generation: a.preview_gen,
            text: Text::raw("t"),
        });
        assert_eq!(a.preview_scroll, 39);
    }

    #[test]
    fn matching_line_scroll_skips_the_header() {
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
            header_lines: 4,
            generation: a.preview_gen,
            text: Text::raw("t"),
        });
        assert_eq!(a.preview_scroll, 43);
    }

    #[test]
    fn preview_scroll_and_toggle() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        a.layout.preview = Rect::new(1, 2, 80, 10);
        a.on_preview(PreviewMsg {
            header_lines: 0,
            generation: a.preview_gen,
            text: Text::raw("x\n".repeat(100)),
        });
        a.handle_key(alt(KeyCode::Down), now);
        assert_eq!(a.preview_scroll, 10);
        a.handle_key(alt(KeyCode::Up), now);
        a.handle_key(alt(KeyCode::Up), now);
        assert_eq!(a.preview_scroll, 0);
        assert_eq!(a.handle_key(ctrl('l'), now), []);
        assert!(!a.preview_visible);
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
        let (mut a, now) = app(Tab::Files);
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
    fn ctrl_t_searches_inside_the_file_with_any_of_the_words() {
        let (mut a, now) = app(Tab::Files);
        a.query = "a.c".into();
        feed(&mut a, &["src/a.c"]);
        assert_eq!(a.handle_key(ctrl('t'), now), [Effect::RunList]);
        assert_eq!(
            (a.tab, a.query.as_str()),
            (Tab::Content, ""),
            "a Files filter is no pattern"
        );
        let ListPlan::Run { cmd, kind } = a.list_plan() else {
            panic!()
        };
        assert_eq!(kind, ParseKind::SearchLines);
        assert_eq!(cmd.args.last(), Some(&OsString::from("src/a.c")));
        for c in "foo  bar".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
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
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        a.on_preview(PreviewMsg {
            header_lines: 0,
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
        let (mut a, _) = app(Tab::Files);
        feed(&mut a, &["a"]);
        let g0 = a.preview_gen;
        assert_eq!(a.refresh_preview(), [Effect::Preview]);
        assert_eq!(a.preview_gen, g0 + 1);
        let (mut empty, _) = app(Tab::Files);
        assert_eq!(empty.refresh_preview(), []);
    }

    #[test]
    fn empty_list_clears_the_preview() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        a.on_preview(PreviewMsg {
            header_lines: 0,
            generation: a.preview_gen,
            text: Text::raw("content"),
        });
        typed(&mut a, "zzz", now);
        assert_eq!(a.preview, Text::default());
    }
}
