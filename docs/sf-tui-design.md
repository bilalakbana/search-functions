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

1. One window. A single-line tab bar at the top: **Files · Content · Deep · Folders · Git** (first Files · Search ·
   Folders · Git), and at its right end
   **⚙ Config**. Tabs are clicked with the mouse; Tab / Shift-Tab cycle forward / backward. No F-key labels.
2. **Config** opens a popup with the current tab's options as checkboxes (click or Space toggles, Esc closes).
   Changes apply at once and are remembered (`~/.config/sf/config.toml`).
3. **Search** replaces Content, Lines, `ugrep-fzf`, `advancedsearch(_plus)`, `detailedsearch` and `ugf`: the query
   is a ugrep pattern; by default the list shows the files that contain it; the option **Matching lines** shows the
   matching lines instead (file, line number, text), **Each match** a row for every match with the text around
   it (`detailedsearch`); the option **Archives & documents** also searches inside zip/tar/7z/gz/... archives and
   PDF/Office documents (`ugrep+ -z`). The tab **Deep** (2026-10-17) is the same search in every file, binaries and
   archives included: `advancedsearch`, `advancedsearch_plus` and `detailedsearch` under one Config.
4. The actions keep their keys and work on the selected item: **Enter** open with the default application,
   **Ctrl-O** nvim (at the line in Matching lines), **Ctrl-T** search inside the file, **Ctrl-U / Ctrl-G / Alt-G**
   git history / diff / short diff, **Alt-U** git log of the file, **Ctrl-E** show in the file manager,
   **Ctrl-C** copy the path. Since Step 3 (v0.3.0) Ctrl-T and the git keys are shown inside sf; only the editor
   gets a window of its own: a new tmux window, or outside tmux the terminal handed over until it ends.
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
│ preview: the file (bat), the numbered matches (ugrep), the commit (git show + delta)          │
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
| Search | `ugrep -rl` (files), matching lines (`ugrep -rn`) or each match (`ugrep -rno -B50 -A50`); `ugrep+ -z` with archives | the ugrep pattern; runs after a short pause (150 ms) | Matching lines ✗ (Bilal), Each match ✗, Archives & documents ✗, Hidden files ✗, Ignored files ✓ (Bilal) |
| Deep | `ugrep -rzl` (files, binaries and archives included) or each match (`ugrep -rznoa -B50 -A50`); `ugrep+` with Documents | as Search | Each match ✗, Documents ✗, Hidden files ✗, Ignored files ✓ |
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
  `file -b`; with a query in Search, the matches numbered: "Match #N" over each matching line with 2 lines around
  it, the line marked with `:` and its matches red, a thin rule between the blocks (Step 4).
- Search-lines and Each match: `bat` scrolled to the line, the line in a band.
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
- Actions: the `platform` module (open, reveal, copy on Linux or WSL), the editor in a new tmux window or in
  the foreground with the TUI suspended, and the views (`views.rs`) and Ctrl-T drawn by sf itself.
- Code in `~/.SearchFunctions/sf-rs` (crate and binary `sf`), installed with `cargo install --path sf-rs`
  (`~/.cargo/bin/sf`); the release package (see "Release package") has a static musl build for Linux and WSL.
  The fzf prototype is retired only when this version covers it.

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

Release package (Step 2 of `docs/sf-next-steps.md`, 2026-10-17):

- `sf-linux-x86_64.tar.gz` unpacks into `~/.local` (no top folder): `bin/sf`, `lib/sf/{fzf,fd,bat,delta,ugrep,ug,
  ugrep+}`, `lib/sf/sub-functions/` (the six scripts sf's keys run and `platform_action`), `share/doc/sf/`
  (README with the source commit, THIRD-PARTY.md, licenses/). x86_64 only; git, tmux, nvim and less come from the
  system (Bilal: no aarch64 for now).
- sf finds the bundle as `lib/sf` beside the folder of its own executable (`/proc/self/exe`, so symlinks to it
  work) and puts it first in its own `$PATH` before any thread starts: the engines are found there first, and
  everything sf starts inherits it (ugrep+ runs `ug`, the scripts run fzf, bat, ugrep, delta). tmux windows get
  sf's `PATH` with `-e PATH=…` (tmux would give them its server's). Sub-functions: `$SF_SUBFUNCTIONS`, else the
  bundle's, else `~/.SearchFunctions/sub-functions`. A `cargo install`ed sf has no bundle and behaves as before.
- The shipped scripts find each other beside themselves (`search_in_single_file` exports `SF_SUB`, its tmux
  windows get `-e PATH=$PATH`; `platform_action` takes `open_in_file_manager` from its own folder). `show_diffs`
  without git-split-diffs (npm, not shipped) shows `git log -p` through `delta --side-by-side` (Bilal's choice),
  as a plain pipe: `core.pager` would only page on a terminal.
- `packaging/bundle.toml` pins every download (URL + sha256): fzf, fd, bat, delta as their official static
  releases, ugrep's source archive, the Alpine image (tag + digest), and the license texts of the libraries
  linked into ugrep. `packaging/build.py` (stdlib Python, no shell strings) downloads and checks them, builds
  ugrep in Alpine 3.24 with `--enable-static LIBS=-lbrotlicommon` (brotlicommon after brotlidec/enc makes the
  static link work; ugrep's README would drop brotli instead) and stops when Alpine's library versions differ
  from bundle.toml, builds sf for `x86_64-unknown-linux-musl`, strips, refuses any binary that `file` does not
  call statically linked, and packs a deterministic tar (sorted, owner root, mtime of the commit, gzip mtime 0).
- Licenses in the package: sf (GPL-3.0, the repo's LICENSE; the README names the source commit), the five
  programs (fzf's from its tag, as its archive has none; bat's `--acknowledgements` for the syntaxes it
  carries), ugrep's libraries (pcre2, zlib, bzip2, xz, lz4, zstd, brotli, musl, the GCC runtime exception), the
  166 Rust crates sf links (their license files; three MIT crates without one get a NOTICE with their authors)
  and Rust's standard library (`COPYRIGHT-library.html` and its license texts).
- `packaging/test_package.sh` unpacks the package in clean Ubuntu 22.04, Debian 12 and Ubuntu 24.04 containers
  (apt gives only the tests' own git, tmux, zip, python3, file, less; the image must have none of the engines),
  checks that `sf doctor` names the bundled engines and sub-functions, and runs `tests/e2e.sh` against the
  installed sf and `tests/sub_functions.sh` against the packaged scripts.
- `.github/workflows/release.yml`: on a pushed tag `vX.Y.Z` (must equal Cargo.toml's version) unit tests, build,
  package test, then `gh release create` with the package and its `.sha256`; started by hand it does all but
  publish. Only `actions/checkout`, pinned by commit; `permissions: {}` at the top, `contents: write` for the
  job; values reach the scripts as environment variables. No weekly version check for now (Bilal).

Command line (v0.2.0, 2026-10-17; Bilal asked for engine flags and a print mode):

- `sf [files|content|deep|folders|git|help] [QUERY...] [--print] [-- ENGINE-OPTIONS...]` (`cli.rs`). QUERY (words joined
  by spaces) starts the tab with it: the filter in Files, Folders and Git, the pattern in Content. A word with a
  leading `-` before `--` is an error that points to `--` (a pattern starting with `-` is typed in the window).
  Kept as words (`sf files`), not `--files`: after `--` every option is the engine's.
- Options after `--` go to the starting tab's engine only (fd in Files and Folders, ugrep in Content, git log
  in Git): fd and git get them last, ugrep before `--exclude-dir=.git -e QUERY .`; Content's preview count and
  matches get them too, so the preview counts as the list does. They show in the input line (`-- -w -i`).
  Refused, with the reason: options that change what the engine prints, which sf parses (fd `-l -x -X -0 -c
  -q`, ugrep `-c -l -L -h -A -B -C -y -b -k -T -Q --json --format --replace --color …`, git `--oneline --pretty
  --format -p --stat …`; tables in `cli.rs`). None of the three takes abbreviated long options (checked);
  short clusters are read letter by letter until a letter that takes a value.
- `--print`, or stdout not a terminal (a pipe or a file): the list instead of the window, the window's rows
  (sorted, numbered, filtered by QUERY; `..` left out). On a terminal every row as drawn in the window, so no
  control character reaches it (the commit review found that a file name with escape sequences printed raw
  could drive the terminal). Into a pipe the real bytes for scripts: Files' and Folders' paths, Git rows as
  plain text, Content as ugrep's own untruncated `path` / `path:line:text` output. Exit 0 when something was
  listed, 1 when nothing, 2 on an error (an engine message, a missing engine); a closed pipe (`| head`) ends
  quietly. A wrong word prints the usage; a refused option only its one line.
- Ctrl-O: nvim, else `$VISUAL`, `$EDITOR` (with its words, as "code --wait"), vim, vi; vi and vim get `+LINE`,
  the pattern through `$SF_QUERY` and `--`, any other editor only `+LINE` and the absolute path. The tab bar
  names it (`^O nvim`, `^O nano`), the tmux window is named after it, `sf doctor` shows it.
- README: `wget -qO- … | tar -xz -C ~/.local` beside the curl line (curl is not on every desktop).

Step 3, the windows inside sf (v0.3.0, 2026-10-17):

- Ctrl-U, Ctrl-G, Alt-G, Alt-U and Enter in Git open a view over the whole window (`views.rs`, drawn by
  `ui::draw_view`): Ctrl-G `git -C DIR diff HEAD --unified=99999999 -- NAME` (the whole file), Alt-G the same with
  10 lines, Ctrl-U `git log -p`, all through `delta --side-by-side --width=W`; Alt-U `git log --name-status` with
  the old `show_logs` format; a commit `git show --patch-with-stat` through delta (top to bottom, as its preview,
  which is now made by the same function and runs `git -C ROOT`: before it ran in sf's starting folder, wrong
  after Folders went into another repository). git always runs with `-C` in the file's folder (a folder is
  diffed from inside, `.`), so submodules work. The text comes from a second preview worker (the newest request
  wins); nothing printed shows why ("No local changes in …, or git does not track it"); a resize asks again at
  the new width. Keys: Esc or q back, ↑↓ j k, PgUp PgDn b space, Home End g G, the wheel, `/` search
  (case-insensitive, Enter), n N next and previous; the found line in a band. Views are always inside sf, in tmux
  too (Bilal).
- Ctrl-T narrows Content to the file (`App::scope`): `ugrep -nHU --smart-case … -e PATTERN -- FILE` read as
  Matching lines, PATTERN the typed words joined by `|` (any of them, as the old script; Bilal), every line while
  empty; from Content it keeps the pattern, from Files it starts empty. The input reads "In NAME ›", Content's
  options are hidden there and the preview always shows the line (bat). Esc or Tab goes back to the tab and its
  query; Ctrl-O gets the same pattern.
- sf no longer runs any script: `Env.sub`, `$SF_SUBFUNCTIONS`, doctor's Sub-functions and the foreground
  `git show | delta` are gone; Ctrl-T and the git keys reaching `actions` (Folders, Git) say "Not available here".
  The package drops fzf and `lib/sf/sub-functions` (12 MB instead of 14). The scripts stay in the repo for the fzf
  functions, tested by `tests/sub_functions.sh`.

Step 4, advancedsearch and detailedsearch inside sf (2026-10-17):

- `advancedsearch` (files, then the matches in the preview) is Content as it was. `detailedsearch` is the new
  Content option **Each match** (Bilal: a new option, not a tab): a row for every match, `ugrep -rnoU -B50 -A50`,
  so each match comes with up to 50 columns of its line around it (ugrep's `-o` with context fits the match and
  its context in that many columns, `...` where it cut). The match is found in ugrep's colours: `--color=always`
  with `GREP_COLORS=ms=7:mc=7` and every other colour empty, so only the match is in reverse video, which sf turns
  into the same marks as Matching lines (red in the row). Matching lines and Each match are two ways to list the
  matches: turning one on turns the other off. The row, Ctrl-O, Ctrl-T and the preview work as in Matching lines;
  `--print` prints ugrep's own output.
- Content's preview with a pattern numbers the matches as `detailedsearch` did, in the form Bilal chose ("with
  context, numbered"): `File`, `Pattern`, `Matches` as before, then for each matching line "Match #N" (a line with
  three matches "Match #2–4", so the numbers add up to `Matches`), the line with 2 lines above and below, its
  number followed by `:` and its matches red, a thin rule between the blocks. The matching lines come from the
  same `ugrep -nHU --null --separator=0x1f --replace=0x01%o0x02` records as Matching lines (with the options after
  `--`); the lines around them are read from the file (its first 4 MiB, whole lines only), so a tab shows as
  spaces and a CRLF file without its `\r`. At most 300 blocks, then "… and N more matching lines". A binary file,
  an archive or a document shows each match on its own line with some bytes around it (`-znao -B200 -A200`),
  numbered the same way.

The Deep tab (2026-10-17; Bilal: a tab of its own, "Deep", because advancedsearch and detailedsearch also search
binaries such as dll and exe, which Content leaves out; in a repository full of binaries one Tab key switches
between the two instead of an option in the popup):

- Deep is `advancedsearch`, `advancedsearch_plus` and `detailedsearch` under one Config: the file list is
  `ugrep -rzl` without `--ignore-binary` (binaries, and archive members as `a.zip{m}` read as `a.zip`), `*.rar`
  left out as the scripts did; **Documents** runs `ugrep+` (PDF, Office, e-books through its filters:
  `advancedsearch_plus`); **Each match** is `-rzno -a -B50 -A50` (`detailedsearch`: `-a` shows a binary's
  matches as text, with the bytes around them). Hidden ✗, Ignored ✓ as in Content. While the pattern is empty it
  lists every file (`-rlU`, no `-z`: archive members are what a pattern finds).
- Everything else is Content's: one shared query (Tab from Content keeps the pattern), the 150 ms pause, rows not
  filtered by the pattern, the numbered preview (a binary's matches through `-znao -B200 -A200`; in Each match a
  binary row shows those, a text row bat at the line), the header with the count, Ctrl-T, Ctrl-O with the pattern,
  `sf deep PATTERN -- OPTIONS` with Content's refusals, `--print` and pipes. Content keeps its own Each match and
  Archives & documents (Bilal: keep them there too). The code shares Content's: `commands::Search` says how a
  tab searches, `Tab::searches()` which tabs take a pattern.
- Case: smart case as in the rest of sf (the scripts always passed `-i`); `sf deep P -- -i` for that.

After v0.5.0 (2026-10-17):

- Files starts without its preview (Bilal: the list is what Files is for; Ctrl-L shows it). Each tab keeps its
  own (`App::previews_hidden`), so hiding Content's leaves Deep's; without a preview the list's border names the
  root, which was in the preview's border only.
- Fixed after Bilal saw Deep's preview fail on binaries: (1) ansi-to-tui breaks a line at every carriage
  return, so a binary's match ending in `\r\r\n` became empty "Match #2, #3…" blocks; `items::ansi_text` now
  leaves `\r` out (the fzf previews ran `tr -d '\r'`), which also helps a CRLF file anywhere. (2) A file sf calls
  text (`is_text` looks at its first 8 KiB) with a NUL byte later made ugrep print "Binary file … matches"
  instead of the lines: an empty preview under "Matches 2". The numbered preview's lines command has `-a` now.
- Deep's preview made cheaper (Bilal chose all four; measured on this PC): a preview runs for the selected row
  only, so many binaries cost the list (Deep on /opt, 2.6 GB in 1909 files: 0.46 s, the first row after 16 ms),
  not the preview; one big file does. (1) A binary shows its first 300 matches, then the worker stops reading and
  ends ugrep (`run_limited`): a 312 MB ELF with 5522 matches took 0.77 s and 2.5 MB of text, its first 300 take
  0.09 s; the header still counts them all. (2) The header's count runs beside the matches (`with_header`, the
  worker kills every group it runs when a newer request comes): a 634 MB zip took 4.8 s per scan, 9.5 s for the
  two, now about 4.7 s. (3) A preview not there after 250 ms shows "searching inside NAME…" ("loading NAME…"
  outside Content and Deep) instead of the previous row's. (4) Deep starts a preview once the cursor rests for
  100 ms (`App::preview_wait`), so running down a list of binaries starts no scans. No size limit: it would hide
  what was searched for.
- The two offers Bilal approved: (1) the tab bar names `^S options` in every tab, and `^F filter` in Content and
  Deep, right after Enter, so they stay when the bar is narrow. (2) Ctrl-F in Content and Deep keeps the pattern
  and makes typing filter the rows found, by fzf's rules as in Files (`App::result_filter`): the input reads
  `Content › needle · filter: words`, the words are red in the rows, the search does not run again (Ctrl-R keeps
  the filter). Esc or Ctrl-F again goes back to the pattern (Bilal); another tab or Ctrl-T ends it. A start
  with a pattern (`sf deep needle`) begins in the filter, as `advancedsearch needle` did (Bilal). Where Esc goes
  back (the filter, a file after Ctrl-T) the bar says "Esc back" instead of "Esc quit".

File types in Ctrl-S (2026-10-17; Bilal asked for a key showing every file type, all on, to turn some off, then
chose to put them into the Ctrl-S popup instead of a key of their own):

- In Files, Content and Deep the popup lists, under the options and a rule, "All types" and every file type
  (`items::file_type`: the extension as written, `a.tar.gz` → gz; none for `Makefile` and `.bashrc`) of the
  rows listed, with how many rows have it, the most first, "(no extension)" last among equals. Space or a click
  turns one off or on, `a` (or the "All types" row) all of them; the list scrolls with the cursor.
- Off types leave the rows (`Filter::set_hidden_types`, any type, at once). In Content and Deep they also go to
  ugrep as `--exclude=*.EXT` (`App::list_plan`, before the pattern) and the search runs again, so Deep never
  reads them; an extension a glob would read otherwise (`[]*?{},!\`) is only left out of the rows. A type that
  is off stays in the popup with the count it had, to be turned on again.
- Each tab has its own (Bilal), none inside one file (Ctrl-T); they are not saved and a new root turns every
  type on again (Bilal). The input line says "1 type off" / "N types off". The types are those of the rows
  (Bilal: with a pattern, the files found; without one, every file listed), counted again only when the list
  changed (`App::types_seen`).

Ctrl-T in tmux, words, Match case and Help (2026-10-17, Bilal's four requests with his choices):

- Ctrl-T in tmux opens a window of its own, named "infile" as the fzf functions' was: `tmux new-window` runs
  `sf in KEY QUERY` (the query one argument, as typed) in the root through the fixed `sh -c` script; outside
  tmux sf narrows itself as before. `sf in FILE [WORDS...]` is also a command (`Cli::InFile`): sf starts
  inside the file, keeps the window's name and quits on Esc (`Scope::back` is None). tmux gives a new window
  its server's environment, not sf's, so sf passes the variables it reads (`actions::WINDOW_ENV`: config
  folder, bat's theme, editor, desktop session) with `-e`; not the whole environment, which `ps` would show
  on the command line with whatever secrets it holds. The e2e caught it: without them the window's sf would
  have read and written the real `~/.config/sf/config.toml`.
- Words: the fzf window ran ugrep with `a|b` and its fzf then kept the lines with every word, so in effect
  "all on a line"; sf had made it "any". Now ugrep's Boolean query: All words on a line (`--bool`) and All
  words in the file (`--bool --files`: files with every word, the lines with any of them), one or the other,
  in Content, Deep and inside a file (its own options, Ctrl-S inside it, saved as `[in_file]`). Default: on a
  line inside a file (as before in the fzf window), off in Content and Deep (Bilal: a space and parentheses
  mean something else in a Boolean query, which would surprise a regex). Both off: the pattern as it is,
  inside a file any of the words. Ctrl-O's vim gets the words as any of them (`App::editor_pattern`).
- Match case in every tab, off by default (Bilal: as before): off, a pattern has smart case and a filter
  none; on, ugrep loses `--smart-case` and the filter (`Filter::set_match_case`, also `--print` and Ctrl-F)
  compares as typed after its lower-case pass, the red marks too.
- Help, the last tab: the manual (`help.rs`) in columns: the tabs and their engines, the keys, the options,
  patterns and Boolean words, command lines from simple to advanced (each checked against the installed fd,
  ugrep and git, and a test parses every one as sf would), the engines. Typing finds in it (its own search,
  not the tabs' query), Enter the next, Esc clears then quits; ↑↓ PgUp PgDn and the wheel scroll. A test
  fails when an option is missing from it. `sf help` starts there.

The Ctrl-S popup's place (2026-10-17, Bilal: "under Config, at the right, never spilling; a column more on the
left when it is long"; he chose the reading order left to right):

- `ui::popup_geometry` (pure, unit-tested): the popup hangs under ⚙ Config at the right edge, between the
  tab bar and the input line. Lines one column cannot hold go on in more columns, balanced, the popup growing
  to the left with the first column leftmost; each column as wide as its own lines (the options' hints make
  the first wide, the types' columns stay narrow, so more of them fit); as many columns as the width holds,
  and what does not fit even so scrolls with the cursor. It never spills over the screen. ←/→ go to the row
  on the same line of the next or previous column; a click works in every column.
- Tried on real folders: /usr/lib/python3/dist-packages (55 types, two columns), /usr/share/doc (731 types,
  five columns at 200 × 50, the rest scrolling), 80 × 24 in Content (one column, at the right edge).

