//! Drawing. Top to bottom: the tab bar (the keys for the selected item in its middle), the preview,
//! the list (filled from the bottom, as fzf) and the input line; the
//! Config popup over them. Records where everything went in `app.layout` for the mouse.

use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

use crate::app::{App, Find, PopupRow, ScreenLayout};
use crate::filter::{Kind, Query, Term};
use crate::model::{Opt, Tab};
use crate::preview::Found;
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
    if app.tab == Tab::Help {
        let body = Rect::new(area.x, preview.y, area.width, preview.height + list.height);
        layout.help = draw_help(frame, app, body);
        draw_input(frame, app, input);
        app.layout = layout;
        return;
    }
    let status = status_line(app);
    if app.preview_visible() {
        (layout.preview, layout.preview_bar) = draw_preview(frame, app, preview, status.clone());
    }
    (layout.list, layout.list_bar) = draw_list(
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
/// The scroll bar on `area`'s right border: a full block in the accent colour for the thumb (Bilal:
/// the thin one was hard to see); returns its track, where the mouse scrolls, or an empty rect.
fn draw_scrollbar(
    frame: &mut Frame,
    area: Rect,
    total: usize,
    position: usize,
    visible: usize,
) -> Rect {
    if total <= visible || area.height < 3 {
        return Rect::default();
    }
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .track_style(style(theme::FAINT))
        .thumb_symbol("█")
        .thumb_style(style(theme::MAUVE));
    let mut state = ScrollbarState::new(total - visible + 1)
        .position(position)
        .viewport_content_length(visible);
    let track = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    frame.render_stateful_widget(bar, track, &mut state);
    Rect::new(track.right().saturating_sub(1), track.y, 1, track.height)
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

/// The Help tab's manual in `area`, scrolled, the found line in a band; returns its text area.
fn draw_help(frame: &mut Frame, app: &App, area: Rect) -> Rect {
    let h = &app.help;
    let block = bordered().title_top(Span::styled(" Manual ", style(theme::TEXT)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let total = h.text.lines.len();
    let from = h.scroll.min(total.saturating_sub(inner.height as usize));
    let mut shown: Vec<Line> = h.text.lines[from..]
        .iter()
        .take(inner.height as usize)
        .cloned()
        .collect();
    if let Some(i) = h
        .found
        .and_then(|f| f.checked_sub(from))
        .filter(|&i| i < shown.len())
    {
        shown[i] = shown[i]
            .clone()
            .patch_style(Style::new().bg(theme::SURFACE));
    }
    let text_area = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(1),
        inner.height,
    );
    frame.render_widget(Paragraph::new(Text::from(shown)), text_area);
    draw_scrollbar(frame, area, total, from, inner.height as usize);
    inner
}

/// The preview; returns its text area and its scroll bar's track.
fn draw_preview(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    status: Option<Line<'static>>,
) -> (Rect, Rect) {
    let mut block = bordered().title_top(root_title(app));
    // at the top right, so they stay while the text scrolls: Content's and Deep's pattern with its count,
    // then Ctrl-F's (the right titles line up from the right, the last one rightmost)
    if let Some(found) = app.preview_found.as_ref().filter(|_| app.tab.searches()) {
        block = block.title_top(found_title(found, area.width / 2).right_aligned());
    }
    if let Some(found) = app
        .preview_find
        .as_ref()
        .and_then(|f| find_title(f, area.width / 2))
    {
        // drawn after the root, over its end when the border is short: it stays whole
        block = block.title_top(found.right_aligned());
    }
    if let Some(status) = status {
        block = block.title_bottom(status);
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Only the lines that can be on screen: a preview can have hundreds of thousands.
    let from = (app.preview_scroll as usize).min(app.preview.lines.len());
    let mut shown: Vec<Line> = app.preview.lines[from..]
        .iter()
        .take(inner.height as usize)
        .cloned()
        .collect();
    // Ctrl-F: every match on the lines shown coloured, over whatever colours their parts have
    if let Some(f) = &app.preview_find {
        let first = f.hits.partition_point(|(l, _)| *l < from);
        for (k, (l, _)) in f.hits.iter().enumerate().skip(first) {
            if *l >= from + shown.len() {
                break;
            }
            if k > first && f.hits[k - 1].0 == *l {
                continue; // the line is marked already
            }
            let marks: Vec<(Range<usize>, bool)> = f.hits[k..]
                .iter()
                .enumerate()
                .take_while(|(_, (m, _))| m == l)
                .map(|(j, (_, r))| (r.clone(), f.current == Some(k + j)))
                .collect();
            let line = std::mem::take(&mut shown[l - from]);
            shown[l - from] = mark_matches(line, &marks);
        }
    }
    frame.render_widget(
        Paragraph::new(Text::from(shown)).wrap(Wrap { trim: false }),
        inner,
    );
    let bar = draw_scrollbar(
        frame,
        area,
        app.preview.lines.len(),
        from,
        inner.height as usize,
    );
    (inner, bar)
}

/// Ctrl-F, at the preview's top right: what is looked for as typed, and which match of how many, or
/// that it is nowhere or does not parse; the text cut to fit `max` columns. None while nothing is typed.
fn find_title(f: &Find, max: u16) -> Option<Line<'static>> {
    if f.text.is_empty() {
        return None;
    }
    let (said, said_style) = match f.current {
        _ if f.bad => ("bad regex".to_string(), style(theme::RED)),
        _ if f.slow => ("too slow".to_string(), style(theme::RED)),
        None => ("not found".to_string(), style(theme::RED)),
        Some(c) => (format!("{}/{}", c + 1, f.hits.len()), style(theme::SUB)),
    };
    Some(titled(&f.text, said, said_style, max))
}

/// Content's and Deep's pattern and how often it occurs in the previewed file, at its top right.
fn found_title(f: &Found, max: u16) -> Line<'static> {
    let count = match f.count {
        Some(1) => "1 match".to_string(),
        Some(n) => format!("{n} matches"),
        None => "? matches".to_string(),
    };
    titled(&f.pattern, count, style(theme::PEACH), max)
}

/// ` TEXT · SAID ` for a border: the text as typed (printable, red), cut with "…" to fit `max` columns.
fn titled(text: &str, said: String, said_style: Style, max: u16) -> Line<'static> {
    let mut text = crate::items::printable(text);
    let room = usize::from(max).saturating_sub(said.chars().count() + 5);
    if text.chars().count() > room {
        text = text
            .chars()
            .take(room.saturating_sub(1))
            .collect::<String>()
            + "…";
    }
    Line::from(vec![
        Span::raw(" "),
        Span::styled(text, style(theme::RED)),
        Span::styled(" · ", style(theme::FAINT)),
        Span::styled(said, said_style),
        Span::raw(" "),
    ])
}

/// Ctrl-F's matches coloured in `line`, given as the bytes of its text and whether it is the current
/// one: every one red as in the list, the current one bold and underlined; no background, so the
/// line keeps its own (delta's diff colours). A part a match crosses is split there.
fn mark_matches(line: Line<'static>, marks: &[(Range<usize>, bool)]) -> Line<'static> {
    let other = Style::new().fg(theme::RED);
    let current = Style::new()
        .fg(theme::RED)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let mut spans = Vec::with_capacity(line.spans.len() + 2 * marks.len());
    let mut start = 0; // of the part in the line's text
    for span in line.spans {
        let text = span.content.as_ref();
        let end = start + text.len();
        let mut at = start; // what of the part is out
        for (r, is_current) in marks {
            let (from, to) = (r.start.max(at), r.end.min(end));
            if from >= to {
                continue;
            }
            if from > at {
                spans.push(Span::styled(
                    text[at - start..from - start].to_string(),
                    span.style,
                ));
            }
            let mark = if *is_current { current } else { other };
            spans.push(Span::styled(
                text[from - start..to - start].to_string(),
                span.style.patch(mark),
            ));
            at = to;
        }
        if at == start {
            spans.push(span); // no match in it
        } else if at < end {
            spans.push(Span::styled(text[at - start..].to_string(), span.style));
        }
        start = end;
    }
    Line { spans, ..line }
}

/// The list; returns its rows' area and its scroll bar's track.
fn draw_list(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    status: Option<Line<'static>>,
) -> (Rect, Rect) {
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
    let match_case = app.options.get(app.tab, Opt::MatchCase);
    let typed = match &app.result_filter {
        Some(words) => words.as_str(),
        None if app.tab.searches() => "",
        None => &app.query,
    };
    let query = Query::parse(typed, match_case);
    let terms: Vec<Term> = query.shown_terms().cloned().collect();
    // a list of matches: the words found in a row's path red there too (ugrep marks the line's)
    let keeper = app.row_words().map(|w| w.keeper());
    // searching versions: each version's name (its tag, else its date) beside its hash
    let version_names: std::collections::HashMap<&str, &str> = app
        .versions
        .iter()
        .flat_map(|v| v.list.iter())
        .map(|r| (r.hash.as_str(), r.name.as_str()))
        .collect();
    // searching versions: git grep marks nothing, so the pattern is found again in each row shown
    let versions_finder = app.versions.as_ref().and_then(|v| {
        let o = &app.options.versions;
        let opts = crate::model::FindOpts {
            match_case: o.match_case,
            smart_case: o.smart_case,
            whole_word: o.whole_word,
            regex: o.regex,
        };
        crate::find::Finder::new(&v.query, &opts).ok().flatten()
    });
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
            .chain(
                named(
                    highlight(
                        in_version(
                            in_path(item, keeper.as_ref()),
                            item,
                            versions_finder.as_ref(),
                        ),
                        &terms,
                        match_case,
                    ),
                    item,
                    &version_names,
                )
                .spans,
            )
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
    let bar = draw_scrollbar(frame, area, total, from_top, visible);
    (inner, bar)
}

/// A versions row with its version's name after the short hash (Bilal: the hash alone does not say
/// which version it is); after the marks, whose places count from the row's start.
fn named(
    mut line: Line<'static>,
    item: &crate::items::Item,
    names: &std::collections::HashMap<&str, &str>,
) -> Line<'static> {
    if let Some(name) = item.version().and_then(|(rev, _)| names.get(rev)) {
        let at = 1.min(line.spans.len());
        line.spans
            .insert(at, Span::styled(format!(" {name}"), style(theme::YELLOW)));
    }
    line
}

/// A versions row with what `finder` finds in its text red (not in its version, path or number).
fn in_version(
    line: Line<'static>,
    item: &crate::items::Item,
    finder: Option<&crate::find::Finder>,
) -> Line<'static> {
    let (Some(f), Some((at, text))) = (finder, item.version_text()) else {
        return line;
    };
    let marks: Vec<(Range<usize>, bool)> = f
        .matches(&text)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.start + at..r.end + at, false))
        .collect();
    mark_matches(line, &marks)
}

/// `item`'s row with `keeper`'s words red where they are in its path (a matching line's row starts with it).
fn in_path(item: &crate::items::Item, keeper: Option<&crate::find::Keeper>) -> Line<'static> {
    let line = item.display();
    match (keeper, item.match_parts()) {
        (Some(k), Some((path, _))) => {
            let marks: Vec<(Range<usize>, bool)> =
                k.marks(&path).into_iter().map(|r| (r, false)).collect();
            mark_matches(line, &marks)
        }
        _ => line,
    }
}

/// `line` with the filter's terms red and bold where they are found (every occurrence, or at the start
/// or end for an anchored one): in any case (the terms in lower case), or (`match_case`) as typed.
fn highlight(line: Line<'static>, terms: &[Term], match_case: bool) -> Line<'static> {
    if terms.is_empty() {
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
        if match_case {
            lower.push(c);
            from.push(k);
            continue;
        }
        for l in c.to_lowercase() {
            lower.push(l);
            from.push(k);
        }
    }
    let mut hit = vec![false; chars.len()];
    // where an anchored term may start and end: past the spaces at that end, as the filter reads it
    let first = lower.iter().take_while(|c| c.is_whitespace()).count();
    let last = lower.len() - lower.iter().rev().take_while(|c| c.is_whitespace()).count();
    for term in terms {
        let w: Vec<char> = term.text.chars().collect();
        if w.is_empty() || w.len() > lower.len() {
            continue;
        }
        let at_start = |start: usize| start == if w[0].is_whitespace() { 0 } else { first };
        let at_end = |start: usize| {
            start + w.len()
                == if w[w.len() - 1].is_whitespace() {
                    lower.len()
                } else {
                    last
                }
        };
        for start in 0..=lower.len() - w.len() {
            let placed = match term.kind {
                Kind::Anywhere => true,
                Kind::Start => at_start(start),
                Kind::End => at_end(start),
                Kind::Whole => at_start(start) && at_end(start),
            };
            if placed && lower[start..start + w.len()] == w[..] {
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
        Opt::WordsOnALine => "words on a line",
        Opt::WordsInTheFile => "words in the file",
        Opt::MatchCase => "match case",
        Opt::WholeFiles => "whole files",
        Opt::AllBranches => "all branches",
        Opt::WholeWord => "whole word",
        Opt::Regex => "regex",
        Opt::Binary => "binary",
        Opt::SmartCase => "smart case",
    }
}

fn placeholder(app: &App) -> &'static str {
    if app.preview_find.is_some() {
        return "type to find · Enter ↓ the next · ↑ the one before · ^S options · Esc back";
    }
    if app.versions.is_some() {
        return "type a pattern to search every version · listed: each file's every change · ^S options";
    }
    if app.scope.is_some() {
        let f = &app.options.in_file;
        return if f.words_on_a_line {
            "type words · the lines with all of them"
        } else if f.words_in_the_file {
            "type words · all of them in the file, the lines with any"
        } else {
            "type words · the lines with any of them"
        };
    }
    match app.tab {
        Tab::Files => "type to filter file names",
        Tab::Content if app.options.content.matching_lines => {
            "type words · each on its line or path"
        }
        Tab::Content if app.options.content.each_match => {
            "type words · every match · each on its line or path"
        }
        Tab::Content => "type a pattern · lists the files that contain it",
        Tab::Deep if app.options.deep.each_match => {
            "type words · every match, binaries too · each on its line or path"
        }
        Tab::Deep => "type a pattern · the files that contain it, binaries and archives too",
        Tab::Folders => "type to filter folders · Enter goes inside, .. goes up",
        Tab::Git => "type to filter commits · Enter shows one",
        Tab::Help => "type to find in the manual · Enter the next one",
    }
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let options = match app.scope {
        _ if app.preview_find.is_some() => app.options.find_list(),
        _ if app.versions.is_some() => app.options.versions_list(),
        Some(_) => app.options.in_file_list(), // inside one file, its own
        None => app.options.list(app.tab),
    };
    let finding = app.preview_find.is_some();
    // a pattern is a regular expression unless said otherwise: only "plain text" is named
    let mut active: Vec<String> = options
        .into_iter()
        .filter_map(|(o, on)| match (o, on) {
            (Opt::Regex, true) if !finding => None,
            (Opt::Regex, false) if !finding => Some("plain text".to_string()),
            (o, true) => Some(short(o).to_string()),
            _ => None,
        })
        .collect();
    match app.types_off_count() {
        0 => {}
        1 => active.push("1 type off".into()),
        n => active.push(format!("{n} types off")),
    }
    // Ctrl-P: the files searched in, when not all
    match app
        .chosen_of(app.tab)
        .filter(|_| app.scope.is_none() && app.tab.searches())
    {
        Some(c) if c.on.len() == 1 => active.push("1 file chosen".into()),
        Some(c) => active.push(format!("{} files chosen", c.on.len())),
        None => {}
    }
    let extra = app.extra_options();
    if !extra.is_empty() {
        let words: Vec<_> = extra.iter().map(|a| a.to_string_lossy()).collect();
        active.push(crate::items::printable(&format!("-- {}", words.join(" "))));
    }
    let mut count = format!("{}/{}", app.filter.len(), app.filter.total());
    if app.loading {
        count.push_str(" …");
    }
    let mut count_style = style(theme::DIM);
    if app.preview_find.is_some() {
        count.clear(); // Ctrl-F's count is at the preview's top right (`find_title`)
    } else if app.tab == Tab::Help {
        // the manual: where it is, or that the search is nowhere in it
        let h = &app.help;
        if !h.search.is_empty() && h.found.is_none() {
            (count, count_style) = ("not found".into(), style(theme::RED));
        } else {
            count = format!("{}/{}", h.scroll + 1, h.text.lines.len());
        }
    }
    let gap = if count.is_empty() { "" } else { "   " };
    let right = Line::from(vec![
        Span::styled(active.join(" · "), style(theme::SUB)),
        Span::raw(gap),
        Span::styled(count, count_style),
        Span::raw(" "),
    ]);
    let right_w = (right.width() as u16).min(area.width / 2);
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w)]).areas(area);

    let title = match &app.scope {
        _ if app.preview_find.is_some() => "Find in preview".to_string(),
        _ if app.versions.is_some() => "Search in versions".to_string(),
        Some(scope) => {
            let name = std::path::Path::new(&scope.key)
                .file_name()
                .unwrap_or(&scope.key);
            format!("In {}", crate::items::printable(&name.to_string_lossy()))
        }
        None => app.tab.title().to_string(),
    };
    let typed = match (&app.preview_find, &app.versions) {
        (Some(f), _) => &f.text,
        (None, Some(v)) => &v.query,
        (None, None) if app.tab == Tab::Help => &app.help.search,
        (None, None) => &app.query,
    };
    let query = crate::items::printable(typed);
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
    // Ctrl-Alt-F: the pattern stays; the cursor is in the words that filter the rows (not while finding)
    if let Some(words) = app
        .result_filter
        .as_ref()
        .filter(|_| app.preview_find.is_none())
    {
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
            "^Alt-F filter",
            "^P files",
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
            "^F find",
            "^Alt-F search in versions",
            "^C copy hash",
            "Esc quit",
        ],
        Tab::Help => &["↑↓ PgUp PgDn scroll", "Enter next found", "Esc quit"],
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
    if app.versions.is_some() {
        // a row is a file in a version: the commit's keys are not for it
        items = [
            "Enter show file",
            "^S options",
            "^F find",
            "^C copy",
            "Esc quit",
        ]
        .map(String::from)
        .to_vec();
    }
    if app.result_filter.is_some()
        || app.scope.is_some()
        || app.preview_find.is_some()
        || app.versions.is_some()
    {
        // Esc leaves the filter, the file or the finding, not sf
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

/// Columns of the Ctrl-S popup are this far apart.
const COLUMN_GAP: u16 = 3;

/// Where the Ctrl-S popup goes and how its lines are laid out.
#[derive(Debug, PartialEq)]
struct PopupGeometry {
    /// The popup with its frame.
    area: Rect,
    /// Each column's width, as wide as its own lines; the first column leftmost.
    widths: Vec<u16>,
    /// Lines in a column, the last one may have fewer.
    per_column: usize,
    /// The first line shown: what no number of columns holds scrolls with the cursor.
    first: usize,
}

/// A version in the popup: its short hash, date, author, branch and subject (as Bilal asked).
fn version_line(app: &App, hash: &str, check: Span<'static>) -> Line<'static> {
    let row = app
        .versions
        .as_ref()
        .and_then(|v| v.list.iter().find(|r| r.hash == hash));
    let short: String = hash.chars().take(7).collect();
    let mut spans = vec![
        check,
        Span::raw(" "),
        Span::styled(short, style(theme::BLUE).add_modifier(Modifier::BOLD)),
    ];
    if let Some(r) = row {
        let subject: String = r.subject.chars().take(50).collect();
        spans.extend([
            Span::styled(format!("  {}", r.date), style(theme::DIM)),
            Span::styled(format!("  {}", r.author), style(theme::SUB)),
            Span::styled(format!("  {}", r.branch), style(theme::YELLOW)),
            Span::styled(format!("  {subject}"), style(theme::TEXT)),
        ]);
    }
    Line::from(spans)
}

/// The popup hangs under ⚙ Config at the right edge, between the tab bar and the input line. Lines
/// (`widths`: one per line) that one column cannot hold go on in more columns, balanced, the popup
/// growing to the left (the first column leftmost); as many columns as the screen's width holds, and
/// what does not fit even so scrolls so that the cursor's line (`at`) shows. It never spills over.
fn popup_geometry(screen: Rect, widths: &[u16], header_width: u16, at: usize) -> PopupGeometry {
    // the frame (2), a blank line and the help line (2); the tab bar and the input line stay free
    let room = (screen.height.saturating_sub(2 + 4) as usize).max(1);
    let lines = widths.len();
    let mut columns = lines.div_ceil(room).max(1);
    loop {
        let capacity = room * columns;
        let first = if lines > capacity {
            (at + 1).saturating_sub(capacity)
        } else {
            0
        };
        let shown = &widths[first..lines.min(first + capacity)];
        let per_column = shown.len().div_ceil(columns).max(1);
        let column_widths: Vec<u16> = shown
            .chunks(per_column)
            .map(|c| c.iter().copied().max().unwrap_or(0))
            .collect();
        let content = column_widths.iter().sum::<u16>()
            + COLUMN_GAP * column_widths.len().saturating_sub(1) as u16;
        let w = content.max(header_width) + 4;
        if w <= screen.width || columns == 1 {
            let w = w.min(screen.width);
            let h = (per_column as u16 + 4).min(screen.height.saturating_sub(2));
            return PopupGeometry {
                area: Rect::new(screen.right() - w, screen.y + 1, w, h),
                widths: column_widths,
                per_column,
                first,
            };
        }
        columns -= 1; // too wide: fewer columns, the rest scrolls
    }
}

fn draw_popup(frame: &mut Frame, app: &App, cursor: usize, layout: &mut ScreenLayout) {
    let rows = app.popup_rows();
    let check = |on: bool| {
        Span::styled(
            if on { "[x]" } else { "[ ]" },
            style(if on { theme::MAUVE } else { theme::DIM }),
        )
    };
    let type_name = |ty: &Option<String>| match ty {
        Some(ext) => crate::items::printable(&format!(".{ext}")),
        None => "(no extension)".to_string(),
    };
    let name_w = rows
        .iter()
        .filter_map(|r| match r {
            PopupRow::Type(ty, _, _) => Some(type_name(ty).chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    // what is drawn: the rows (with their index) and, above the types, a rule naming them
    let mut lines: Vec<(Option<usize>, Line)> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let line = match row {
            PopupRow::Opt(opt, on) => Line::from(vec![
                check(*on),
                Span::styled(format!(" {}", opt.label(app.tab)), style(theme::TEXT)),
                Span::styled(format!("   {}", opt.hint()), style(theme::DIM)),
            ]),
            PopupRow::AllTypes(on) => {
                lines.push((
                    None,
                    Line::from(Span::styled("── File types ──", style(theme::FAINT))),
                ));
                Line::from(vec![
                    check(*on),
                    Span::styled(" All types", style(theme::TEXT)),
                ])
            }
            PopupRow::Type(ty, n, on) => Line::from(vec![
                check(*on),
                Span::styled(format!(" {:<name_w$}", type_name(ty)), style(theme::TEXT)),
                Span::styled(format!("  {n:>6}"), style(theme::DIM)),
            ]),
            PopupRow::AllVersions(on) => {
                lines.push((
                    None,
                    Line::from(Span::styled("── Versions ──", style(theme::FAINT))),
                ));
                Line::from(vec![
                    check(*on),
                    Span::styled(" All versions", style(theme::TEXT)),
                ])
            }
            PopupRow::Version(hash, on) => version_line(app, hash, check(*on)),
            PopupRow::AllFiles(on) => Line::from(vec![
                check(*on),
                Span::styled(" All files", style(theme::TEXT)),
            ]),
            PopupRow::File(path, on) => {
                let mut spans = vec![check(*on), Span::raw(" ")];
                spans.extend(crate::items::path_spans(&crate::items::printable(
                    &path.to_string_lossy(),
                )));
                Line::from(spans)
            }
        };
        lines.push((Some(i), line));
    }
    let help = if rows.iter().any(|r| matches!(r, PopupRow::AllTypes(_))) {
        "Space / click · a all types · Esc close"
    } else if rows.iter().any(|r| matches!(r, PopupRow::AllVersions(_))) {
        "Space / click · a all versions · Esc close"
    } else if rows.iter().any(|r| matches!(r, PopupRow::AllFiles(_))) {
        "Space / click · a all files · Esc close"
    } else {
        "Space / click · Esc close"
    };
    let title = match app.scope {
        _ if app.popup_files => " Files to search in ".to_string(),
        _ if app.preview_find.is_some() => " Find options ".to_string(),
        _ if app.versions.is_some() => " Search in versions options ".to_string(),
        Some(_) => " In file options ".to_string(),
        None => format!(" {} options ", app.tab.title()),
    };
    let at = lines
        .iter()
        .position(|(i, _)| *i == Some(cursor))
        .unwrap_or(0);
    let widths: Vec<u16> = lines.iter().map(|(_, l)| l.width() as u16).collect();
    let header_w = [help.chars().count(), title.chars().count()]
        .into_iter()
        .max()
        .unwrap_or(0) as u16;
    let g = popup_geometry(frame.area(), &widths, header_w, at);
    let popup = g.area;
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
    // column after column, each from the top; the first column leftmost
    let shown = lines
        .into_iter()
        .skip(g.first)
        .take(g.widths.len() * g.per_column);
    for (n, (index, line)) in shown.enumerate() {
        let (column, row) = (n / g.per_column, n % g.per_column);
        let left: u16 = g.widths[..column].iter().map(|w| w + COLUMN_GAP).sum();
        let (x, y) = (inner.x + 1 + left, inner.y + row as u16);
        let width = g.widths[column];
        let cell = Rect::new(x, y, width, 1).intersection(inner);
        if cell.is_empty() {
            continue;
        }
        let band = Rect::new(x - 1, y, width + 2, 1).intersection(inner);
        if index == Some(cursor) {
            frame
                .buffer_mut()
                .set_style(band, Style::new().bg(theme::SURFACE));
        }
        frame.render_widget(Paragraph::new(line), cell);
        if let Some(i) = index {
            layout.popup_rows.push((i, band));
        }
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
    use crate::app::tests::{alt, app, app_with_preview, ctrl, feed, key};
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
        assert!(top.contains("Help"), "{top:?}");
        let keys = top[top.find("Help").unwrap() + 4..top.find("⚙").unwrap()]
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
        assert!(find(&buf, "[ ] Whole word").is_some());
        assert!(find(&buf, "[x] Regular expression").is_some());
        assert_eq!(a.layout.popup_rows.len(), 11);
        assert_eq!(
            a.layout.popup_rows[0].0, 0,
            "Matching lines, the popup's first row"
        );
        let (_, y) = find(&buf, "[ ] Matching lines").unwrap();
        assert_eq!(a.layout.popup_rows[0].1.y, y);
    }

    #[test]
    fn popup_shows_the_file_types_under_the_options() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a.rs", "b.rs", "Makefile"]);
        a.handle_key(ctrl('s'), now);
        let buf = render(&mut a, 80, 24);
        assert!(find(&buf, "── File types ──").is_some());
        assert!(find(&buf, "[x] All types").is_some());
        let rs = row(&buf, find(&buf, "[x] .rs").unwrap().1);
        let after: Vec<&str> = rs
            .split("[x] .rs")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .collect();
        assert_eq!(after.first(), Some(&"2"), "its count: {rs:?}");
        assert!(find(&buf, "[x] (no extension)").is_some());
        assert!(find(&buf, "a all types").is_some());
    }

    #[test]
    fn the_input_line_says_how_many_types_are_off() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["a.rs", "b.md", "c.dll"]);
        a.handle_key(ctrl('s'), now);
        a.popup = Some(4);
        a.handle_key(key(KeyCode::Char(' ')), now);
        a.handle_key(key(KeyCode::Esc), now);
        a.loading = false;
        let input = row(&render(&mut a, 100, 24), 23);
        assert!(
            input.trim_end().ends_with("ignored · 1 type off   2/3"),
            "{input:?}"
        );
        a.handle_key(ctrl('s'), now);
        a.popup = Some(5);
        a.handle_key(key(KeyCode::Char(' ')), now);
        let input = row(&render(&mut a, 100, 24), 23);
        assert!(input.contains("2 types off"), "{input:?}");
    }

    #[test]
    fn the_popup_hangs_under_config_at_the_right_edge() {
        let g = popup_geometry(Rect::new(0, 0, 120, 30), &[40; 5], 20, 0);
        assert_eq!(
            (g.area.right(), g.area.y),
            (120, 1),
            "under the tab bar, at the right edge"
        );
        assert_eq!(
            (g.widths.as_slice(), g.per_column, g.first),
            (&[40][..], 5, 0)
        );
        assert_eq!(
            g.area.height,
            5 + 4,
            "the rows, the frame, a blank and the help line"
        );
    }

    #[test]
    fn a_list_too_long_for_one_column_grows_to_the_left() {
        // 30 high: 24 rows fit a column between the tab bar, the input line and the frame
        let g = popup_geometry(Rect::new(0, 0, 120, 30), &[30; 40], 20, 0);
        assert_eq!(
            (g.widths.as_slice(), g.per_column, g.first),
            (&[30, 30][..], 20, 0)
        );
        assert_eq!(g.area.right(), 120, "still at the right edge");
        assert_eq!(g.area.width, 2 * 30 + COLUMN_GAP + 4);
        assert!(g.area.bottom() <= 29, "the input line stays free");
    }

    #[test]
    fn each_column_is_as_wide_as_its_own_lines() {
        // the options' long lines in the first column, short types after them
        let mut widths = vec![50; 5];
        widths.extend([12; 35]);
        let g = popup_geometry(Rect::new(0, 0, 120, 30), &widths, 20, 0);
        assert_eq!(g.widths, [50, 12]);
        assert_eq!(g.area.width, 50 + 12 + COLUMN_GAP + 4);
    }

    #[test]
    fn what_no_number_of_columns_holds_scrolls_and_nothing_spills() {
        // 80 x 24: 18 rows a column, two 30-wide columns fit, three do not
        let screen = Rect::new(0, 0, 80, 24);
        let g = popup_geometry(screen, &[30; 60], 20, 59);
        assert_eq!((g.widths.len(), g.per_column), (2, 18));
        assert_eq!(g.first, 60 - 36, "the shown lines end at the cursor's");
        assert!(g.area.x >= screen.x && g.area.right() <= screen.right());
        assert!(g.area.bottom() <= 23);
        let g = popup_geometry(Rect::new(0, 0, 30, 10), &[50; 8], 20, 0);
        assert!(
            g.area.width <= 30 && g.area.bottom() <= 9,
            "too small a screen: cut, not spilled"
        );
    }

    #[test]
    fn many_types_take_columns_under_config() {
        let (mut a, now) = app(Tab::Files);
        let names: Vec<String> = (0..40).map(|i| format!("f.t{i:02}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        a.handle_key(ctrl('s'), now);
        let buf = render(&mut a, 160, 30);
        let p = a.layout.popup;
        assert_eq!((p.right(), p.y), (160, 1), "under ⚙ Config");
        let (hx, hy) = find(&buf, "Hidden files").unwrap();
        let (tx, _) = find(&buf, "[x] .t39").unwrap();
        assert_eq!(hy, 2, "the first row right under the frame");
        assert!(
            tx > hx,
            "the list goes on in a column to the right of the first"
        );
        assert!(row(&buf, 29).contains("Files ›"), "the input line stays");
        assert_eq!(
            a.layout.popup_rows.len(),
            a.popup_rows().len(),
            "every row on screen"
        );
    }

    #[test]
    fn a_click_in_the_second_column_turns_that_type_off() {
        use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let (mut a, now) = app(Tab::Files);
        let names: Vec<String> = (0..40).map(|i| format!("f.t{i:02}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        a.handle_key(ctrl('s'), now);
        render(&mut a, 160, 30);
        let first_x = a.layout.popup_rows[0].1.x;
        let &(i, r) = a
            .layout
            .popup_rows
            .iter()
            .find(|(_, r)| r.x > first_x)
            .expect("a second column");
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x + 2,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        };
        a.handle_mouse(click, now);
        assert_eq!(a.popup, Some(i));
        assert!(
            matches!(a.popup_rows()[i], PopupRow::Type(_, _, false)),
            "{:?}",
            a.popup_rows()[i]
        );
        assert_eq!(a.filter.len(), 39);
    }

    #[test]
    fn left_and_right_go_across_the_columns() {
        let (mut a, now) = app(Tab::Files);
        let names: Vec<String> = (0..40).map(|i| format!("f.t{i:02}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        a.handle_key(ctrl('s'), now);
        render(&mut a, 160, 30);
        let at = |a: &App, i: usize| a.layout.popup_rows.iter().find(|(j, _)| *j == i).unwrap().1;
        let from = at(&a, 0);
        a.handle_key(key(KeyCode::Right), now);
        let to = at(&a, a.popup.unwrap());
        assert_eq!(to.y, from.y, "the same line");
        assert!(to.x > from.x, "the next column");
        a.handle_key(key(KeyCode::Left), now);
        assert_eq!(a.popup, Some(0));
        a.handle_key(key(KeyCode::Left), now);
        assert_eq!(a.popup, Some(0), "no column further left");
    }

    #[test]
    fn a_long_list_of_types_scrolls_to_the_cursor() {
        let (mut a, now) = app(Tab::Files);
        let names: Vec<String> = (0..40).map(|i| format!("f.t{i:02}")).collect();
        feed(
            &mut a,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        a.handle_key(ctrl('s'), now);
        let last = a.popup_rows().len() - 1;
        a.popup = Some(last);
        let buf = render(&mut a, 80, 24);
        assert!(
            find(&buf, "[x] .t39").is_some(),
            "the cursor's row is in view"
        );
        assert!(
            find(&buf, "Hidden files").is_none(),
            "the top scrolled away"
        );
        assert!(a.layout.popup_rows.iter().any(|&(i, _)| i == last));
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
            input.contains("type words · every match · each on its line or path"),
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
            input.contains("type words · every match, binaries too · each on its line or path"),
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
            top.contains("Enter open · ^S options · ^Alt-F filter · ^P files · ^O nvim"),
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
    fn the_input_line_names_plain_text_not_the_default_regex() {
        let (mut a, _) = app(Tab::Content);
        let input = row(&render(&mut a, 120, 24), 23);
        assert!(
            !input.contains("regex"),
            "the default goes unsaid: {input:?}"
        );
        a.options.toggle(Tab::Content, Opt::Regex);
        let input = row(&render(&mut a, 120, 24), 23);
        assert!(input.contains("ignored · plain text"), "{input:?}");
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
                "Enter show commit · ^S options · ^F find · ^Alt-F search in versions · ^C copy hash · Esc quit",
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
                found: None,
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
            column(&buf, l.right(), l.y, l.bottom()).contains('█'),
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
        assert!(!column(&buf, l.right(), l.y, l.bottom()).contains('█'));
    }

    #[test]
    fn long_preview_has_a_scrollbar() {
        let (mut a, _) = app_with_preview(Tab::Files);
        feed(&mut a, &["a"]);
        a.on_preview(crate::preview::PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::raw("x\n".repeat(200)),
        });
        render(&mut a, 80, 24);
        let buf = render(&mut a, 80, 24);
        let p = a.layout.preview;
        assert!(column(&buf, p.right(), p.y, p.bottom()).contains('█'));
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
    fn words_found_in_a_path_are_red_there_too() {
        let (mut a, now) = app(Tab::Content);
        a.options.content.matching_lines = true;
        for c in "code interest".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        a.tick(now + std::time::Duration::from_secs(1));
        a.on_list(crate::runner::ListMsg::Items(
            a.list_gen,
            crate::app::tests::rows(
                crate::commands::ParseKind::SearchLines,
                &["docs/CODE.md\x007\x1fthe \x01interest\x02"],
            ),
        ));
        let buf = render(&mut a, 120, 30);
        let row = "docs/CODE.md:7:the interest";
        assert_eq!(fg_at(&buf, row, 5), theme::RED, "C of CODE, in the path");
        assert_eq!(fg_at(&buf, row, 8), theme::RED, "E of CODE");
        assert_ne!(fg_at(&buf, row, 0), theme::RED, "d of docs");
        assert_eq!(
            fg_at(&buf, row, 19),
            theme::RED,
            "i of interest, ugrep's mark"
        );
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
    fn fzf_syntax_marks_only_where_it_found() {
        let (mut a, now) = app(Tab::Files);
        feed(&mut a, &["src/src.rs", "src/app.rs"]);
        for c in "^src .rs$ !app".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        let buf = render(&mut a, 80, 24);
        assert!(find(&buf, "src/app.rs").is_none(), "left out");
        assert_eq!(
            fg_at(&buf, "src/src.rs", 0),
            theme::RED,
            "^src: at the start"
        );
        assert_ne!(
            fg_at(&buf, "src/src.rs", 4),
            theme::RED,
            "not the src after it"
        );
        assert_eq!(fg_at(&buf, "src/src.rs", 7), theme::RED, ".rs$: at the end");
        for c in "\x08\x08\x08\x08 | a | !b".chars() {
            let code = if c == '\x08' {
                KeyCode::Backspace
            } else {
                KeyCode::Char(c)
            };
            a.handle_key(key(code), now);
        }
        // "^src .rs$ | a | !b": the bar and the left-out word are not marked
        let buf = render(&mut a, 80, 24);
        assert_eq!(fg_at(&buf, "src/app.rs", 4), theme::RED, "a of app");
        assert_ne!(fg_at(&buf, "src/app.rs", 3), theme::RED, "/ is no word");
    }

    #[test]
    fn with_match_case_only_the_case_typed_is_red() {
        let (mut a, now) = app(Tab::Files);
        a.options.toggle(Tab::Files, Opt::MatchCase);
        a.handle_key(ctrl('r'), now);
        feed(&mut a, &["src/Main.rs", "src/MAIN.md"]);
        for c in "Main".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        let buf = render(&mut a, 80, 24);
        assert_eq!(fg_at(&buf, "src/Main.rs", 4), theme::RED);
        assert!(
            find(&buf, "src/MAIN.md").is_none(),
            "filtered out by its case"
        );
    }

    #[test]
    fn in_a_file_the_popup_and_the_input_line_name_its_options() {
        let (mut a, now) = app(Tab::Content);
        feed(&mut a, &["src/a.c"]);
        a.handle_key(ctrl('t'), now);
        let input = row(&render(&mut a, 100, 24), 23);
        assert!(input.contains("words on a line"), "{input:?}");
        a.handle_key(ctrl('s'), now);
        let buf = render(&mut a, 100, 24);
        assert!(find(&buf, "In file options").is_some());
        assert!(find(&buf, "[x] All words on a line").is_some());
    }

    #[test]
    fn in_a_file_the_placeholder_says_how_the_words_are_read() {
        let (mut a, now) = app(Tab::Content);
        feed(&mut a, &["src/a.c"]);
        a.handle_key(ctrl('t'), now);
        let says = |a: &mut App| row(&render(a, 120, 24), 23);
        assert!(says(&mut a).contains("type words · the lines with all of them"));
        a.options.toggle_in_file(Opt::WordsInTheFile);
        assert!(says(&mut a).contains("type words · all of them in the file, the lines with any"));
        a.options.toggle_in_file(Opt::WordsInTheFile);
        assert!(says(&mut a).contains("type words · the lines with any of them"));
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
        assert!(col.ends_with('█') && !col.starts_with('█'), "{col:?}");
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
    fn the_help_tab_shows_the_manual() {
        let (mut a, now) = app(Tab::Help);
        let buf = render(&mut a, 120, 30);
        assert!(find(&buf, "sf — the search functions in one window").is_some());
        assert!(
            row(&buf, 29).contains("Help › type to find in the manual"),
            "{:?}",
            row(&buf, 29)
        );
        assert!(a.layout.help.height > 0);
        for c in "ctrl-f".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        let buf = render(&mut a, 120, 30);
        assert!(
            row(&buf, 29).contains("Help › ctrl-f"),
            "{:?}",
            row(&buf, 29)
        );
        let (x, y) = find(&buf, "Ctrl-F").unwrap();
        assert_eq!(buf[(x, y)].bg, theme::SURFACE, "the found line in a band");
        for c in "zzz".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        let buf = render(&mut a, 120, 30);
        assert!(row(&buf, 29).contains("not found"), "{:?}", row(&buf, 29));
    }

    #[test]
    fn ctrl_f_draws_what_it_finds() {
        let (mut a, now) = app_with_preview(Tab::Git);
        feed(&mut a, &["abcdef1"]);
        render(&mut a, 120, 30);
        let lines: Vec<Line> = (0..40)
            .map(|i| {
                Line::raw(if i == 9 {
                    "a Needle here".to_string()
                } else {
                    format!("line {i}")
                })
            })
            .collect();
        a.on_preview(crate::preview::PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::from(lines),
        });
        a.handle_key(ctrl('f'), now);
        let buf = render(&mut a, 120, 30);
        assert!(
            row(&buf, 29).contains("Find in preview › type"),
            "{:?}",
            row(&buf, 29)
        );
        assert!(row(&buf, 0).contains("Esc back"), "{:?}", row(&buf, 0));
        for c in "needle".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        let buf = render(&mut a, 120, 30);
        assert!(
            row(&buf, 29).contains("Find in preview › needle"),
            "{:?}",
            row(&buf, 29)
        );
        let (x, y) = find(&buf, "a Needle here").unwrap();
        assert_ne!(
            buf[(x, y)].bg,
            theme::SURFACE,
            "no band: the word is coloured"
        );
        assert_eq!(buf[(x + 2, y)].fg, theme::RED);
        for c in "zzz".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        assert!(row(&render(&mut a, 120, 30), 1).contains("needlezzz · not found"));
    }

    #[test]
    fn ctrl_f_colours_the_word_over_delta_colours() {
        let (mut a, now) = app_with_preview(Tab::Git);
        feed(&mut a, &["abcdef1"]);
        render(&mut a, 120, 30);
        let green = Style::new().bg(ratatui::style::Color::Rgb(0, 80, 0));
        let lines = vec![
            Line::raw("context"),
            Line::from(vec![Span::styled("+ the new Needle", green)]),
        ];
        a.on_preview(crate::preview::PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::from(lines),
        });
        a.handle_key(ctrl('f'), now);
        for c in "needle".chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        let buf = render(&mut a, 120, 30);
        let (x, y) = find(&buf, "+ the new Needle").unwrap();
        let green = ratatui::style::Color::Rgb(0, 80, 0);
        assert_eq!(buf[(x, y)].bg, green, "no band over delta's own colour");
        let word = &buf[(x + 10, y)];
        assert_eq!(
            (word.fg, word.bg),
            (theme::RED, green),
            "red on delta's colour"
        );
    }

    /// Git searching every version (Ctrl-Alt-F) for `query`, with a row found.
    fn versions_found(query: &str) -> (App, Instant) {
        let (mut a, now) = app_with_preview(Tab::Git);
        a.handle_key(crate::app::tests::ctrl_alt('f'), now);
        for c in query.chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        a.tick(now + std::time::Duration::from_secs(1));
        let rev = "0123456789abcdef0123456789abcdef01234567";
        a.on_list(crate::runner::ListMsg::Items(
            a.list_gen,
            crate::app::tests::rows(
                crate::commands::ParseKind::Versions,
                &[&format!("{rev}:src/a.c\x009\x00the Needle here")],
            ),
        ));
        (a, now)
    }

    #[test]
    fn searching_versions_says_so_and_marks_what_it_found() {
        let (mut a, now) = versions_found("needle");
        let buf = render(&mut a, 120, 30);
        assert!(
            row(&buf, 29).contains("Search in versions › needle"),
            "{:?}",
            row(&buf, 29)
        );
        assert!(!row(&buf, 29).contains("regex"), "the default goes unsaid");
        let r = "0123456 src/a.c:9:the Needle here";
        assert_eq!(fg_at(&buf, r, 22), theme::RED, "N of Needle, case aside");
        assert_ne!(fg_at(&buf, r, 18), theme::RED, "t of the");
        assert!(row(&buf, 0).contains("Esc back"), "{:?}", row(&buf, 0));
        a.on_versions(vec![crate::versions::VersionRow {
            hash: "0123456789abcdef0123456789abcdef01234567".into(),
            date: "2026-10-17".into(),
            author: "Bilal Akbana".into(),
            branch: "main".into(),
            subject: "sf 0.9.1".into(),
            name: "v0.9.1".into(),
        }]);
        a.handle_key(ctrl('s'), now);
        let buf = render(&mut a, 120, 30);
        assert!(find(&buf, " Search in versions options ").is_some());
        assert!(find(&buf, "── Versions ──").is_some());
        assert!(
            find(&buf, "a all versions").is_some(),
            "the popup's own keys"
        );
        assert!(
            find(
                &buf,
                "[x] 0123456  2026-10-17  Bilal Akbana  main  sf 0.9.1"
            )
            .is_some(),
            "hash, date, author, branch, subject"
        );
        let (mut b, now) = versions_found("");
        let input = row(&render(&mut b, 120, 30), 29);
        assert!(
            input.contains("Search in versions › type a pattern"),
            "{input:?}"
        );
        let _ = now;
    }

    #[test]
    fn ctrl_p_shows_the_files_and_says_how_many_are_chosen() {
        let (mut a, now) = app_with_preview(Tab::Content);
        let top = row(&render(&mut a, 200, 30), 0);
        assert!(top.contains("^P files"), "{top:?}");
        feed(&mut a, &["src/a.c", "docs/b.md"]);
        a.handle_key(ctrl('p'), now);
        let buf = render(&mut a, 120, 30);
        assert!(find(&buf, " Files to search in ").is_some());
        assert!(find(&buf, "[x] All files").is_some());
        assert!(find(&buf, "[x] src/a.c").is_some() && find(&buf, "[x] docs/b.md").is_some());
        assert!(find(&buf, "a all files").is_some());
        a.popup = Some(2);
        a.handle_key(key(KeyCode::Char(' ')), now);
        a.handle_key(key(KeyCode::Esc), now);
        let input = row(&render(&mut a, 120, 30), 29);
        assert!(input.contains("1 file chosen"), "{input:?}");
    }

    #[test]
    fn the_scroll_bars_are_plain_to_see_and_where_the_mouse_finds_them() {
        let (mut a, _) = app_with_preview(Tab::Content);
        let names: Vec<String> = (0..60).map(|i| format!("f{i}.txt")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        feed(&mut a, &names);
        render(&mut a, 120, 30);
        let lines: Vec<Line> = (0..200).map(|i| Line::raw(i.to_string())).collect();
        a.on_preview(crate::preview::PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::from(lines),
        });
        let buf = render(&mut a, 120, 30);
        for bar in [a.layout.preview_bar, a.layout.list_bar] {
            assert!(bar.height > 0, "{bar:?}");
            let thumb = (bar.y..bar.bottom())
                .map(|y| &buf[(bar.x, y)])
                .find(|c| c.symbol() == "█")
                .expect("a full block for the thumb");
            assert_eq!(thumb.fg, theme::MAUVE, "the accent, not a dim line");
        }
    }

    #[test]
    fn a_versions_row_names_its_version_beside_the_hash() {
        let (mut a, _) = versions_found("needle");
        a.on_versions(vec![crate::versions::VersionRow {
            hash: "0123456789abcdef0123456789abcdef01234567".into(),
            date: "2026-10-17".into(),
            author: "T".into(),
            branch: "main".into(),
            subject: "s".into(),
            name: "v7.8.5~3".into(),
        }]);
        let buf = render(&mut a, 120, 30);
        let r = "0123456 v7.8.5~3 src/a.c:9:the Needle here";
        let (x, y) = find(&buf, r).expect(r);
        assert_eq!(buf[(x + 8, y)].fg, theme::YELLOW, "the name, as a tag");
        assert_eq!(buf[(x + 31, y)].fg, theme::RED, "N of Needle, still marked");
        assert_ne!(buf[(x + 27, y)].fg, theme::RED, "t of the");
    }

    #[test]
    fn git_names_ctrl_alt_f_for_versions() {
        let (mut a, _) = app(Tab::Git);
        let top = row(&render(&mut a, 200, 24), 0);
        assert!(top.contains("^Alt-F search in versions"), "{top:?}");
    }

    #[test]
    fn git_names_ctrl_f() {
        let (mut a, _) = app(Tab::Git);
        let top = row(&render(&mut a, 200, 24), 0);
        assert!(top.contains("^F find"), "{top:?}");
    }

    /// Git with `lines` as the commit's preview, Ctrl-F finding `text` in it.
    fn finding_in(lines: Vec<Line<'static>>, text: &str) -> (App, Instant) {
        let (mut a, now) = app_with_preview(Tab::Git);
        feed(&mut a, &["abcdef1"]);
        render(&mut a, 120, 30);
        a.on_preview(crate::preview::PreviewMsg {
            found: None,
            generation: a.preview_gen,
            text: Text::from(lines),
        });
        a.handle_key(ctrl('f'), now);
        for c in text.chars() {
            a.handle_key(key(KeyCode::Char(c)), now);
        }
        (a, now)
    }

    #[test]
    fn ctrl_f_colours_every_match_red_the_current_one_underlined() {
        let lines = vec![Line::raw("x one"), Line::raw("two x X")];
        let (mut a, now) = finding_in(lines, "x");
        let buf = render(&mut a, 120, 30);
        let (x0, y0) = find(&buf, "x one").unwrap();
        let (x1, y1) = find(&buf, "two x X").unwrap();
        let underlined = |c: &ratatui::buffer::Cell| c.modifier.contains(Modifier::UNDERLINED);
        let current = &buf[(x0, y0)];
        assert_eq!(current.fg, theme::RED, "the current one red");
        assert!(underlined(current) && current.modifier.contains(Modifier::BOLD));
        for x in [x1 + 4, x1 + 6] {
            let c = &buf[(x, y1)];
            assert_eq!(c.fg, theme::RED, "every other one red, case aside");
            assert!(!underlined(c));
        }
        for c in [&buf[(x0, y0)], &buf[(x0 + 2, y0)], &buf[(x1 + 4, y1)]] {
            assert!(
                c.bg != theme::RED && c.bg != theme::SURFACE,
                "no zone, no band"
            );
        }
        assert_ne!(
            buf[(x1, y1)].fg,
            theme::RED,
            "the rest of the line as it was"
        );
        a.handle_key(key(KeyCode::Enter), now);
        let buf = render(&mut a, 120, 30);
        assert!(underlined(&buf[(x1 + 4, y1)]), "Enter: the next is current");
        assert!(!underlined(&buf[(x0, y0)]) && buf[(x0, y0)].fg == theme::RED);
    }

    #[test]
    fn a_match_across_coloured_parts_is_marked_whole() {
        let green = Style::new().bg(ratatui::style::Color::Rgb(0, 80, 0));
        let line = Line::from(vec![
            Span::styled("+ ne", green),
            Span::styled("edle b", green.fg(theme::YELLOW)),
        ]);
        let (mut a, _) = finding_in(vec![Line::raw("ctx"), line], "needle");
        let buf = render(&mut a, 120, 30);
        let (x, y) = find(&buf, "+ needle b").unwrap();
        let green = ratatui::style::Color::Rgb(0, 80, 0);
        for dx in 2..8 {
            let c = &buf[(x + dx, y)];
            assert_eq!((c.fg, c.bg), (theme::RED, green), "column {dx}");
            assert!(c.modifier.contains(Modifier::UNDERLINED), "column {dx}");
        }
        assert_eq!(buf[(x, y)].bg, green, "no band before it");
        assert_eq!(
            (buf[(x + 9, y)].bg, buf[(x + 9, y)].fg),
            (green, theme::YELLOW),
            "after it, the part's own colours"
        );
    }

    #[test]
    fn the_preview_names_what_is_found_and_how_often_at_its_top_right() {
        let lines = vec![Line::raw("a b a"), Line::raw("a")];
        let (mut a, now) = finding_in(lines, "a");
        let top = |a: &mut App| row(&render(a, 120, 30), 1);
        assert!(
            top(&mut a).trim_end().ends_with("a · 1/3 ╮"),
            "{:?}",
            top(&mut a)
        );
        assert!(
            !row(&render(&mut a, 120, 30), 29).contains("1/3"),
            "the count is not on the input line too"
        );
        a.handle_key(key(KeyCode::Enter), now);
        assert!(
            top(&mut a).trim_end().ends_with("a · 2/3 ╮"),
            "{:?}",
            top(&mut a)
        );
        a.handle_key(key(KeyCode::Char('z')), now);
        assert!(top(&mut a).contains("az · not found"), "{:?}", top(&mut a));
        a.handle_key(key(KeyCode::Backspace), now);
        a.options.find.regex = true;
        a.handle_key(key(KeyCode::Char('(')), now);
        let buf = render(&mut a, 120, 30);
        let (x, y) = find(&buf, "a( · bad regex").expect("bad regex");
        assert_eq!(y, 1, "on the preview's border");
        assert_eq!(buf[(x + 5, y)].fg, theme::RED);
    }

    /// Content with a row and its preview, which found `count` matches of "interest".
    fn content_found(count: Option<u64>) -> (App, Instant) {
        let (mut a, now) = app_with_preview(Tab::Content);
        feed(&mut a, &["a.md"]);
        render(&mut a, 120, 30);
        a.on_preview(crate::preview::PreviewMsg {
            found: Some(crate::preview::Found {
                pattern: "interest".into(),
                count,
            }),
            generation: a.preview_gen,
            text: Text::from(vec![Line::raw("fn interest")]),
        });
        (a, now)
    }

    #[test]
    fn content_names_the_pattern_and_its_count_at_the_top_right() {
        let top = |a: &mut App| row(&render(a, 120, 30), 1).trim_end().to_string();
        let (mut a, now) = content_found(Some(12));
        assert!(
            top(&mut a).ends_with(" interest · 12 matches ╮"),
            "{:?}",
            top(&mut a)
        );
        let (mut b, _) = content_found(Some(1));
        assert!(
            top(&mut b).ends_with(" interest · 1 match ╮"),
            "{:?}",
            top(&mut b)
        );
        let (mut c, _) = content_found(None);
        assert!(
            top(&mut c).ends_with(" interest · ? matches ╮"),
            "{:?}",
            top(&mut c)
        );
        a.handle_key(ctrl('f'), now);
        for ch in "fn".chars() {
            a.handle_key(key(KeyCode::Char(ch)), now);
        }
        let t = top(&mut a);
        assert!(
            t.ends_with(" interest · 12 matches ─ fn · 1/1 ╮"),
            "Ctrl-F's at the far right: {t:?}"
        );
        let buf = render(&mut a, 120, 30);
        let (x, y) = find(&buf, "12 matches").unwrap();
        assert_eq!(buf[(x, y)].fg, theme::PEACH);
    }

    #[test]
    fn a_search_that_ran_out_of_time_says_so() {
        let (mut a, _) = finding_in(vec![Line::raw("a")], "a");
        let f = a.preview_find.as_mut().unwrap();
        (f.hits, f.current, f.slow) = (Vec::new(), None, true);
        let top = row(&render(&mut a, 120, 30), 1);
        assert!(top.contains(" a · too slow "), "{top:?}");
    }

    #[test]
    fn what_is_found_stays_at_the_top_while_the_preview_scrolls() {
        let lines: Vec<Line> = (0..200).map(|i| Line::raw(format!("{i} x"))).collect();
        let pattern = r"(?<=\d )x\b";
        let (mut a, now) = finding_in(lines, pattern);
        a.handle_key(key(KeyCode::Down), now);
        render(&mut a, 120, 30);
        for _ in 0..5 {
            a.handle_key(alt(KeyCode::Down), now);
        }
        let top = row(&render(&mut a, 120, 30), 1);
        assert!(a.preview_scroll > 20, "scrolled: {}", a.preview_scroll);
        assert!(top.contains(&format!(" {pattern} · ")), "as typed: {top:?}");
        let (mut a, _) = finding_in(vec![Line::raw("a")], "");
        assert!(
            !row(&render(&mut a, 120, 30), 1).contains(" · "),
            "nothing typed, nothing said"
        );
    }

    #[test]
    fn what_is_found_keeps_its_place_when_the_preview_is_narrow() {
        let (mut a, _) = finding_in(vec![Line::raw("needle")], "needle");
        a.root = std::path::PathBuf::from(
            "/a/very/long/folder/name/that/fills/the/whole/border/and/more",
        );
        let top = row(&render(&mut a, 60, 30), 1);
        assert!(top.contains("needle · 1/1"), "{top:?}");
    }

    #[test]
    fn ctrl_s_while_finding_shows_finds_options() {
        let (mut a, now) = finding_in(vec![Line::raw("a")], "");
        let buf = render(&mut a, 120, 30);
        assert!(row(&buf, 29).contains("^S options"), "{:?}", row(&buf, 29));
        assert!(
            !row(&buf, 29).contains("all branches"),
            "the Git tab's options are not Find's: {:?}",
            row(&buf, 29)
        );
        a.handle_key(ctrl('s'), now);
        let buf = render(&mut a, 120, 30);
        assert!(find(&buf, " Find options ").is_some());
        assert!(find(&buf, "Regular expression").is_some());
        a.handle_key(key(KeyCode::Char(' ')), now);
        a.handle_key(key(KeyCode::Esc), now);
        let input = row(&render(&mut a, 120, 30), 29);
        assert!(input.contains("whole word"), "{input:?}");
    }

    #[test]
    fn finding_from_the_result_filter_shows_only_what_is_found() {
        let (mut a, now) = app_with_preview(Tab::Content);
        feed(&mut a, &["a.txt"]);
        a.query = "needle".into();
        a.result_filter = Some("words".into());
        a.handle_key(ctrl('f'), now);
        assert!(a.preview_find.is_some());
        let input = row(&render(&mut a, 120, 30), 29);
        assert!(
            input.contains("Find in preview › type to find") && !input.contains("filter"),
            "{input:?}"
        );
        a.handle_key(key(KeyCode::Esc), now);
        let input = row(&render(&mut a, 120, 30), 29);
        assert!(input.contains("needle · filter: words"), "back: {input:?}");
    }

    #[test]
    fn marks_split_the_parts_they_cross() {
        let red = Style::new().fg(theme::RED);
        let line = Line::from(vec![Span::raw("ab"), Span::styled("cd", red)]);
        let marked = mark_matches(line, &[(1..3, false)]);
        let parts: Vec<(&str, Style)> = marked
            .spans
            .iter()
            .map(|s| (s.content.as_ref(), s.style))
            .collect();
        let other = Style::new().fg(theme::RED);
        assert_eq!(
            parts,
            [
                ("a", Style::new()),
                ("b", Style::new().patch(other)),
                ("c", red.patch(other)),
                ("d", red)
            ]
        );
    }

    #[test]
    fn layout_rects_recorded() {
        let (mut a, _) = app_with_preview(Tab::Files);
        let buf = render(&mut a, 120, 30);
        let l = &a.layout;
        assert_eq!(l.tabs.len(), 6);
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
