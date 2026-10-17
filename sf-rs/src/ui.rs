//! Drawing. Top to bottom: the tab bar (the keys for the selected item in its middle), the preview,
//! the list (filled from the bottom, as fzf) and the input line; the
//! Config popup over them. Records where everything went in `app.layout` for the mouse.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

use crate::app::{App, ScreenLayout};
use crate::model::{Opt, Tab};
use crate::theme;

const CONFIG: &str = "⚙ Config";

pub fn draw(frame: &mut Frame, app: &mut App) {
    if app.view.is_some() {
        app.layout = ScreenLayout {
            view: draw_view(frame, app),
            ..ScreenLayout::default()
        };
        return;
    }
    let area = frame.area();
    let body = area.height.saturating_sub(2);
    let preview_h = if app.preview_visible() {
        body * 60 / 100
    } else {
        0
    };
    let [bar, preview, list, input] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(preview_h),
        Constraint::Length(body - preview_h),
        Constraint::Length(1),
    ])
    .areas(area);
    let mut layout = ScreenLayout::default();
    draw_tab_bar(frame, app, bar, &mut layout);
    let status = status_line(app);
    if app.preview_visible() {
        layout.preview = draw_preview(frame, app, preview, status.clone());
    }
    layout.list = draw_list(
        frame,
        app,
        list,
        if app.preview_visible() { None } else { status },
    );
    draw_input(frame, app, input);
    if let Some(cursor) = app.popup {
        draw_popup(frame, app, cursor, &mut layout);
    }
    app.layout = layout;
}

fn style(fg: ratatui::style::Color) -> Style {
    Style::new().fg(fg)
}

fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect, layout: &mut ScreenLayout) {
    let config_w = CONFIG.chars().count() as u16;
    layout.config = Rect::new(
        area.right().saturating_sub(config_w),
        area.y,
        config_w.min(area.width),
        1,
    );
    let config_style = if app.popup.is_some() {
        Style::new()
            .fg(theme::BASE)
            .bg(theme::MAUVE)
            .add_modifier(Modifier::BOLD)
    } else {
        style(theme::SUB)
    };
    frame.render_widget(
        Paragraph::new(Span::styled(CONFIG, config_style)),
        layout.config,
    );

    let room = area.width.saturating_sub(config_w + 1);
    let mut x = area.x;
    for tab in Tab::ALL {
        let label = format!(" {} ", tab.title());
        let w = (label.chars().count() as u16).min((area.x + room).saturating_sub(x));
        if w == 0 {
            break;
        }
        let st = if tab == app.tab {
            Style::new()
                .fg(theme::BASE)
                .bg(theme::MAUVE)
                .add_modifier(Modifier::BOLD)
        } else {
            style(theme::DIM)
        };
        let rect = Rect::new(x, area.y, w, 1);
        frame.render_widget(Paragraph::new(Span::styled(label, st)), rect);
        layout.tabs.push((tab, rect));
        x += w + 1;
    }
    // the keys for the selected item, between the tabs and Config
    let from = layout.tabs.last().map_or(area.x, |(_, r)| r.right()) + 2;
    let to = layout.config.x.saturating_sub(2);
    if to > from {
        draw_keys(frame, app, Rect::new(from, area.y, to - from, 1));
    }
}

/// A thin bar on the right border of `area` (a bordered block) when `total` rows do not fit.
fn draw_scrollbar(frame: &mut Frame, area: Rect, total: usize, position: usize, visible: usize) {
    if total <= visible || area.height < 3 {
        return;
    }
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .track_style(style(theme::FAINT))
        .thumb_symbol("┃")
        .thumb_style(style(theme::SUB));
    let mut state = ScrollbarState::new(total - visible + 1)
        .position(position)
        .viewport_content_length(visible);
    let track = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    frame.render_stateful_widget(bar, track, &mut state);
}

fn status_line(app: &App) -> Option<Line<'static>> {
    let (text, error, _) = app.status.as_ref()?;
    let st = if *error {
        style(theme::RED).add_modifier(Modifier::BOLD)
    } else {
        style(theme::GREEN)
    };
    Some(Line::from(Span::styled(
        format!(" {} ", crate::items::printable(text)),
        st,
    )))
}

fn bordered() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(style(theme::FAINT))
}

/// The root, `~` for the home folder, coloured as every path (`items::path_spans`).
fn root_title(app: &App) -> Line<'static> {
    let mut path = app.root.display().to_string();
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = app.root.strip_prefix(&home) {
            path = if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    let mut spans = vec![Span::raw(" ")];
    spans.extend(crate::items::path_spans(&crate::items::printable(&path)));
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// A view over the whole screen: its text in a border titled with what it shows, the keys below (or
/// the search being typed) and where the view is. Returns the text area.
fn draw_view(frame: &mut Frame, app: &App) -> Rect {
    let Some(v) = &app.view else {
        return Rect::default();
    };
    let [body, footer] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(frame.area());
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(crate::items::printable(&v.kind.title()), style(theme::TEXT)),
        Span::raw(" "),
    ]);
    let block = bordered().title_top(title);
    let inner = block.inner(body);
    frame.render_widget(block, body);
    let total = v.text.lines.len();
    let from = v.scroll.min(total.saturating_sub(inner.height as usize));
    let mut shown: Vec<Line> = v.text.lines[from..]
        .iter()
        .take(inner.height as usize)
        .cloned()
        .collect();
    if v.loading && shown.is_empty() {
        shown.push(Line::styled("…", style(theme::DIM)));
    }
    if let Some(i) = v
        .found
        .and_then(|f| f.checked_sub(from))
        .filter(|&i| i < shown.len())
    {
        shown[i] = shown[i]
            .clone()
            .patch_style(Style::new().bg(theme::SURFACE));
    }
    frame.render_widget(Paragraph::new(Text::from(shown)), inner);
    draw_scrollbar(frame, body, total, from, inner.height as usize);

    let left = if v.searching {
        Line::from(vec![
            Span::styled("/ ", style(theme::MAUVE)),
            Span::styled(crate::items::printable(&v.search), style(theme::TEXT)),
            Span::styled("▏", style(theme::SUB)),
        ])
    } else if !v.search.is_empty() && v.found.is_none() {
        Line::styled(
            format!("not found: {}", crate::items::printable(&v.search)),
            style(theme::RED),
        )
    } else {
        Line::styled(
            "Esc back · ↑↓ PgUp PgDn Home End · / search · n N next",
            style(theme::DIM),
        )
    };
    let position = if total == 0 {
        String::new()
    } else {
        format!("{}/{total} ", from + 1)
    };
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(position.chars().count() as u16),
    ])
    .areas(footer);
    frame.render_widget(Paragraph::new(left), left_area);
    frame.render_widget(
        Paragraph::new(Span::styled(position, style(theme::DIM))),
        right_area,
    );
    inner
}

fn draw_preview(frame: &mut Frame, app: &App, area: Rect, status: Option<Line<'static>>) -> Rect {
    let mut block = bordered().title_top(root_title(app));
    if let Some(status) = status {
        block = block.title_bottom(status);
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Only the lines that can be on screen: a preview can have hundreds of thousands.
    let from = (app.preview_scroll as usize).min(app.preview.lines.len());
    let shown: Vec<Line> = app.preview.lines[from..]
        .iter()
        .take(inner.height as usize)
        .cloned()
        .collect();
    frame.render_widget(
        Paragraph::new(Text::from(shown)).wrap(Wrap { trim: false }),
        inner,
    );
    draw_scrollbar(
        frame,
        area,
        app.preview.lines.len(),
        from,
        inner.height as usize,
    );
    inner
}

fn draw_list(frame: &mut Frame, app: &mut App, area: Rect, status: Option<Line<'static>>) -> Rect {
    let mut block = bordered();
    if !app.preview_visible() {
        block = block.title_top(root_title(app)); // else in the preview's border
    }
    if let Some(status) = status {
        block = block.title_bottom(status);
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let notes = app.notes.len().min(inner.height as usize);
    let visible = inner.height as usize - notes;
    if app.selected < app.offset {
        app.offset = app.selected;
    } else if visible > 0 && app.selected >= app.offset + visible {
        app.offset = app.selected + 1 - visible;
    }
    // bottom up, as in fzf: the notes next to the input line, then the rows, the first one lowest
    let mut y = inner.bottom();
    for note in &app.notes[..notes] {
        y -= 1;
        let line = Line::from(
            [Span::raw(" ")]
                .into_iter()
                .chain(note.display().spans)
                .collect::<Vec<_>>(),
        );
        frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
    }
    // the filter's words, drawn red in the rows (Search's query is a pattern for the contents)
    let words: Vec<String> = match &app.result_filter {
        Some(words) => words.split_whitespace().map(str::to_lowercase).collect(),
        None if app.tab.searches() => Vec::new(),
        None => app
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect(),
    };
    for i in app.offset..app.offset + visible {
        let Some(item) = app.filter.get(i) else { break };
        y -= 1;
        let rect = Rect::new(inner.x, y, inner.width, 1);
        let selected = i == app.selected;
        let marker = if selected {
            Span::styled("▌", style(theme::MAUVE))
        } else {
            Span::raw(" ")
        };
        let mut spans: Vec<Span> = [marker]
            .into_iter()
            .chain(highlight(item.display(), &words).spans)
            .collect();
        if selected {
            // the band under every span: coloured engine output carries its own (reset) background
            frame
                .buffer_mut()
                .set_style(rect, Style::new().bg(theme::SURFACE));
            for span in &mut spans {
                span.style = span.style.bg(theme::SURFACE);
            }
        }
        let line = Line::from(spans);
        frame.render_widget(Paragraph::new(line), rect);
    }
    // the bar counts from the top, where the last rows are
    let total = app.filter.len();
    let from_top = total.saturating_sub(visible).saturating_sub(app.offset);
    draw_scrollbar(frame, area, total, from_top, visible);
    inner
}

/// Every occurrence of every word (lower case) in `line`, case-insensitive, drawn red and bold.
fn highlight(line: Line<'static>, words: &[String]) -> Line<'static> {
    if words.is_empty() {
        return line;
    }
    let chars: Vec<(usize, char)> = line
        .spans
        .iter()
        .enumerate()
        .flat_map(|(i, s)| s.content.chars().map(move |c| (i, c)))
        .collect();
    // the line in lower case, each lower-case char pointing back at the char it came from
    let (mut lower, mut from) = (Vec::new(), Vec::new());
    for (k, &(_, c)) in chars.iter().enumerate() {
        for l in c.to_lowercase() {
            lower.push(l);
            from.push(k);
        }
    }
    let mut hit = vec![false; chars.len()];
    for word in words {
        let w: Vec<char> = word.chars().collect();
        if w.is_empty() || w.len() > lower.len() {
            continue;
        }
        for start in 0..=lower.len() - w.len() {
            if lower[start..start + w.len()] == w[..] {
                for &k in &from[start..start + w.len()] {
                    hit[k] = true;
                }
            }
        }
    }
    if !hit.contains(&true) {
        return line;
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = (usize::MAX, false, String::new());
    for (k, &(span, c)) in chars.iter().enumerate() {
        if (run.0, run.1) != (span, hit[k]) && !run.2.is_empty() {
            spans.push(hit_span(&line, &mut run));
        }
        (run.0, run.1) = (span, hit[k]);
        run.2.push(c);
    }
    if !run.2.is_empty() {
        spans.push(hit_span(&line, &mut run));
    }
    Line { spans, ..line }
}

fn hit_span(line: &Line<'static>, run: &mut (usize, bool, String)) -> Span<'static> {
    let mut style = line.spans[run.0].style;
    if run.1 {
        style = style.fg(theme::RED).add_modifier(Modifier::BOLD);
    }
    Span::styled(std::mem::take(&mut run.2), style)
}

fn short(opt: Opt) -> &'static str {
    match opt {
        Opt::Hidden => "hidden",
        Opt::Ignored => "ignored",
        Opt::MatchingLines => "matching lines",
        Opt::EachMatch => "each match",
        Opt::Archives => "archives & documents",
        Opt::Documents => "documents",
        Opt::AllBranches => "all branches",
    }
}

fn placeholder(app: &App) -> &'static str {
    if app.scope.is_some() {
        return "type words · the lines with any of them";
    }
    match app.tab {
        Tab::Files => "type to filter file names",
        Tab::Content if app.options.content.matching_lines => {
            "type a pattern · lists the lines that contain it"
        }
        Tab::Content if app.options.content.each_match => {
            "type a pattern · lists every match with the text around it"
        }
        Tab::Content => "type a pattern · lists the files that contain it",
        Tab::Deep if app.options.deep.each_match => {
            "type a pattern · every match with the text around it, in binaries too"
        }
        Tab::Deep => "type a pattern · the files that contain it, binaries and archives too",
        Tab::Folders => "type to filter folders · Enter goes inside, .. goes up",
        Tab::Git => "type to filter commits · Enter shows one",
    }
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let mut active: Vec<String> = app
        .options
        .list(app.tab)
        .into_iter()
        .filter(|(_, on)| *on && app.scope.is_none()) // none applies inside one file
        .map(|(o, _)| short(o).to_string())
        .collect();
    let extra = app.extra_options();
    if !extra.is_empty() {
        let words: Vec<_> = extra.iter().map(|a| a.to_string_lossy()).collect();
        active.push(crate::items::printable(&format!("-- {}", words.join(" "))));
    }
    let mut count = format!("{}/{}", app.filter.len(), app.filter.total());
    if app.loading {
        count.push_str(" …");
    }
    let right = Line::from(vec![
        Span::styled(active.join(" · "), style(theme::SUB)),
        Span::raw("   "),
        Span::styled(count, style(theme::DIM)),
        Span::raw(" "),
    ]);
    let right_w = (right.width() as u16).min(area.width / 2);
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w)]).areas(area);

    let title = match &app.scope {
        Some(scope) => {
            let name = std::path::Path::new(&scope.key)
                .file_name()
                .unwrap_or(&scope.key);
            format!("In {}", crate::items::printable(&name.to_string_lossy()))
        }
        None => app.tab.title().to_string(),
    };
    let query = crate::items::printable(&app.query);
    let mut left = vec![
        Span::raw(" "),
        Span::styled(
            title.clone(),
            style(theme::MAUVE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" › ", style(theme::FAINT)),
    ];
    let mut cursor_x = left_area.x
        + 1
        + title.chars().count() as u16
        + 3
        + Line::raw(query.as_str()).width() as u16;
    let hint =
        |text: &'static str| Span::styled(text, style(theme::DIM).add_modifier(Modifier::ITALIC));
    if query.is_empty() {
        left.push(hint(placeholder(app)));
    } else {
        left.push(Span::styled(query, style(theme::TEXT)));
    }
    // Ctrl-F: the pattern stays; the cursor is in the words that filter the rows
    if let Some(words) = &app.result_filter {
        let words = crate::items::printable(words);
        left.push(Span::styled(" · filter: ", style(theme::FAINT)));
        cursor_x += " · filter: ".chars().count() as u16 + Line::raw(words.as_str()).width() as u16;
        if words.is_empty() {
            left.push(hint("type words · the rows with all of them"));
        } else {
            left.push(Span::styled(words, style(theme::TEXT)));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(left)), left_area);
    frame.render_widget(
        Paragraph::new(right).alignment(ratatui::layout::Alignment::Right),
        right_area,
    );
    if app.popup.is_none() {
        frame.set_cursor_position(Position::new(
            cursor_x.min(left_area.right().saturating_sub(1)),
            area.y,
        ));
    }
}

fn footer_items(tab: Tab, editor: &str) -> Vec<String> {
    let items: &[&str] = match tab {
        Tab::Files => &[
            "Enter open",
            "^S options",
            "^O",
            "^T in file",
            "^U/^G diff",
            "Alt-U log",
            "^E reveal",
            "^C copy",
            "Esc quit",
        ],
        Tab::Content | Tab::Deep => &[
            "Enter open",
            "^S options",
            "^F filter",
            "^O",
            "^T in file",
            "^U/^G diff",
            "Alt-U log",
            "^E reveal",
            "^C copy",
            "Esc quit",
        ],
        Tab::Folders => &[
            "Enter go inside",
            "^S options",
            "^E reveal",
            "^C copy",
            "Esc quit",
        ],
        Tab::Git => &[
            "Enter show commit",
            "^S options",
            "^C copy hash",
            "Esc quit",
        ],
    };
    items
        .iter()
        .map(|i| match *i {
            "^O" => format!("^O {editor}"),
            i => i.to_string(),
        })
        .collect()
}

/// The keys for the selected item, centred in `area`; when narrow, items before "Esc quit" are left
/// out, never cut.
fn draw_keys(frame: &mut Frame, app: &App, area: Rect) {
    let width = |items: &[String]| {
        items.iter().map(|i| i.chars().count()).sum::<usize>() + 3 * items.len().saturating_sub(1)
    };
    let mut items = footer_items(app.tab, &app.editor);
    if app.result_filter.is_some() || app.scope.is_some() {
        // Esc leaves the filter or the file, not sf
        if let Some(esc) = items.last_mut() {
            *esc = "Esc back".into();
        }
    }
    while items.len() > 1 && width(&items) > area.width as usize {
        items.remove(items.len() - 2);
    }
    let w = width(&items) as u16;
    if w > area.width {
        return;
    }
    let area = Rect::new(area.x + (area.width - w) / 2, area.y, w, 1);
    let mut spans = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", style(theme::FAINT)));
        }
        let (key, what) = item.split_once(' ').unwrap_or((item, ""));
        spans.push(Span::styled(key.to_string(), style(theme::SUB)));
        spans.push(Span::styled(format!(" {what}"), style(theme::DIM)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_popup(frame: &mut Frame, app: &App, cursor: usize, layout: &mut ScreenLayout) {
    let opts = app.options.list(app.tab);
    let rows: Vec<Line> = opts
        .iter()
        .map(|&(opt, on)| {
            Line::from(vec![
                Span::styled(
                    if on { "[x]" } else { "[ ]" },
                    style(if on { theme::MAUVE } else { theme::DIM }),
                ),
                Span::styled(format!(" {}", opt.label(app.tab)), style(theme::TEXT)),
                Span::styled(format!("   {}", opt.hint()), style(theme::DIM)),
            ])
        })
        .collect();
    let help = "Space / click · Esc close";
    let title = format!(" {} options ", app.tab.title());
    let content_w = rows
        .iter()
        .map(Line::width)
        .chain([help.chars().count(), title.chars().count()])
        .max()
        .unwrap_or(0);
    let area = frame.area();
    let w = (content_w as u16 + 4).min(area.width);
    let h = (opts.len() as u16 + 4).min(area.height);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(style(theme::MAUVE))
        .title_top(Span::styled(
            title,
            style(theme::MAUVE).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    for (i, (line, &(opt, _))) in rows.into_iter().zip(&opts).enumerate() {
        let y = inner.y + i as u16;
        if y >= inner.bottom() {
            break;
        }
        let rect = Rect::new(inner.x, y, inner.width, 1);
        if i == cursor {
            frame
                .buffer_mut()
                .set_style(rect, Style::new().bg(theme::SURFACE));
        }
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(inner.x + 1, y, inner.width.saturating_sub(1), 1),
        );
        layout.popup_rows.push((opt, rect));
    }
    if inner.height > 0 {
        let help_area = Rect::new(
            inner.x,
            inner.bottom() - 1,
            inner.width.saturating_sub(1),
            1,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(help, style(theme::FAINT)))
                .alignment(ratatui::layout::Alignment::Right),
            help_area,
        );
    }
    layout.popup = popup;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{app, app_with_preview, ctrl, feed, key};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::KeyCode;
    use std::time::Instant;

    fn render(a: &mut App, w: u16, h: u16) -> Buffer {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, a)).unwrap();
        t.backend().buffer().clone()
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    fn find(buf: &Buffer, text: &str) -> Option<(u16, u16)> {
        (0..buf.area.height).find_map(|y| {
            let r = row(buf, y);
            r.find(text).map(|i| (r[..i].chars().count() as u16, y))
        })
    }

    #[test]
    fn tab_bar_is_one_line_with_config_right() {
        for w in [80, 160] {
            let (mut a, _) = app(Tab::Files);
            let buf = render(&mut a, w, 24);
            let top = row(&buf, 0);
            for t in ["Files", "Content", "Folders", "Git"] {
                assert!(top.contains(t), "{w}: {top:?}");
            }
            assert!(top.trim_end().ends_with("⚙ Config"), "{w}: {top:?}");
            assert_eq!(buf[(w - 1, 0)].symbol(), "g");
            assert!(!row(&buf, 1).contains("Content"));
        }
    }

    #[test]
    fn current_tab_is_a_mauve_block() {
        let (mut a, _) = app(Tab::Content);
        let buf = render(&mut a, 80, 24);
        let (x, _) = find(&buf, "Content").unwrap();
        assert_eq!(buf[(x, 0)].bg, theme::MAUVE);
        let (x, _) = find(&buf, "Files").unwrap();
        assert_ne!(buf[(x, 0)].bg, theme::MAUVE);
    }

    #[test]
    fn narrow_60_keeps_config_visible() {
        let (mut a, _) = app(Tab::Files);
        feed(&mut a, &["a"]);
        let buf = render(&mut a, 60, 20);
        let top = row(&buf, 0);
        assert!(top.trim_end().ends_with("⚙ Config"), "{top:?}");
        assert!(top.contains("Git"), "{top:?}");
        let keys = top[top.find("Git").unwrap() + 3..top.find("⚙").unwrap()]
            .trim()
            .to_string();
        assert!(
            keys.is_empty() || keys.ends_with("quit"),
            "cut mid-item: {keys:?}"
        );
        assert!(!keys.ends_with('·'), "{keys:?}");
    }

    #[test]
    fn popup_shows_search_options_with_boxes() {
        let (mut a, now) = app(Tab::Content);
        a.handle_key(ctrl('s'), now);
        let buf = render(&mut a, 80, 24);
        assert!(find(&buf, "Content options").is_some());
        assert!(find(&buf, "[ ] Matching lines").is_some());
        assert!(find(&buf, "[ ] Each match").is_some());
        assert!(find(&buf, "[ ] Archives & documents").is_some());
        assert!(find(&buf, "[ ] Hidden files").is_some());
        assert!(find(&buf, "[x] Ignored files").is_some());
        assert_eq!(a.layout.popup_rows.len(), 5);
        assert_eq!(a.layout.popup_rows[0].0, Opt::MatchingLines);
        let (_, y) = find(&buf, "[ ] Matching lines").unwrap();
        assert_eq!(a.layout.popup_rows[0].1.y, y);
    }

    #[test]
    fn placeholder_when_query_empty() {
        let (mut a, now) = app(Tab::Content);
        let buf = render(&mut a, 100, 24);
        let input = row(&buf, 23);
        assert!(
            input.contains("Content › type a pattern · lists the files that contain it"),
            "{input:?}"
        );
        a.handle_key(key(KeyCode::Char('n')), now);
        let buf = render(&mut a, 100, 24);
        let input = row(&buf, 23);
        assert!(input.contains("Content › n"), "{input:?}");
        assert!(!input.contains("type a pattern"));
    }

    #[test]
    fn placeholder_says_each_match() {
        let (mut a, _) = app(Tab::Content);
        a.options.content.each_match = true;
        let buf = render(&mut a, 120, 24);
        let input = row(&buf, 23);
        assert!(
            input.contains("type a pattern · lists every match with the text around it"),
            "{input:?}"
        );
    }

    #[test]
    fn deep_says_what_it_searches() {
        let (mut a, _) = app(Tab::Deep);
        let input = row(&render(&mut a, 120, 24), 23);
        assert!(
            input.contains(
                "Deep › type a pattern · the files that contain it, binaries and archives too"
            ),
            "{input:?}"
        );
        a.options.deep.each_match = true;
        a.options.deep.documents = true;
        let input = row(&render(&mut a, 120, 24), 23);
        assert!(
            input.contains("type a pattern · every match with the text around it, in binaries too"),
            "{input:?}"
        );
        assert!(
            input.contains("each match · documents · ignored"),
            "{input:?}"
        );
        let top = row(&render(&mut a, 160, 24), 0);
        assert!(
            top.contains("Deep") && top.contains("^T in file") && top.contains("^U/^G diff"),
            "{top:?}"
        );
    }

    #[test]
    fn the_result_filter_is_drawn_after_the_pattern() {
        let (mut a, now) = app(Tab::Content);
        a.start_query("needle".into());
        feed(&mut a, &["src/main.c", "docs/b.md"]);
        let input = row(&render(&mut a, 120, 24), 23);
        assert!(
            input.contains("Content › needle · filter: type words · the rows with all of them"),
            "{input:?}"
        );
        for c in "main".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        let buf = render(&mut a, 120, 24);
        assert!(
            row(&buf, 23).contains("Content › needle · filter: main"),
            "{:?}",
            row(&buf, 23)
        );
        assert_eq!(
            fg_at(&buf, "src/main.c", 4),
            theme::RED,
            "the filter's words are red"
        );
    }

    #[test]
    fn the_keys_name_options_and_the_filter() {
        let (mut a, _) = app(Tab::Content);
        let top = row(&render(&mut a, 200, 24), 0);
        assert!(
            top.contains("Enter open · ^S options · ^F filter · ^O nvim"),
            "{top:?}"
        );
        let (mut a, _) = app(Tab::Folders);
        let top = row(&render(&mut a, 200, 24), 0);
        assert!(top.contains("^S options") && !top.contains("^F"), "{top:?}");
    }

    #[test]
    fn esc_is_named_back_where_it_goes_back() {
        let (mut a, now) = app(Tab::Content);
        a.start_query("needle".into());
        let top = row(&render(&mut a, 200, 24), 0);
        assert!(
            top.contains("Esc back") && !top.contains("Esc quit"),
            "{top:?}"
        );
        a.handle_key(key(KeyCode::Esc), now);
        let top = row(&render(&mut a, 200, 24), 0);
        assert!(top.contains("Esc quit"), "{top:?}");
        feed(&mut a, &["src/a.c"]);
        a.handle_key(ctrl('t'), now);
        let top = row(&render(&mut a, 200, 24), 0);
        assert!(top.contains("Esc back"), "inside a file: {top:?}");
    }

    #[test]
    fn input_shows_active_options_and_count() {
        let (mut a, _) = app(Tab::Content);
        a.options.content.archives = true;
        feed(&mut a, &["a", "b"]);
        a.loading = false;
        let buf = render(&mut a, 100, 24);
        let input = row(&buf, 23);
        assert!(
            input
                .trim_end()
                .ends_with("archives & documents · ignored   2/2"),
            "{input:?}"
        );
    }

    #[test]
    fn input_shows_the_engine_options_of_its_tab() {
        let (mut a, _) = app(Tab::Files);
        a.extra = Some((Tab::Files, vec!["-e".into(), "rs".into()]));
        feed(&mut a, &["a.rs"]);
        a.loading = false;
        let buf = render(&mut a, 100, 24);
        let input = row(&buf, 23);
        assert!(
            input.trim_end().ends_with("ignored · -- -e rs   1/1"),
            "{input:?}"
        );
    }

    #[test]
    fn keys_sit_in_the_middle_of_the_tab_bar() {
        let cases = [
            (
                Tab::Files,
                "Enter open · ^S options · ^O nvim · ^T in file · ^U/^G diff · Alt-U log · ^E reveal · ^C copy · Esc quit",
            ),
            (
                Tab::Folders,
                "Enter go inside · ^S options · ^E reveal · ^C copy · Esc quit",
            ),
            (
                Tab::Git,
                "Enter show commit · ^S options · ^C copy hash · Esc quit",
            ),
        ];
        for (tab, want) in cases {
            let (mut a, _) = app(tab);
            let buf = render(&mut a, 200, 24);
            let top: Vec<String> = (0..200).map(|x| buf[(x, 0)].symbol().to_string()).collect();
            let text = top.concat();
            let at = text
                .find(want)
                .unwrap_or_else(|| panic!("{tab:?}: {text:?}"));
            let start = text[..at].chars().count() as u16;
            let end = start + want.chars().count() as u16;
            let left = start - a.layout.tabs.last().unwrap().1.right();
            let right = a.layout.config.x - end;
            assert!(left.abs_diff(right) <= 1, "{tab:?}: {left} vs {right}");
            assert!(!row(&buf, 23).contains("Esc quit"), "no footer any more");
        }
    }

    fn open_diff_view(a: &mut App, lines: usize) {
        let now = std::time::Instant::now();
        a.handle_key(ctrl('g'), now);
        if lines > 0 {
            let text: Vec<Line> = (0..lines)
                .map(|i| Line::from(format!("line {i}")))
                .collect();
            a.on_view(crate::preview::PreviewMsg {
                generation: a.view_gen,
                text: Text::from(text),
                header_lines: 0,
            });
        }
    }

    #[test]
    fn a_view_covers_the_tabs_with_its_title_text_and_keys() {
        let (mut a, _) = app(Tab::Files);
        feed(&mut a, &["src/a.c"]);
        open_diff_view(&mut a, 50);
        let buf = render(&mut a, 80, 12);
        assert!(
            row(&buf, 0).contains("a.c · changes against HEAD"),
            "{:?}",
            row(&buf, 0)
        );
        assert!(!row(&buf, 0).contains("Content"), "no tab bar");
        assert!(row(&buf, 1).contains("line 0"), "{:?}", row(&buf, 1));
        let last = row(&buf, 11);
        assert!(
            last.contains("Esc back") && last.trim_end().ends_with("1/50"),
            "{last:?}"
        );
        assert_eq!(a.layout.view, Rect::new(1, 1, 78, 9));
    }

    #[test]
    fn a_view_shows_the_search_while_it_is_typed() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["src/a.c"]);
        open_diff_view(&mut a, 50);
        a.handle_key(key(KeyCode::Char('/')), now);
        a.handle_key(key(KeyCode::Char('x')), now);
        let buf = render(&mut a, 80, 12);
        assert!(row(&buf, 11).starts_with("/ x"), "{:?}", row(&buf, 11));
    }

    #[test]
    fn a_view_says_while_it_is_made() {
        let (mut a, _) = app(Tab::Files);
        feed(&mut a, &["src/a.c"]);
        open_diff_view(&mut a, 0);
        let buf = render(&mut a, 80, 12);
        assert!(row(&buf, 1).contains("…"), "{:?}", row(&buf, 1));
    }

    #[test]
    fn the_input_names_the_file_searched_in() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["src/a.c"]);
        a.handle_key(ctrl('t'), now);
        let buf = render(&mut a, 100, 24);
        let input = row(&buf, 23);
        assert!(
            input.trim_start().starts_with("In a.c › type words"),
            "{input:?}"
        );
        assert!(
            !input.contains("ignored"),
            "no Content option applies in a file: {input:?}"
        );
    }

    #[test]
    fn ctrl_o_names_the_editor() {
        let (mut a, _) = app(Tab::Files);
        a.editor = "nano".into();
        let buf = render(&mut a, 200, 24);
        assert!(row(&buf, 0).contains("^O nano"), "{:?}", row(&buf, 0));
    }

    /// The cells of column `x` between rows `from` and `to`.
    fn column(buf: &Buffer, x: u16, from: u16, to: u16) -> String {
        (from..to).map(|y| buf[(x, y)].symbol()).collect()
    }

    #[test]
    fn long_list_has_a_scrollbar() {
        let (mut a, _) = app(Tab::Files);
        let names: Vec<String> = (0..100).map(|i| format!("f{i:03}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        render(&mut a, 80, 24);
        let buf = render(&mut a, 80, 24);
        let l = a.layout.list;
        assert!(
            column(&buf, l.right(), l.y, l.bottom()).contains('┃'),
            "{:?}",
            column(&buf, l.right(), l.y, l.bottom())
        );
    }

    #[test]
    fn short_list_has_no_scrollbar() {
        let (mut a, _) = app(Tab::Files);
        feed(&mut a, &["a", "b", "c"]);
        let buf = render(&mut a, 80, 24);
        let l = a.layout.list;
        assert!(!column(&buf, l.right(), l.y, l.bottom()).contains('┃'));
    }

    #[test]
    fn long_preview_has_a_scrollbar() {
        let (mut a, _) = app_with_preview(Tab::Files);
        feed(&mut a, &["a"]);
        a.on_preview(crate::preview::PreviewMsg {
            header_lines: 0,
            generation: a.preview_gen,
            text: Text::raw("x\n".repeat(200)),
        });
        render(&mut a, 80, 24);
        let buf = render(&mut a, 80, 24);
        let p = a.layout.preview;
        assert!(column(&buf, p.right(), p.y, p.bottom()).contains('┃'));
    }

    #[test]
    fn selected_row_has_a_band() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["src/a.rs", "src/b.rs"]);
        a.handle_key(key(KeyCode::Up), now);
        let buf = render(&mut a, 80, 24);
        let (x, y) = find(&buf, "src/b.rs").unwrap();
        assert_eq!(buf[(x, y)].bg, theme::SURFACE);
        assert_eq!(buf[(x - 1, y)].symbol(), "▌");
        assert_eq!(buf[(78, y)].bg, theme::SURFACE, "the band spans the row");
        let (x, y) = find(&buf, "src/a.rs").unwrap();
        assert_ne!(buf[(x, y)].bg, theme::SURFACE);
    }

    #[test]
    fn band_covers_coloured_rows_too() {
        let (mut a, _) = app(Tab::Git);
        let rows = crate::app::tests::rows(
            crate::commands::ParseKind::Git,
            &["* \x1b[1;34mabc1234\x1b[m T\x1fabc1234"],
        );
        a.on_list(crate::runner::ListMsg::Items(a.list_gen, rows));
        let buf = render(&mut a, 80, 24);
        let (x, y) = find(&buf, "abc1234").unwrap();
        assert_eq!(buf[(x, y)].bg, theme::SURFACE);
        assert_eq!(buf[(x + 8, y)].bg, theme::SURFACE, "after the reset");
    }

    fn fg_at(buf: &Buffer, text: &str, offset: u16) -> ratatui::style::Color {
        let (x, y) = find(buf, text).unwrap();
        buf[(x + offset, y)].fg
    }

    #[test]
    fn typed_words_are_highlighted() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["src/Main.rs", "docs/other.md"]);
        for c in "main s".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        let buf = render(&mut a, 80, 24);
        assert_eq!(fg_at(&buf, "src/Main.rs", 4), theme::RED, "M of Main");
        assert_eq!(fg_at(&buf, "src/Main.rs", 7), theme::RED, "n of Main");
        assert_eq!(fg_at(&buf, "src/Main.rs", 0), theme::RED, "s of src");
        assert_eq!(
            fg_at(&buf, "src/Main.rs", 1),
            theme::BLUE,
            "r of src keeps the folder colour"
        );
        assert_eq!(fg_at(&buf, "src/Main.rs", 10), theme::RED, "s of .rs");
        let (x, y) = find(&buf, "src/Main.rs").unwrap();
        assert_eq!(
            buf[(x + 4, y)].bg,
            theme::SURFACE,
            "the band stays under a highlight"
        );
    }

    #[test]
    fn search_pattern_is_not_drawn_into_file_names() {
        for tab in [Tab::Content, Tab::Deep] {
            let (mut a, now) = app(tab);
            a.handle_key(key(KeyCode::Char('m')), now);
            a.tick(now + std::time::Duration::from_secs(1));
            feed(&mut a, &["main.c"]);
            let buf = render(&mut a, 80, 24);
            assert_ne!(fg_at(&buf, "main.c", 0), theme::RED, "{tab:?}");
        }
    }

    #[test]
    fn list_scrolls_to_keep_the_selection_visible() {
        let (mut a, now) = app(Tab::Files);
        let names: Vec<String> = (0..100).map(|i| format!("f{i:03}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        render(&mut a, 80, 24);
        a.handle_key(key(KeyCode::Home), now);
        let buf = render(&mut a, 80, 24);
        assert!(find(&buf, "f099").is_some());
        assert!(find(&buf, "f000").is_none());
    }

    #[test]
    fn notes_come_before_rows() {
        let (mut a, _) = app(Tab::Files);
        a.on_list(crate::runner::ListMsg::Error(a.list_gen, "fd: boom".into()));
        feed(&mut a, &["a.txt"]);
        let buf = render(&mut a, 80, 24);
        let (_, note_y) = find(&buf, "fd: boom").unwrap();
        let (_, row_y) = find(&buf, "a.txt").unwrap();
        // bottom up, as in fzf: the note next to the input line, the first row above it
        assert_eq!(note_y, a.layout.list.bottom() - 1);
        assert_eq!(row_y, note_y - 1);
    }

    #[test]
    fn the_first_row_is_at_the_bottom() {
        let (mut a, _) = app(Tab::Files);
        feed(&mut a, &["first.txt", "second.txt"]);
        let buf = render(&mut a, 80, 24);
        let (_, first) = find(&buf, "first.txt").unwrap();
        let (_, second) = find(&buf, "second.txt").unwrap();
        assert_eq!(first, a.layout.list.bottom() - 1);
        assert_eq!(second, first - 1);
        assert_eq!(
            buf[(1, first)].symbol(),
            "▌",
            "the selection starts at the bottom"
        );
    }

    #[test]
    fn scrollbar_thumb_is_at_the_bottom_at_the_start() {
        let (mut a, _) = app(Tab::Files);
        let names: Vec<String> = (0..100).map(|i| format!("f{i:03}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        render(&mut a, 80, 24);
        let buf = render(&mut a, 80, 24);
        let l = a.layout.list;
        let col = column(&buf, l.right(), l.y, l.bottom());
        assert!(col.ends_with('┃') && !col.starts_with('┃'), "{col:?}");
    }

    #[test]
    fn preview_border_has_root_and_status() {
        let (mut a, now) = app_with_preview(Tab::Files);
        a.root = "/r/project".into();
        a.set_status("Copied to the clipboard", false, now);
        let buf = render(&mut a, 80, 24);
        let (x, y) = find(&buf, "/r/project").unwrap();
        assert_eq!(y, 1);
        assert_eq!(buf[(x, y)].fg, theme::YELLOW, "the slashes yellow");
        assert_eq!(buf[(x + 1, y)].fg, theme::BLUE, "the folders blue");
        assert_eq!(buf[(x + 3, y)].fg, theme::GREEN, "the name green");
        let (x, y) = find(&buf, "Copied to the clipboard").unwrap();
        assert_eq!(buf[(x, y)].fg, theme::GREEN);
        assert_eq!(y, a.layout.preview.y + a.layout.preview.height);
    }

    #[test]
    fn hidden_preview_gives_the_list_the_room() {
        let (mut a, now) = app_with_preview(Tab::Files);
        a.handle_key(ctrl('l'), now);
        render(&mut a, 80, 24);
        assert_eq!(a.layout.preview, Rect::default());
        assert_eq!(a.layout.list.height, 24 - 1 - 1 - 2);
    }

    #[test]
    fn without_a_preview_the_list_border_names_the_root() {
        let (mut a, _) = app(Tab::Files); // Files starts without its preview
        feed(&mut a, &["a"]);
        let buf = render(&mut a, 80, 24);
        assert!(row(&buf, 1).contains("/r/p"), "{:?}", row(&buf, 1));
    }

    #[test]
    fn layout_rects_recorded() {
        let (mut a, _) = app_with_preview(Tab::Files);
        let buf = render(&mut a, 120, 30);
        let l = &a.layout;
        assert_eq!(l.tabs.len(), 5);
        for (tab, r) in &l.tabs {
            assert_eq!(r.y, 0);
            let text: String = (r.x..r.x + r.width).map(|x| buf[(x, 0)].symbol()).collect();
            assert_eq!(text.trim(), tab.title());
        }
        assert_eq!(l.config.x + l.config.width, 120);
        assert!(l.preview.height > 0 && l.list.height > 0);
        assert!(
            l.preview.y + l.preview.height < l.list.y,
            "preview above the list, borders between"
        );
        assert_eq!(l.list.x, 1);
        assert_eq!(l.popup, Rect::default());
        assert!(l.popup_rows.is_empty());
    }

    #[test]
    fn tiny_terminals_do_not_panic() {
        for (w, h) in [(1, 1), (5, 2), (10, 3), (20, 5), (30, 8)] {
            let (mut a, now) = app(Tab::Content);
            feed(&mut a, &["a", "b"]);
            a.on_list(crate::runner::ListMsg::Error(a.list_gen, "e".into()));
            render(&mut a, w, h);
            a.handle_key(ctrl('s'), now);
            render(&mut a, w, h);
        }
    }

    #[test]
    fn cursor_follows_the_query() {
        let (mut a, now) = app(Tab::Files);
        a.handle_key(key(KeyCode::Char('a')), now);
        let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
        t.draw(|f| draw(f, &mut a)).unwrap();
        let pos = t.get_cursor_position().unwrap();
        let input = row(t.backend().buffer(), 23);
        assert_eq!(pos.y, 23);
        assert_eq!(input.chars().nth(pos.x as usize - 1), Some('a'));
        let _ = Instant::now();
    }
}
