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
    Quit,
}

/// Where things were drawn, for the mouse; filled by `ui::draw`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreenLayout {
    pub tabs: Vec<(Tab, Rect)>,
    pub config: Rect,
    /// The rows of the list (inside its border); the notes come first.
    pub list: Rect,
    /// The text area of the preview (inside its border); empty when hidden.
    pub preview: Rect,
    /// The popup with its border; empty when closed.
    pub popup: Rect,
    pub popup_rows: Vec<(Opt, Rect)>,
}

pub struct App {
    pub tab: Tab,
    pub root: PathBuf,
    /// Shared by all tabs: a filter in Files, Folders and Git, the pattern in Search.
    pub query: String,
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

    pub fn list_plan(&self) -> ListPlan {
        commands::list_plan(self.tab, &self.options, &self.query, &self.engines)
    }

    pub fn preview_plan(&self) -> PreviewPlan {
        match self.current() {
            Some(item) => commands::preview_plan(
                self.tab,
                &self.options,
                &self.query,
                item,
                self.layout.preview.width.max(20),
                &self.engines,
                &self.root,
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
            KeyCode::Up => self.select(self.selected.saturating_sub(1)),
            KeyCode::Down => self.select(self.selected + 1),
            KeyCode::PageUp => self.select(self.selected.saturating_sub(page)),
            KeyCode::PageDown => self.select(self.selected + page),
            KeyCode::Home => self.select(0),
            KeyCode::End => self.select(usize::MAX),
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
                    self.select(if down {
                        self.selected + WHEEL
                    } else {
                        self.selected.saturating_sub(WHEEL)
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
        let row = usize::from(at.y - self.layout.list.y);
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
                if self.tab == Tab::Content
                    && self.filter.total() == 0
                    && self.notes.is_empty()
                    && self.query.is_empty()
                {
                    self.notes.push(items::hint("No text files here"));
                } else if self.tab == Tab::Content
                    && self.filter.total() == 0
                    && self.notes.is_empty()
                {
                    let mut hint = "No match".to_string();
                    if !self.options.content.archives {
                        hint += ". Archives & documents (⚙ Config) also searches inside zip, PDF and Office files";
                    }
                    self.notes.push(items::hint(&hint));
                }
            }
            _ => {}
        }
        Vec::new()
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
        self.filter.set_query(if self.tab == Tab::Content {
            ""
        } else {
            &self.query
        });
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
        if self.tab == Tab::Content {
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
            Tab::Git => vec![Effect::Act(ActionKind::ShowCommit)],
            _ => vec![Effect::Act(ActionKind::Open)],
        }
    }

    fn act(&mut self, kind: ActionKind, now: Instant) -> Vec<Effect> {
        if self.current().is_some_and(|i| i.selectable) {
            vec![Effect::Act(kind)]
        } else {
            self.set_status("Nothing selected", true, now);
            Vec::new()
        }
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

    pub fn rows(kind: ParseKind, raws: &[&str]) -> Vec<Item> {
        raws.iter()
            .map(|r| items::parse(kind, r.as_bytes(), 1).unwrap())
            .collect()
    }

    pub fn feed(a: &mut App, names: &[&str]) -> Vec<Effect> {
        let kind = match a.tab {
            Tab::Folders => ParseKind::Folders,
            Tab::Content => ParseKind::SearchFiles { archives: false },
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
        assert_eq!(a.handle_key(key(KeyCode::Down), now), [Effect::Preview]);
        assert_eq!(current_key(&a), "b");
        assert_eq!(a.handle_key(key(KeyCode::Down), now), []);
        assert_eq!(a.handle_key(key(KeyCode::Home), now), [Effect::Preview]);
        assert_eq!(current_key(&a), "a");
        a.handle_key(key(KeyCode::End), now);
        assert_eq!(current_key(&a), "b");
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
        a.handle_key(key(KeyCode::PageDown), now);
        assert_eq!(current_key(&a), "f10");
        a.handle_key(key(KeyCode::PageUp), now);
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
        assert_eq!(a.popup, Some(1));
        assert_eq!(
            a.handle_key(key(KeyCode::Char(' ')), now),
            [Effect::RunList, Effect::SaveConfig]
        );
        assert!(a.options.content.archives);
        assert_eq!(a.query, "", "space in the popup is no typing");
        a.handle_key(key(KeyCode::Up), now);
        a.handle_key(key(KeyCode::Up), now);
        assert_eq!(a.popup, Some(3), "the cursor wraps");
        a.handle_key(key(KeyCode::Enter), now);
        assert!(a.options.content.ignored);
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
        a.handle_key(key(KeyCode::Down), now);
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
    fn enter_elsewhere_opens_or_shows_the_commit() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        assert_eq!(
            a.handle_key(key(KeyCode::Enter), now),
            [Effect::Act(ActionKind::Open)]
        );
        let (mut g, now) = app(Tab::Git);
        g.on_list(ListMsg::Items(
            g.list_gen,
            rows(ParseKind::Git, &["* x\x1fabcdef1"]),
        ));
        assert_eq!(
            g.handle_key(key(KeyCode::Enter), now),
            [Effect::Act(ActionKind::ShowCommit)]
        );
    }

    #[test]
    fn action_keys() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a"]);
        let cases = [
            (ctrl('o'), ActionKind::Editor),
            (ctrl('t'), ActionKind::InFile),
            (ctrl('u'), ActionKind::DiffSplit),
            (ctrl('g'), ActionKind::Diff),
            (alt(KeyCode::Char('g')), ActionKind::DiffShort),
            (alt(KeyCode::Char('u')), ActionKind::Log),
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

    fn list_row(a: &App, i: u16) -> Rect {
        Rect::new(a.layout.list.x, a.layout.list.y + i, a.layout.list.width, 1)
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
    fn wheel_over_list_moves() {
        let (mut a, now) = app(Tab::Files);
        let names: Vec<String> = (0..10).map(|i| format!("f{i}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        drawn(&mut a);
        a.handle_mouse(mouse(MouseEventKind::ScrollDown, a.layout.list), now);
        assert_eq!(current_key(&a), "f3");
        a.handle_mouse(mouse(MouseEventKind::ScrollUp, a.layout.list), now);
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
        let row = a.layout.popup_rows[1].1;
        assert_eq!(
            a.handle_mouse(click(row), now),
            [Effect::RunList, Effect::SaveConfig]
        );
        assert!(a.options.content.archives);
        assert_eq!(a.popup, Some(1));
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
