# sf (Rust TUI) Implementation Plan

> Executed 2026-10-17. Where the code differs from a step below (e.g. Matching lines via `--replace`, compact rows,
> no `theme::PEACH`), the reason is in `docs/sf-tui-design.md` (Architecture, "Decided while building").

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One terminal app `sf` with the tabs Files · Search · Folders · Git, a Config popup per tab, mouse and keys, Catppuccin on the terminal background, replacing the fzf prototype.

**Architecture:** A pure `App` state machine (keys/mouse in, effects out) drawn by ratatui; list sources and previews run external engines (fd, ugrep/ugrep+, git, bat, delta) on worker threads that stream into channels; actions go through a platform module (Linux or WSL) or run in tmux / in the foreground.

**Tech Stack:** Rust 1.94 (edition 2024), ratatui 0.30.2 (crossterm 0.29 via `ratatui::crossterm`), ansi-to-tui 8.0.1, serde 1 + toml 1, dirs 7, anyhow 1.

**Spec:** `docs/sf-tui-design.md`

## Global Constraints

- Crate `sf` in `~/.SearchFunctions/sf-rs`; no dependencies beyond the six above without a stated reason.
- Deviation from the spec, decided while planning: no `nucleo`. nucleo always ranks by score; Bilal's fzf runs with `--no-sort -i -e` (input order kept, case-insensitive substring, all words must match). An own filter keeps that order and those semantics exactly (Task 4 proves its speed).
- Every external program runs through `std::process::Command` with an argument list; never `sh -c` with data; paths after `--`.
- Colours (Catppuccin Macchiato): text `#cad3f5`, subtext0 `#a5adcb`, overlay1 `#8087a2`, overlay0 `#6e738d`, surface0 `#363a4f`, base `#24273a`, mauve `#c6a0f6`, blue `#8aadf4`, green `#a6da95`, red `#ed8796`, peach `#f5a97f`; backgrounds `Color::Reset` (terminal's own) except the selected row (surface0), the current tab (mauve) and the popup border.
- Config file `~/.config/sf/config.toml` (or `$XDG_CONFIG_HOME/sf/config.toml`); sub-functions in `~/.SearchFunctions/sub-functions` (override: env `SF_SUBFUNCTIONS`).
- Defaults: Files hidden ✓ ignored ✓; Search matching lines ✗ archives ✗ hidden ✓ ignored ✗; Folders hidden ✓ ignored ✓; Git all branches ✓.
- Search waits 150 ms after the last keystroke; `.git` is never listed; empty Search query lists nothing and says "type a pattern".

## Review Focus

1. File names with spaces, quotes, `&`, `$(..)`, a leading `-`, `:` or control characters (incl. 0x1f): drawn with U+FFFD for control characters, acted on with the real path, never parsed by a shell — tests in Tasks 3 and 10.
2. A huge or slow source (millions of lines, a slow archive search): items stream in, a new query or tab kills the old process, typing stays responsive — tests in Tasks 4 and 5.
3. Missing engines (no `ugrep+`, `delta`, `bat`, `fd`): the tab that needs one says so, the rest works — tests in Task 2.
4. Narrow terminals (60 columns): Config stays visible at the right, nothing overlaps, the footer is cut cleanly — test in Task 8.
5. Outside tmux, a terminal program (nvim, Ctrl-T window) takes the terminal and sf comes back intact; a folder that is not a git repository in Git; unreadable files making ugrep exit 2 are not shown as failure — tests in Tasks 5, 10, 11.

---

### Task 1: Model, theme and config

**Files:** Create `src/model.rs`, `src/theme.rs`, `src/config.rs`; modify `src/main.rs` (module declarations only).

**Interfaces — Produces:**
- `enum Tab { Files, Search, Folders, Git }`, `Tab::ALL: [Tab; 4]`, `fn title(self) -> &'static str`, `fn next(self) -> Tab`, `fn prev(self) -> Tab`, `fn from_name(&str) -> Option<Tab>`.
- `enum Opt { Hidden, Ignored, MatchingLines, Archives, AllBranches }`, `fn label(self) -> &'static str`, `fn hint(self) -> &'static str`.
- `#[derive(Serialize, Deserialize)] struct Options { files: FilesOpts, search: SearchOpts, folders: FoldersOpts, git: GitOpts }` (each `#[serde(default)]` with the spec's defaults), `fn list(&self, tab) -> Vec<(Opt, bool)>` (in popup order), `fn get(&self, tab, opt) -> bool`, `fn toggle(&mut self, tab, opt)`.
- `config::default_path() -> Option<PathBuf>`, `config::load(&Path) -> Result<Options, String>` (missing file → defaults), `config::save(&Path, &Options) -> anyhow::Result<()>` (creates the folder).
- `theme::{TEXT, SUB, DIM, FAINT, SURFACE, BASE, MAUVE, BLUE, GREEN, RED, PEACH}: Color`.

- [ ] Tests (`config.rs`): `defaults_match_spec` (the eleven values above), `round_trip`, `missing_file_gives_defaults`, `invalid_file_is_an_error_and_untouched` (bytes before == after), `partial_file_fills_defaults`; (`model.rs`): `tab_cycle_wraps` (Git.next() == Files, Files.prev() == Git), `search_popup_order` (MatchingLines, Archives, Hidden, Ignored).
- [ ] `cargo test` fails, implement, `cargo test` passes.

### Task 2: Engines and command lines

**Files:** Create `src/engines.rs`, `src/commands.rs`.

**Interfaces — Produces:**
- `struct Engines { fd, bat, ugrep, ugrep_plus, git, delta, file, ls: Option<PathBuf> }`, `fn discover(path_var: &OsStr) -> Engines` (`fd` or `fdfind`, `bat` or `batcat`).
- `struct Cmd { program: PathBuf, args: Vec<OsString>, env: Vec<(String, String)> }`.
- `enum ListPlan { Run { cmd: Cmd, kind: ParseKind }, Hint(String) }`, `enum ParseKind { Files, SearchFiles { archives: bool }, SearchLines, Folders, Git }`.
- `fn list_plan(tab: Tab, opts: &Options, query: &str, e: &Engines) -> ListPlan`.
- `enum PreviewPlan { Run(Cmd), Pipe(Cmd, Cmd), Sniff { path: PathBuf }, Text(String), Nothing }`, `fn preview_plan(tab, opts, query, item: &Item, width: u16, e: &Engines, root: &Path) -> PreviewPlan`.
- Constants `GREP_PALETTE` (from the prototype) and `delta_args(theme: &str) -> Vec<OsString>` (catppuccin/delta Macchiato options from the prototype).

- [ ] Tests: `files_cmd_with_defaults` (`--type f --color=never --hidden --no-ignore --exclude .git`), `files_cmd_without_hidden_and_ignored`, `search_empty_query_is_hint`, `search_files_cmd` (`ugrep -rlU --sort=name --smart-case --ignore-binary -s --hidden --ignore-files --exclude-dir=.git -e <q> .`), `search_archives_uses_ugrep_plus_z`, `search_lines_format_puts_key_first` (`--format=%f\x1f%n\x1f%O%~`), `query_starting_with_dash_follows_e`, `missing_ugrep_plus_is_hint`, `git_all_branches_flag`, `discover_finds_fdfind_and_batcat` (temp dir PATH with those two names).
- [ ] Before writing `search_lines`, check with the real ugrep that `--format` prints a line once even with several matches on it (`printf 'eee\n' | ugrep --format='%n%~' -e e` → `1` once); if not, add `%u`.

### Task 3: Items and parsers

**Files:** Create `src/items.rs`.

**Interfaces — Consumes:** `ParseKind`, theme colours. **Produces:**
- `struct Item { key: String, line: Option<u32>, display: Line<'static>, haystack: String, selectable: bool }`.
- `fn parse(kind: ParseKind, raw: &str, git_number: u32) -> Option<Item>`; `fn printable(&str) -> String` (C0, DEL, C1 → U+FFFD); `fn sort_folders(&mut Vec<String>)` (depth, then byte order); archive key: `a.zip{m}` → `a.zip` for the archive extensions of the prototype's `ARCHIVE` list.

- [ ] Tests: `file_display_dims_folder_part`, `control_chars_only_in_display` (key keeps `\x1b`, display has U+FFFD), `search_line_with_colon_in_name`, `search_line_text_with_0x1f_keeps_key_and_line`, `archive_member_key`, `git_line_takes_hash_from_last_field`, `git_graph_only_line_not_selectable`, `folders_sorted_by_depth`, `haystack_is_lowercase_display_text`.

### Task 4: Filter

**Files:** Create `src/filter.rs`.

**Interfaces — Produces:** `struct Filter`, `fn new() -> Filter`, `fn clear(&mut self)`, `fn push(&mut self, items: Vec<Item>)`, `fn set_query(&mut self, q: &str)`, `fn len(&self) -> usize` (matches), `fn total(&self) -> usize`, `fn get(&self, i: usize) -> Option<&Item>` (i-th match, input order). Words = query split on whitespace, lowercased; an item matches when every word is a substring of `haystack`; a narrowing query refilters only the previous matches.

- [ ] Tests: `all_words_must_match`, `case_insensitive`, `keeps_input_order`, `push_after_query_filters_new_items`, `widening_query_refilters_all`, `speed_300k_items` (release build: 300 000 items, ten queries, each < 30 ms; `#[ignore]`, run with `cargo test --release -- --ignored`).

### Task 5: List runner

**Files:** Create `src/runner.rs`.

**Interfaces — Consumes:** `Cmd`, `ParseKind`, `parse`. **Produces:** `enum ListMsg { Items(u64, Vec<Item>), Error(u64, String), Done(u64) }`, `struct Runner`, `fn new(tx: Sender<ListMsg>) -> Runner`, `fn start(&mut self, generation: u64, cmd: Cmd, kind: ParseKind, root: &Path)`, `fn cancel(&mut self)` (kills and reaps the child). Stdout read as bytes, lines decoded lossily, sent in batches (≤ 512 items or 50 ms); Files sorted and Folders depth-sorted before sending (they need all lines); SearchFiles deduplicated; every stderr line is an `Error`; the exit status is ignored (ugrep exits 1 on no match and 2 on one unreadable file).

- [ ] Tests: `streams_items_from_printf` (`/usr/bin/printf` with three lines → three items, then `Done`), `stderr_lines_become_errors`, `cancel_kills_child_quickly` (`sleep 30` cancelled in < 1 s, no zombie), `nonzero_exit_is_not_an_error`.

### Task 6: Preview worker

**Files:** Create `src/preview.rs`.

**Interfaces — Consumes:** `PreviewPlan`. **Produces:** `struct PreviewMsg { generation: u64, text: Text<'static> }`, `struct Previewer`, `fn new(tx: Sender<PreviewMsg>) -> Previewer`, `fn request(&self, generation: u64, plan: PreviewPlan)` (the worker keeps only the newest pending request), `fn is_text(path: &Path) -> io::Result<bool>` (port of `is_text_file.c`: first 8 KiB, any NUL → false, ≥ 95 % printable/space/≥0x80 → true, empty → false). Output capped at 4 MiB, converted with `ansi_to_tui::IntoText`; `Sniff` runs bat for text and `file -b` for binaries ("No text preview: …").

- [ ] Tests: `is_text_cases` (text, NUL binary, empty, UTF-8 Turkish text), `ansi_becomes_styled_spans`, `newest_request_wins` (three quick requests → only the last generation answered), `pipe_plan_feeds_second_command` (`printf a` | `tr a b` → `b`).

### Task 7: App state and keys

**Files:** Create `src/app.rs`.

**Interfaces — Consumes:** Tasks 1–6. **Produces:**
- `struct App` (tab, root, query shared by all tabs, options, engines, filter, selected, list offset, preview text and scroll, preview visible, `popup: Option<usize>` (cursor), status `Option<(String, bool /*error*/, Instant)>`, list/preview generations, `search_due: Option<Instant>`, `layout: Layout` filled by drawing).
- `enum Effect { RunList, Preview, SaveConfig, Act(ActionKind), ChangeRoot(PathBuf), Quit }`.
- `fn handle_key(&mut self, key: KeyEvent, now: Instant) -> Vec<Effect>`, `fn tick(&mut self, now: Instant) -> Vec<Effect>` (fires the Search debounce), `fn on_list(&mut self, msg: ListMsg)`, `fn on_preview(&mut self, msg: PreviewMsg)`, `fn current(&self) -> Option<&Item>`.
- Keys: Tab / BackTab cycle tabs; printable chars, Backspace, Ctrl-W and Alt-Backspace (word), Ctrl-Alt-H (line) edit the query; Up/Down/PgUp/PgDn/Home/End move; Alt-Up/Alt-Down scroll the preview; Ctrl-L preview on/off; Ctrl-R RunList; Ctrl-S opens the popup; Esc closes the popup, else Quit; Enter, Ctrl-O, Ctrl-T, Ctrl-U, Ctrl-G, Alt-G, Alt-U, Ctrl-E, Ctrl-C → `Act` (Enter in Folders → `ChangeRoot`). Popup: Up/Down, Space/Enter toggle (→ RunList + SaveConfig), Esc closes.

- [ ] Tests: `tab_and_backtab_cycle`, `typing_in_files_filters_at_once`, `typing_in_search_waits_150ms` (no RunList at 100 ms, RunList at 160 ms), `toggle_runs_list_and_saves`, `esc_closes_popup_before_quitting`, `enter_in_folders_changes_root` (`..` → parent), `stale_generation_ignored`, `action_without_item_says_nothing_selected`.

### Task 8: Drawing

**Files:** Create `src/ui.rs`.

**Interfaces — Consumes:** `App`. **Produces:** `fn draw(frame: &mut Frame, app: &mut App)` which also records `app.layout` (`tabs: Vec<(Tab, Rect)>`, `config: Rect`, `list: Rect`, `preview: Rect`, `popup_rows: Vec<(Opt, Rect)>`). Rows top to bottom: tab bar (1 line), preview (60 % unless hidden; border title = root path, folder dim, name bright; status shown in the bottom border), list (bordered), input line, footer. Input: `<Tab> › <query>`, dim placeholder when empty (Files "type to filter file names", Search "type a pattern · lists the files that contain it" / "… the lines that contain it", Folders "type to filter folders · Enter goes inside, .. goes up", Git "type to filter commits · Enter shows one"), right: active options joined with " · " and `matches/total`. Footer: Files/Search "Enter open · ^O nvim · ^T in file · ^U/^G diff · Alt-U log · ^E reveal · ^C copy · Esc quit", Folders "Enter go inside · ^E reveal · ^C copy · Esc quit", Git "Enter show commit · ^C copy hash · Esc quit".

- [ ] Tests (`TestBackend`): `tab_bar_is_one_line_with_config_right` (80 and 160 columns: row 0 holds the four titles and "Config" ends at the last column), `narrow_60_keeps_config_visible`, `popup_shows_search_options_with_boxes` (`[ ]` Matching lines … `[x]` Hidden files), `placeholder_when_query_empty`, `footer_matches_tab`, `layout_rects_recorded`.

### Task 9: Mouse

**Files:** Modify `src/app.rs`.

**Interfaces — Produces:** `fn handle_mouse(&mut self, m: MouseEvent, now: Instant) -> Vec<Effect>`: left click on a tab → switch; on Config → popup; on a list row → select; same row twice within 400 ms → as Enter; wheel over list → move 3; wheel over preview → scroll 3; in the popup a click on a row toggles, a click outside closes.

- [ ] Tests (layout from a `TestBackend` draw): `click_tab_switches`, `click_config_opens_popup`, `click_row_selects`, `double_click_acts`, `wheel_over_preview_scrolls`, `click_popup_row_toggles`, `click_outside_popup_closes`.

### Task 10: Actions

**Files:** Create `src/actions.rs`.

**Interfaces — Produces:** `enum ActionKind { Open, Reveal, Copy, Editor, InFile, DiffSplit, Diff, DiffShort, Log, ShowCommit }`; `enum Outcome { Done(Option<String>), Foreground(Cmd), Failed(String) }`; `struct Env { wsl: bool, tmux: bool, sub: PathBuf }` with `Env::detect()`; `fn perform(kind, tab, item: &Item, query: &str, root: &Path, e: &Engines, env: &Env) -> Outcome`. Linux: `xdg-open` (stdio null, exit status checked), `open_in_file_manager`, `wl-copy` (path relative to root on stdin); WSL: `windows_action open|reveal|copy-path|copy-text`. Terminal programs (nvim `[+N]` / `-c "/query" -c "set hlsearch"`, `search_in_single_file [query]`, `show_diffs`, `git_diff`, `git_diff_short`, `show_logs`): in tmux `tmux new-window -n <name> -c <root> <argv...>` then `tmux select-pane -T <title>`, else `Foreground`. Git: Enter → `Foreground(git show | delta --paging=always)` (Pipe), Ctrl-C copies the hash, other keys → "Not available in Git".

- [ ] Tests (stub programs in a temp PATH logging argv and stdin): `open_runs_xdg_open_with_real_path` (name with `&` and `'`), `copy_sends_relative_path_on_stdin`, `wsl_uses_windows_action`, `editor_in_tmux_opens_window`, `editor_outside_tmux_is_foreground_with_line`, `infile_passes_query_in_search_only`, `git_copy_is_hash`, `git_other_keys_not_available`, `xdg_open_failure_is_reported`.

### Task 11: Main loop

**Files:** Modify `src/main.rs`.

**Interfaces — Consumes:** everything. Behaviour: args `[files|search|folders|git]`, `-h/--help`; discover engines, load config (an error becomes the first status); `ratatui::init()` plus `EnableMouseCapture`; in tmux rename the window to `search` and restore it on exit and on panic; loop: poll crossterm events (30 ms), drain list/preview channels, `tick`, perform effects (`RunList` → runner, `Preview` → previewer with the preview width, `SaveConfig`, `Act` → `perform`, `Foreground` → restore terminal, run, re-init, redraw, `ChangeRoot`, `Quit`); always restore the terminal.

- [ ] End-to-end (`sf-rs/tests/e2e.sh`, private tmux server, the prototype's fixture tree, stubs for xdg-open / wl-copy / nvim / open_in_file_manager, `XDG_CONFIG_HOME` in a temp dir): start in Files, filter, Enter/^C/^O logged; Tab to Search, pattern, results, toggle Matching lines in the popup (Ctrl-S, Space) and see `file:line:` rows, config file written; Archives finds the zip member; Folders Enter changes the root; Git lists commits, outside a repo says so; Ctrl-T opens the `infile` window in tmux; outside tmux Ctrl-O runs nvim and sf redraws; screenshots of every tab rendered with `scratchpad/ansi2png.py` and looked at.

### Task 12: Speed, install, docs

**Files:** Modify `README.md`, `docs/sf-tui-design.md` (status, nucleo note).

- [ ] Release build; measure: Search on `/usr/include` (2.8 M lines in Matching lines) keeps keystrokes responsive; Files on `$HOME` filtering < 30 ms per keystroke.
- [ ] `cargo install --path sf-rs` (to `~/.cargo/bin/sf`); README section for the app (install, tabs, Config, keys, config file); the prototype stays until Bilal retires it.
- [ ] Commit (with Bilal's approval).
