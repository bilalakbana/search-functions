# sf — the search functions as one terminal app (Rust)

Status: approved by Bilal (2026-10-16) and built (2026-10-17, plan `docs/sf-tui-plan.md`, not yet reviewed by him).
Replaces the fzf prototype `~/.SearchFunctions/sf`, which stays in use until this version does everything it does.

## Why

The search functions are many separate commands (`f`, `ugrep-fzf`, `advancedsearch`, `detailedsearch`, `ugf`,
`gfzf`, folder lists, ...) that have to be remembered. The fzf prototype showed that one window with tabs works,
but fzf cannot give the interface Bilal wants: a single-line tab bar you click, a Config button that opens a popup
with checkboxes, defaults per tab, and a polished look that is understood at a glance. He also wants a compiled,
fast tool he can ship and use on the Linux PC and on WSL.

## Goals

1. One window. A single-line tab bar at the top: **Files · Search · Folders · Git**, and at its right end
   **⚙ Config**. Tabs are clicked with the mouse; Tab / Shift-Tab cycle forward / backward. No F-key labels.
2. **Config** opens a popup with the current tab's options as checkboxes (click or Space toggles, Esc closes).
   Changes apply at once and are remembered (`~/.config/sf/config.toml`).
3. **Search** replaces Content, Lines, `ugrep-fzf`, `advancedsearch(_plus)`, `detailedsearch` and `ugf`: the query
   is a ugrep pattern; by default the list shows the files that contain it; the option **Matching lines** shows the
   matching lines instead (file, line number, text); the option **Archives & documents** also searches inside
   zip/tar/7z/gz/... archives and PDF/Office documents (`ugrep+ -z`).
4. The actions keep their keys and work on the selected item, each in its own window: **Enter** open with the
   default application, **Ctrl-O** nvim (at the line in Matching lines), **Ctrl-T** search inside the file,
   **Ctrl-U / Ctrl-G / Alt-G** git diff (side by side / delta / short), **Alt-U** git log of the file,
   **Ctrl-E** show in the file manager, **Ctrl-C** copy the path. Inside tmux "own window" is a new tmux window;
   outside tmux the app steps aside (terminal handed over) and comes back when the program ends.
5. Understood without memorising: the input shows what typing does in this tab while it is empty, the active
   options are listed next to it, and a dim footer names the keys for the selected item.
6. Look: Catppuccin Macchiato (as Ghostty, tmux, Obsidian and the prototype) drawn on the terminal's own
   background, so the window follows the terminal's opacity.
7. Fast: results stream in as the engine finds them; filtering hundreds of thousands of items never freezes typing.
8. Linux (any desktop, Wayland or X11) and WSL alike, from one binary: open, reveal and copy go through sf's
   platform layer (see "Platform layer" under "Decided while building"); `sf doctor` says how they work here.

## Non-goals (this version)

- No own search, walk or diff engine: ugrep / ugrep+, fd, git, bat, delta stay the engines, run as programs.
- No own syntax highlighting: previews come from bat / delta / ugrep as coloured text and are drawn as such.
- No editing, no multi-select, no `logs*` / `fbr` / rga.
- No cd of the calling shell on exit (a later addition).

## Anti-goals

- More buttons, statistics or decoration than the goals above; it must stay calm and plain.
- Any value from the disk or from git becoming part of a shell command: every program gets its arguments as a
  list (no shell), file names never parsed as options (`--`), control characters never drawn raw.
- A failure that shows nothing: every engine error and failed action appears in the window.

## Screen

```
 Files   Search   Folders   Git                                                       ⚙ Config
╭ ~/Projects/app ──────────────────────────────────────────────────────────────────────────────╮
│ preview: the file (bat), the matches with context (ugrep), the commit (git show + delta)      │
│ ...                                                                                            │
╰────────────────────────────────────────────────────────────────────────────────────────────────╯
╭───────────────────────────────────────────────────────────────────────────────────────────────╮
│ src/lib.rs                                                                                     │
│▌src/main.rs                                                                                    │
╰───────────────────────────────────────────────────────────────────────────────────────────────╯
 Search › needle▏                                              hidden · archives & documents   2/340
 Enter open · ^O nvim · ^T in file · ^U/^G diff · Alt-U log · ^E reveal · ^C copy · Esc quit
```

- The current tab is a mauve block (as the tmux session name); the others dim text. ⚙ Config at the far right.
- The border of the preview carries the current folder; the selected list row a darker band. Every path (rows,
  the border, the preview's `File`) is coloured as ugrep-fzf does: folders blue, slashes yellow, the name green
  (Bilal, 2026-10-17). In Matching lines: the path so, line number dim, text bright, the match red.
- The list fills from the bottom as in fzf's default layout (Bilal, 2026-10-17): the first row right above the
  input line and selected; Up / PgUp / the wheel up go to later rows, Home to the top, End to the first row.
- The input line: tab name, the query; while empty a dim explanation of the tab ("type a pattern · lists the
  files that contain it"); on the right the active options and the count.

Config popup (Search):

```
            ╭─ Search options ────────────────────────────────╮
            │ [ ] Matching lines   list lines, not files       │
            │ [ ] Archives & documents   zip, tar, 7z, PDF,…   │
            │ [x] Hidden files                                  │
            │ [ ] Ignored files   .gitignore'd                  │
            │                         Space / click · Esc close │
            ╰─────────────────────────────────────────────────────╯
```

## Tabs and their options (defaults)

| Tab | Lists | Typing | Options (default) |
|---|---|---|---|
| Files | `fd --type f`, sorted | filters the list (fuzzy) | Hidden files ✗, Ignored files ✓ (Bilal) |
| Search | `ugrep -rl` (files) or matching lines (`ugrep -rn`); `ugrep+ -z` with archives | the ugrep pattern; runs after a short pause (150 ms) | Matching lines ✗ (Bilal), Archives & documents ✗, Hidden files ✗, Ignored files ✓ (Bilal) |
| Folders | `fd --type d`, sorted by depth, `..` first | filters the list | Hidden folders ✗, Ignored folders ✓ (Bilal) |
| Git | `git log --graph`, numbered | filters the list | All branches ✓ |

`.git` itself is never listed. Enter in Folders makes the selected folder the new root of every tab (`..` goes up);
in Git it shows the whole commit (delta, paged). The app starts in the current directory.

Details settled from the existing setup and the prototype:

- Filtering in Files, Folders and Git matches as Bilal's fzf does (`FZF_DEFAULT_OPTS="-i -e"`): case-insensitive
  substring, words separated by spaces must all match; no fuzzy matching. Every occurrence of the typed words is
  drawn red in the rows (Bilal, 2026-10-17: "yazdığım nereye tekabül etmiş bilmiyorum").
- Search with an empty query lists every text file, as `ugrep-fzf` does (`ugrep -rl … -e ''`), whatever the
  options: every line of every file, or every PDF converted, would be far too much (Bilal, 2026-10-17: the files
  should be there at the start). A broken pattern shows ugrep's message.
  In Matching lines with archives, a member `a.zip{m}` is shown as such and acted on as `a.zip`.
- Ctrl-O in Search opens nvim searching for the query; Ctrl-T in Search passes the query on.
- Ctrl-C copies the path relative to the root on Linux, the Windows path on WSL; the result ("Copied") or the
  error shows for a moment in the preview's border.
- Mouse: a click selects a row, a double click is Enter, the wheel scrolls the list or the preview under the
  pointer. Keys: arrows / PgUp / PgDn / Home / End in the list, Alt-Up / Alt-Down scroll the preview, Ctrl-L hides
  or shows the preview, Ctrl-R runs the list again.
- Inside tmux the window is named "search" while sf runs and gets its name back on exit.
- An unreadable or invalid config file is reported in the window and the defaults are used (the file is left
  as it is).

## Previews

- Files / Search-files: text through `bat` (`--theme ${BAT_THEME:-base16}`, the preview's width), binaries as
  `file -b`; with a query in Search, ugrep's matches with 8 lines of context, matches red.
- Search-lines: `bat` scrolled to the line, the line in a band.
- Folders: the folder's entries. Git: `git show` header, changed files, diff through delta (Catppuccin settings).
- Coloured output is converted with `ansi-to-tui`; a preview is produced in the background, the newest request wins.

## Architecture

- Rust (1.94; ratatui needs 1.88+), `ratatui` 0.30 with its `crossterm` 0.29 (alternate screen, mouse),
  `ansi-to-tui` 8, `serde` + `toml` (config), `dirs`, `anyhow`, `libc` (to end a list's whole process group).
  No `nucleo`: it always ranks by score, while Bilal's fzf keeps the input order (`--no-sort -i -e`); an own
  filter does exactly that, on all cores from 50 000 rows (1 million rows: 1–10 ms per keystroke).
- Rows are compact (about 160 bytes each); the coloured line is built only for the rows on screen. Measured:
  Files on the home folder (970 000 files) loads in 0.9 s with 300 MiB; Search for `e` in `/usr/include` with
  Matching lines (2.2 million lines) completes in 1.4 s, typing stays at about 10 ms while it streams.
- One UI thread with an event loop; the list source and the preview each run as child processes on worker
  threads that send their lines over channels; a new query or tab cancels (kills) the running child.
- Engines found at start (`fd`/`fdfind`, `bat`/`batcat`, `ugrep`, `ugrep+`, `git`, `delta`); a missing one is
  said in the window where it is needed.
- Actions: the `platform` module (open, reveal, copy on Linux or WSL), and the terminal programs (nvim and
  the sub-functions `search_in_single_file`, `show_diffs`, `git_diff`, `git_diff_short`, `show_logs` from
  `~/.SearchFunctions/sub-functions`) in a new tmux window, or in the foreground with the TUI suspended.
- Code in `~/.SearchFunctions/sf-rs` (crate and binary `sf`), installed with `cargo install --path sf-rs`
  (`~/.cargo/bin/sf`); a static musl build for WSL is an option. The fzf prototype is retired only when this
  version covers it.

## Testing

- Unit tests: engine output parsing (ugrep lines with `:` and control characters in names, git log lines),
  option → argument mapping, config load/save, WSL detection from injected environment and files, every
  platform action's arguments, stdin and environment against stub programs, the `sf doctor` report.
- Drawing tests with ratatui's `TestBackend` (tab bar, popup, footer) at 80 and 160 columns.
- End-to-end in a private tmux server against the fixture tree used for the prototype (tricky names, zip, binary,
  git repo), with xdg-open / wl-copy / dbus-send / nvim / sub-functions stubbed, a WSL run (`WSL_DISTRO_NAME`,
  stub `powershell.exe` and `wslpath`), OSC 52 through tmux and the commands; screenshots rendered from the
  captures. `tests/platform_action.sh`: the fzf functions' wrapper, with and without sf, and a real fzf function.
- Speed: Search on `/usr/include` (2.8 million lines) keeps typing responsive.

## Calls made by Claude (check these)

- Defaults changed by Bilal (2026-10-17): Hidden ✗ and Ignored ✓ in Files, Content and Folders (my first call
  for Content, Hidden ✓ / Ignored ✗, is gone: `node_modules` / `target` are now searched unless switched off);
  Archives ✗ (slow); Git: All branches ✓.
- Config is remembered across runs in `~/.config/sf/config.toml`.
- Quit with Esc (Ctrl-C stays "copy the path"); Config also with Ctrl-S; footer instead of a help screen.
- Engines stay external; the prototype's sub-function windows are reused for Ctrl-T/U/G/Alt-G/Alt-U.

Decided while building (2026-10-17):

- After Bilal's first use: the Search tab is called **Content** (`sf content`; `sf search` and a config file's
  `[search]` are still read); the keys for the selected item moved from the footer into the middle of the tab
  bar; the Content preview starts with `File`, `Pattern` and `Matches` (`ugrep -c -o`: matches, not lines), as
  ugrep-fzf; a bar on the right border of the list and the preview shows the position; an empty result clears
  the preview.

- tmux windows never get the folder with `new-window -c`: tmux expands formats there, so a folder named `#(cmd)`
  ran cmd (verified with tmux 3.5a; `##` escaping did not help). A fixed `sh -c 'cd -- "$1" && shift && exec "$@"'`
  gets the folder as an argument; `-e` values and program arguments are not expanded. Fixed in the prototype too.
- A list reads each record bounded (64 KiB, stderr lines 4 KiB): a minified file's 20 MB line never sits in memory
  whole; a cut path is left out and said, since it would name another file.

- Matching lines come from `ugrep -n --null --separator=0x1f --replace=0x01%o0x02`: the path ends at a NUL (it
  cannot hold one), every match is marked, so the list shows them red; `--format` printed a line once per match.
- Ctrl-O with a Search pattern gives nvim the pattern in `$SF_QUERY` (`let @/ = $SF_QUERY`), never as an Ex
  command: the prototype's `-c "/QUERY"` runs a command after `/|` in a crafted pattern.
- Enter in Folders stays in Folders (to go deeper or back with `..`) and clears the query.
- Previews: bat with line numbers only (`--style=numbers`, no header or grid, as the Ctrl-T window); a commit is
  `git show --patch-with-stat` through delta.
- The Config popup takes only its own keys while it is open; an unreadable config file is reported and never
  overwritten (the options then are not saved).

Platform layer (Step 1 of `docs/sf-next-steps.md`, 2026-10-17):

- `platform.rs` decides once: WSL when `WSL_DISTRO_NAME` or `WSL_INTEROP` is set, `/proc/sys/fs/binfmt_misc/
  WSLInterop` exists or `/proc/version` says "microsoft"; Linux otherwise, with `WAYLAND_DISPLAY`, `DISPLAY`,
  `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP`. The WSL edition's `windows_action` is no longer needed.
- Open: `xdg-open PATH`; WSL `powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop';
  Invoke-Item -LiteralPath $env:SF_VALUE"` with the `wslpath -w` path in `SF_VALUE` and `WSLENV=SF_VALUE[:old]`
  (the statements hold no `"`; WSL quotes each argument). Open and reveal first check that the path exists.
- Reveal: `dbus-send --session --print-reply … org.freedesktop.FileManager1.ShowItems array:string:file:///…
  string:` with every byte but `A-Za-z0-9-._~/` percent-encoded (a `,` would split dbus-send's array); the reply
  is waited for, so a missing file manager is noticed and `xdg-open` opens the folder instead, said in the status.
  sf no longer runs `open_in_file_manager` (after xdg-open it added nothing). WSL: Explorer `/select,"PATH"`;
  a Windows path with `"` or a control character is refused.
- Copy: chosen by what the session has: Wayland `wl-copy`, X11 `xclip -selection clipboard` / `xsel --clipboard
  --input`, WSL `Set-Clipboard`; without any, OSC 52: inside tmux `tmux load-buffer -w -` (works with tmux's
  default `set-clipboard external`), else written to `/dev/tty`; the status then says so. A program that fails is
  reported, not replaced by the next one.
- `sf open|reveal|copy-path PATH`, `sf copy-text TEXT` (relative paths are taken in the current folder; exit 1 and
  the reason on stderr on failure) and `sf doctor` (platform, the method of each action, every engine with path
  and version, the sub-functions, the config; exit 0).
- The fzf functions' Enter / Ctrl-e / Ctrl-c call `sub-functions/platform_action ACTION VALUE` (Bilal: all of
  them). It uses sf only when `sf --help` names `sf open|reveal|copy-path PATH`: an older sf build or another
  program called sf (the Salesforce CLI) leaves the work to xdg-open / `open_in_file_manager` / wl-copy. Errors go
  to stderr and, in tmux, to `display-message -l` (printed as is, never expanded as a format). The fzf prototype
  `~/.SearchFunctions/sf` keeps its own calls: it is to be retired, and its e2e must never reach a real sf.
- ugrep, measured on 2026-10-17 by running sf's ugrep commands on the fixture with the distributions' packages
  in Docker and comparing byte by byte with 7.8.5: 3.3.3 (Debian 11) rejects `--replace`, so Matching lines fail;
  3.7.2 (Ubuntu 22.04) and 3.11.2 (Debian 12) work but mark only a line's first match; 5.0.0 (Ubuntu 24.04) is
  identical. `sf doctor` warns below 3.6.0 and below 5.0.0.
- The tests write their stub programs through a child process: a stub written by the test process itself could
  be held open by a fork in another test thread, and running it then failed with "Text file busy" (ETXTBSY).
