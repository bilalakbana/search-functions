# sf: state and next steps (handoff, 2026-10-17)

Read this first in a new session, then `docs/sf-tui-design.md` (the approved spec plus every decision made while
building, see "Decided while building"). `docs/sf-tui-plan.md` and `docs/sf-tui-handoff.md` are history: the plan
was executed, the old handoff describes the state before that.

How Bilal starts the next session (paste):

> ~/.SearchFunctions'taki sf işine devam ediyoruz. Önce docs/sf-next-steps.md'yi oku (özellikle en alttaki
> "Next, asked 2026-10-17 after v0.8.1"), sonra docs/sf-tui-design.md'nin son bölümlerini. Oradaki A ve B
> işlerini sırayla yap: inline, alt agent olmadan, testleriyle, TDD. Commit/push/sürüm/silme için her
> seferinde benden onay al. Bana Türkçe kısa ara raporlar ver.

## State

- `~/.SearchFunctions/sf-rs`: the Rust app `sf` (ratatui 0.30, ~7300 lines incl. tests). Installed as
  `~/.cargo/bin/sf` (`cargo install --path ~/.SearchFunctions/sf-rs`); nothing else in PATH is named sf. The fzf
  prototype `~/.SearchFunctions/sf` still exists (README suggests `alias sf-fzf=…`).
- Git: `origin` (github.com/bilalakbana/search-functions, public) has Steps 1 to 4, the Deep tab and the command
  line; releases `v0.1.0` to `v0.13.0` (2026-10-17); later commits: `git log origin/main..main`.
- Bilal's config `~/.config/sf/config.toml` holds every option explicitly (sf writes all values); on 2026-10-17
  its hidden/ignored values were set to the new defaults (hidden false, ignored true) with his OK.
- Tests, all green on 2026-10-17 after Step 1:
  - `cd ~/.SearchFunctions/sf-rs && cargo test` → 240 passed, 2 ignored (speed).
  - `cargo test --release -- --ignored` → filter speed (1 M rows < 30 ms per query; measured 1–10 ms).
  - `tests/e2e.sh NEW_EMPTY_DIR` → 79/79: private tmux server (`-L sfrs-e2e`), fixture tree from
    `tests/tools/make_fixture.sh`, stubs for xdg-open/wl-copy/dbus-send/nvim/sub-functions and WSL's
    powershell.exe/wslpath, fresh `XDG_CONFIG_HOME`; WSL mode, OSC 52 through tmux, the commands, `sf doctor`.
  - `tests/platform_action.sh NEW_EMPTY_DIR` → 20/20: the fzf functions' wrapper (with sf, an old sf, no sf) and
    `sub-functions/file_list` in a private tmux server (`-L sfrs-platform-action`).
  - v0.2.0 (command line, `--print`, Ctrl-O's editor): `cargo test` 261; `tests/e2e.sh` 98/98.
  - Step 4 (Each match, numbered preview): `cargo test` 290; `tests/e2e.sh` 121/121; platform_action 20/20;
    sub_functions 7/7.
  - Deep tab: `cargo test` 306; `tests/e2e.sh` 131/131; platform_action 20/20; sub_functions 7/7.
  - v0.5.1 (Files without a preview, binary preview fixes, Deep preview speed): `cargo test` 316;
    `tests/e2e.sh` 132/132.
  - Ctrl-F filter and the key hints: `cargo test` 323; `tests/e2e.sh` 136/136.
  - File types in Ctrl-S (v0.6.0): `cargo test` 332; `tests/e2e.sh` 140/140.
  - Ctrl-T window in tmux, words, Match case, Help (v0.7.0): `cargo test` 355; `tests/e2e.sh` 149/149.
  - Ctrl-S popup under ⚙ Config in columns (v0.7.1): `cargo test` 362; `tests/e2e.sh` 149/149.
  - git keys in tmux windows, Whole files, Alt-F, Ctrl-T in views (v0.8.0): `cargo test` 374; `tests/e2e.sh` 156/156.
  - Ctrl-F finds in the preview, Ctrl-Alt-F filters (Alt-F is KDE's; v0.8.1): `cargo test` 375; `tests/e2e.sh` 156/156.
  - Ctrl-F as Notepad++'s Find (marks, count, Match case / Whole word / Regular expression; v0.9.0): `cargo test`
    399; `tests/e2e.sh` 161/161.
  - Whole word and Regular expression in Content/Deep, fzf's syntax in the filters (v0.9.0): `cargo test` 410;
    `tests/e2e.sh` 172/172; speed (release, ignored) 1 M rows ≤ 11 ms.
  - No Ctrl-T in views, Ctrl-F colours words, what/count at the preview's top right, Perl regex, Alt-↑↓ scroll
    while finding (v0.9.1): `cargo test` 418; `tests/e2e.sh` 173/173.
  - Pattern/count top right, words in the path, Git versions search (v0.10.0): `cargo test` 445;
    `tests/e2e.sh` 185/185.
  - Git file keys silent, Smart case, Ctrl-P files (v0.11.0): `cargo test` 453; `tests/e2e.sh` 191/191.
  - v0.12.0 (Search in versions named, file history while empty, version names, popup wheel, scroll
    bars dragged, Ctrl-P lists what shows): `cargo test` 462; `tests/e2e.sh` 192/192.
  - File types above Ctrl-P (v0.12.1): `cargo test` 463; `tests/e2e.sh` 194/194.
  - Ctrl-Shift-Alt-F apart from Ctrl-Alt-F (modified keys asked for; e2e tmux with extended-keys), the
    filter's own input line (v0.13.0): `cargo test` 467; `tests/e2e.sh` 195/195; `packaging/test_package.sh` all
    passed on Ubuntu 22.04, Debian 12, Ubuntu 24.04.
  - Step 2 (same day): `cargo test` 243; `tests/e2e.sh` 86/86 (with a release package's layout);
    `tests/sub_functions.sh NEW_EMPTY_DIR` 7/7 (`-L sfrs-sub-functions`); `python3 -I packaging/test_build.py` 7;
    `packaging/test_package.sh PACKAGE NEW_EMPTY_DIR` all passed on Ubuntu 22.04, Debian 12, Ubuntu 24.04.
  - Look at screens: `python3 -I tests/tools/ansi2png.py screens/X.ansi X.png` and read the PNG (Intel One Mono has
    no ⚙ glyph; that box is the font, not sf).
- Formatting: `cargo fmt` (Bilal OK'd it; his `format` skill has no Rust entry).

## Rules from Bilal that bit during this work

- Conversation Turkish; code, comments, commit messages English. Short replies, result first.
- **No `Co-Authored-By` / Claude / Opus / Fable in commit messages or PR texts**, also not in commands drafted for
  him (global CLAUDE.md + `~/.claude/settings.json` `attribution` since 2026-10-17).
- Git is the undo button (Bilal, 2026-10-31): use it freely (add, commit, branch, merge, tag, stash, push without
  force). Only risky steps ask him first: force-push, `reset --hard`, `clean`, `rebase`, `filter-branch`,
  `branch -D`, `stash drop/clear`, `checkout --`, `restore`, making a repository public or deleting it.
- Every deletion or overwrite of a file not just created: his OK for exactly those paths. Never put `rm` into a
  chained command (a stray `rm -f /dev/null` happened once).
- No subagents unless he says so (state count and cost first).
- Tests stub every desktop program (xdg-open, wl-copy, dbus-send, nvim, powershell.exe, wslpath…); an
  unstubbed `dbus-send` would open his real file manager. tmux tests only in a private socket
  (`tmux -L name -f /dev/null`), never his server.
- Verify flags against the installed tool (`--help`), and look at rendered screens, not text dumps.
- A new key for sf: check his tmux root table (`tmux list-keys -T root`) and KDE's global shortcuts
  (`~/.config/kglobalshortcutsrc`) first: either takes the key before sf sees it, and the e2e (`-f /dev/null`)
  cannot notice (Ctrl-F in tmux, Alt-F in KDE: rofi, 2026-10-17).
- The background security review runs on commits: fix real findings with a failing test first.

## Map of sf-rs/src

| File | What |
|---|---|
| `model.rs` | `Tab` {Files, Content, Deep, Folders, Git} (`from_name("search")` still → Content; `searches()`: Content and Deep), `Opt`, `Options` (serde; field `content` has `alias = "search"`; `theme` written only when not the default), defaults |
| `config.rs` | load/save `$XDG_CONFIG_HOME/sf/config.toml`; an invalid file is reported and never overwritten |
| `engines.rs` | finds fd/fdfind, bat/batcat, ugrep, ugrep+, git, delta, file, ls in PATH; `$BAT_THEME`; `bundle_dir` / `with_bundle` (a release package's `lib/sf`, first in PATH) |
| `commands.rs` | pure: `list_plan` (engine command per tab/options/query; `Search` says how Content or Deep searches) and `preview_plan` (Sniff/Numbered/Run/Pipe, Content/Deep `Header` with `ugrep -c -o` counts) |
| `items.rs` | compact rows (`Look::{Path, Match, Styled}`, ~160 B/row), parsers, `path_spans` (folders blue, `/` yellow, name green), `printable`, `ansi_text` (no escape gets through) |
| `filter.rs` | fzf `-i -e --no-sort` semantics with its extended search (`Query`: `!`, `^`, `$`, `\|`), parallel from 50 000 rows |
| `find.rs` | Ctrl-F's `Finder`: where a preview line holds the text, by `[find]` (Match case, Whole word, Regular expression through fancy-regex: Perl's syntax); `find_all` with a time budget |
| `runner.rs` | list child in its own process group (kill on cancel), bounded records (64 KiB), batches, sorting/numbering |
| `preview.rs` | preview worker (newest wins, kills the old), `is_text`, header lines, Content's numbered matches (`numbered_blocks`, `numbered_lines`) |
| `app.rs` | state machine: keys, mouse, effects; list fills bottom-up like fzf (Up → next row, Home → top) |
| `ui.rs` | drawing as fzf's `--style full`: top border with the root, tab bar with the keys at the right, list (bottom-up, highlight of typed words, scrollbars) beside the preview, the prompt box under both |
| `theme.rs` | the palette (256-colour indexes, fzf's by default) and its `[theme]` table; `init` once in `main`, one accessor per role |
| `actions.rs` | Enter/^O/^E/^C: open/reveal/copy through `platform.rs`, the editor (`find_editor`) in a tmux window via `sh -c 'cd -- "$1" && shift && exec "$@"'` (never `tmux -c DIR`: format injection), nvim pattern via `$SF_QUERY` |
| `platform.rs` | `Platform::detect()` / `from_parts()` (Linux or WSL, display, tmux, PATH), `open`, `reveal` (D-Bus FileManager1, fallback xdg-open on the folder; WSL Explorer), `copy_path`/`copy_text` (`Clipboard`: wl-copy, xclip, xsel, PowerShell, tmux OSC 52, OSC 52), `command()` for `sf open|reveal|copy-path|copy-text`, `launch()` |
| `doctor.rs` | `sf doctor`: the report from injected `run` (versions, D-Bus questions), `run_program` with a timeout |
| `cli.rs` | `sf [TAB] [QUERY] [--print\|--print0] [-- ENGINE-OPTIONS]`, the commands, `doctor`; refuses output-changing engine options |
| `print.rs` | `--print` / piped output (rows as the window draws them on a terminal, real bytes into a pipe) |
| `views.rs` | the views' git/delta plans (diff, history, log, commit, a file as it was in a version) |
| `versions.rs` | `sf grep-versions`: git grep over every version, documents through ugrep from `git cat-file`; the popup's versions (`git log --source`) |
| `help.rs` | the Help tab's manual (tables of tabs, keys, options, patterns, examples) |
| `main.rs` | arguments (`args_os`: tabs, the commands, `doctor`), event loop, foreground programs (SIGINT ignored meanwhile), tmux window name |

Outside sf-rs: `sub-functions/platform_action` (the fzf functions' Enter/^E/^C; sf when its usage names the
actions, else xdg-open/open_in_file_manager/wl-copy).

Facts learned (keep): ugrep Matching lines = `-n --null --separator=0x1f --replace=0x01%o0x02`; `fd -0` prefixes
`./`; tmux expands formats in `new-window -c` (and `new-session -c`) but not in `-e`, `-n`, `select-pane -T` or
program arguments; `##` escaping does not protect; ugrep+ is an `sh` script running `ug` (hence process groups).

## Next steps

### Step 1: one platform layer — done 2026-10-17

What was built and why is in `docs/sf-tui-design.md` → "Platform layer". In short: `platform.rs` (Linux or WSL;
open, reveal over D-Bus, copy by what the session has, OSC 52 last), `sf open|reveal|copy-path|copy-text`,
`sf doctor` (warns for ugrep < 3.6.0 and < 5.0.0, measured in Docker), and every fzf function's Enter / Ctrl-e /
Ctrl-c through `sub-functions/platform_action` (Bilal chose "all of them"). The prototype `~/.SearchFunctions/sf`
was left as it is (to be retired).

Still open from Step 1:

- Bilal, on Windows/WSL (only he can): install sf there and go through this list:
  1. `sf doctor`: Platform `WSL (<distro>)`, Windows `powershell.exe /mnt/c/…`, Paths `wslpath /usr/bin/wslpath`.
  2. In `sf` (Files tab): `Enter` on a `.txt` opens it in its Windows application.
  3. `Ctrl-E` opens Explorer with the file selected (also for a name with spaces, `,`, `&`, `é`).
  4. `Ctrl-C`, then paste in Windows: the Windows path (`\\wsl.localhost\<distro>\…` or `C:\…` under `/mnt/c`).
  5. Git tab, `Ctrl-C`: the hash is in the Windows clipboard.
  6. A fzf function (`f`): `Enter`, `Ctrl-e`, `Ctrl-c` do the same through `platform_action`.
  7. A name with `"` and `Ctrl-E`: sf refuses with "Explorer cannot select a name containing '"' …".
  If all works, the WSL repo (`~/.SearchFunctionsWSL`, github bilalakbana/search-functions-wsl) can retire.

Checked on this PC (2026-10-17): `sf reveal` on `a b,c/sf reveal test é.txt` opened Dolphin with the file selected
(Bilal saw it), so the percent-encoded D-Bus path works; `~/.cargo/bin/sf` is the Step 1 build, so the fzf
functions go through it.

This PC: KDE Plasma on Wayland, `org.freedesktop.FileManager1` activatable (running), `wl-copy` installed, no
xclip/xsel; Docker usable (Bilal is in group `docker`).

### Step 2: one release package — done and released as v0.1.0, 2026-10-17

What was built and why is in `docs/sf-tui-design.md` → "Release package". In short: `packaging/bundle.toml`
(pinned downloads), `packaging/build.py` (builds `sf-linux-x86_64.tar.gz` + `.sha256`, ugrep static in Alpine,
sf for musl, all licenses, static check, deterministic tar: two local builds gave the same sha256),
`packaging/test_package.sh` (clean Ubuntu 22.04 / Debian 12 / Ubuntu 24.04: doctor, e2e 86/86, sub-functions 7/7
on all three), `.github/workflows/release.yml` (tag `vX.Y.Z` → test, build, publish; run by hand → no publish),
sf's bundle lookup (`engines::bundle_dir`, `with_bundle`, `main::use_bundle`), self-relative sub-functions,
`show_diffs` → delta without git-split-diffs, README "sf in one line". No weekly version bump (Bilal: not now).

Released 2026-10-17 (all with Bilal's OK): `main` pushed at `02464b8`; a dry run of the workflow (run
38986892829) passed on GitHub (unit tests, build, package test on the three distributions); tag `v0.1.0` pushed,
its run (38987352934) published https://github.com/bilalakbana/search-functions/releases/tag/v0.1.0
(`sf-linux-x86_64.tar.gz` + `.sha256`). The README's one-line install was then tried as a new user in a clean
Ubuntu 24.04 container: sha256 OK, `sf` in `PATH` after login, `sf doctor` with the bundled engines. The Docker
images used for the tests were removed again.

Still open:

- v0.2.0 (same day, Bilal's OK): the command line (`sf content needle`, `-- ENGINE-OPTIONS`, `--print`,
  pipes), Ctrl-O's editor choice and the README's wget line (see the design doc's "Command line").
- Bilal on WSL: install with the README's line, then the Step 1 checklist above.
- Later, if wanted: a weekly workflow that bumps `bundle.toml` (would need "Allow GitHub Actions to create pull
  requests" in the repo settings), aarch64 packages. A new release: raise `version` in `sf-rs/Cargo.toml`,
  commit, push, then tag `vX.Y.Z` the same.

### Step 3: the terminal windows inside sf — done and released as v0.3.0, 2026-10-17

What and why: `docs/sf-tui-design.md` → "Step 3". Ctrl-U/Ctrl-G/Alt-G/Alt-U and Enter in Git are views inside sf
(`views.rs`: git through delta side by side, `/` search, Esc back); Ctrl-T narrows Content to the file (any of the
typed words); sf runs no script any more, the package drops fzf and the scripts (12 MB). Tests: `cargo test` 277,
`tests/e2e.sh` 114/114 (real git and delta for the views). Released as v0.3.0 (Bilal's OK).

The engines (fd, ugrep, bat, git, delta) stay external on purpose: reimplementing them would change the regex
dialect and lose ugrep+'s archive/PDF search.

### Step 4: advancedsearch / detailedsearch in sf — done and released as v0.4.0, 2026-10-17

What and why: `docs/sf-tui-design.md` → "Step 4". The Content option **Each match** (detailedsearch: a row per
match with up to 50 columns around it, `ugrep -o -B50 -A50`, archives and documents too; it and Matching lines
turn each other off), and Content's preview numbers the matches in the form Bilal chose from a mockup ("with
context, numbered"): "Match #N" blocks with 2 lines around each matching line, the line marked `:`, its matches
red, a rule between the blocks. advancedsearch's file list was already Content (Archives & documents for `_plus`;
`sf content P -- -i --exclude=x` for its case-insensitivity and excludes).

Released as v0.4.0 (Bilal's OK). Still open: the Step 1 WSL checklist.

### The Deep tab — done and released as v0.5.0, 2026-10-17

Bilal wanted advancedsearch/detailedsearch as a tab of their own, because they search binaries (dll, exe) that
Content leaves out. What and why: `docs/sf-tui-design.md` → "The Deep tab". Files · Content · **Deep** · Folders ·
Git; Deep = advancedsearch + `_plus` (option Documents) + detailedsearch (option Each match) under one Config,
preview as Content's (numbered, with context; Bilal's choice); Content keeps its own Each match and Archives &
documents (Bilal). Released as v0.5.0 (Bilal's OK). "^S options" in the tab bar and Ctrl-F (filter the
results after a fixed pattern) followed in v0.5.2 (Bilal's OK), see the design doc's "After v0.5.0".

## Deferred minors (known, not fixed)

- Entering a folder that vanished reports "<program>: No such file or directory" instead of the folder.
- A root path longer than the border is cut at its right end (the folder's name).
- "wl-copy failed (exit 1): " ends in ": " when there is neither an argument nor a stderr line.
- Git collects the whole log before showing it (numbering); slow on very large repositories.
- The tmux window name is not restored when sf is killed by a signal.
- Ctrl-O on a matching line inside an archive member opens nvim on the archive at that line number.
- Rows keep a lower-case copy even when the key is already lower case (~50 MB on $HOME).
- Engines run in their own process groups and outlive a SIGHUP'd sf until their next write.
- The prototype `~/.SearchFunctions/sf` and the old shell functions still use `nvim -c "/QUERY"`-style code paths
  (the prototype's Ctrl-O weakness with a crafted query; tmux `-c` was fixed in the prototype).
- The prototype `~/.SearchFunctions/sf` still calls xdg-open / open_in_file_manager / wl-copy itself (Linux only).
- `cargo clippy` names 6 older spots (collapsible `if`s in app.rs, ui.rs, main.rs, preview.rs, runner.rs; a manual
  range check in a preview test); none in the Step 4 code.

## Next, asked 2026-10-17 after v0.8.1 (the session ended at a full context)

Done the same day (Bilal's OK for each): A and B below, commits fedd98f, 54902b8, 63ba453 (sf 0.9.0) pushed; CI dry
run 39040172823 (410 unit, e2e 172/172 on the three distributions), tag `v0.9.0` (lightweight, the earlier ones
are annotated "sf X.Y.Z"; the workflow only reads the name) published by run 39040468074; downloaded from the
README's `releases/latest` URL: sha256 OK, `sf doctor` says 0.9.0, the regex crates' licenses in the package.
`~/.cargo/bin/sf` is 0.9.0. Still open: `~/.dot-files` a4ee358 not pushed; this paragraph is uncommitted.

State at the hand-over:
- sf 0.8.1: commits 8e19561 + 4d7b20d pushed; released as v0.8.1 (run 39036722844), downloaded from the
  README's `releases/latest` URL, sha256 OK, `sf doctor` says 0.8.1: verified before the hand-over.
  This hand-over note itself (docs/sf-next-steps.md) is uncommitted: commit it with his OK.
  (Its dry run failed once on a DNS error of GitHub's runner while fetching a license text; `gh run rerun`
  passed: a network hiccup, not our code.)
- `~/.cargo/bin/sf` is 0.8.1. `~/.dot-files` commit a4ee358 (tmux Ctrl+F only in Claude Code panes) is
  NOT pushed (master ahead 1): push only with his OK.
- Tests at the hand-over: `cargo test` 375, `tests/e2e.sh` 156/156, platform_action 20/20, sub_functions 7/7;
  clippy: the 6 old warnings only.
- Keys now: Ctrl-F find in the preview (every tab; app.rs `Find`, `find_key`, `find_in_preview`; ui
  `draw_preview` bands the found line), Ctrl-Alt-F Content/Deep result filter (Alt+F is KDE's rofi launcher),
  Ctrl-S options + file types (popup at the right edge, `ui::popup_geometry`), Ctrl-T in file (tmux: window
  `sf in`), Ctrl-G/Alt-G/Ctrl-U/Alt-U views (tmux: windows `sf diff|history|log`), Help tab (help.rs).

A. Ctrl-F, find in the preview, as Notepad++'s Find (Bilal, 2026-10-17) — built 2026-10-17, uncommitted; what and
why: `docs/sf-tui-design.md` → "Ctrl-F as Notepad++'s Find" (his choices: all three options, saved, red marks).
The ask was:
- He asked whether Files and Content need it. Answer to give: it finds inside the SELECTED document's preview
  (Files: the file itself; Content: its numbered matches; Git: the commit), which neither tab's own search
  does; he liked that, but:
- It highlights no word: only the found line gets a band. Mark the typed text red in every visible line
  (all occurrences, the current one stronger), and show a count ("3/17").
- Give Ctrl-F options of its own, like Notepad++/Notepadqq's Find: at least Match case, Whole word, Regular
  expression; maybe Wrap around (now always on), direction. Research Notepad++'s Find dialog first (official
  docs: npp-user-manual.org "Searching"; Notepadqq on Linux); he allowed web search and Brave
  (Claude in Chrome tools). Then a short proposal with a mockup and AskUserQuestion before building.
  Where the options live: probably a Ctrl-S popup while finding (as inside a file, `in_file`), saved.
- Mind: crossterm/tmux/KDE key conflicts (memory feedback-sf-keys-vs-tmux-root: check `tmux list-keys -T
  root`, `~/.config/kglobalshortcutsrc`, `ghostty +list-keybinds --default`).

B. Built 2026-10-17, uncommitted; what and why: `docs/sf-tui-design.md` → "Other search tools' options" (his
choice: Whole word, fzf's syntax in the filters, Regular expression in Content/Deep; not Newest first). The ask was:
After A: what other search tools offer in their options (ripgrep/ugrep flags, fzf, VS Code's search
  panel: include/exclude globs, whole word, regex, preserve case; Everything; Notepad++ Find in Files;
  Double Commander / Dolphin search), and propose the few that would help sf. Bilal's taste: minimal, no
  feature overload; give a recommendation with reasons and one alternative each; ask before building.

## Live-sweep defects (card t_7c990be7, 2026-10-31, on vpsbilal)

Fixed, one commit each (details in `docs/sf-tui-design.md`, "Command line" and "After v0.5.0"):

1. Files shows its preview at once (no Ctrl-L); a preview asked for before the frame that lays it out is
   asked for again at the real width (bat had wrapped at 20 columns). Corpus `shapes/` checked in tmux: text,
   CRLF, no final newline, UTF-8, empty, binary (`file -b`), NUL bytes, 1 MB line, 300k lines; nothing hangs.
2. `--print0`: a NUL after each path (fd `-0`, ugrep `-0`); a newline in a name survives `xargs -0`.
3. Paths ugrep cannot read: one line, "2 paths could not be read", in the window and with `--print` (exit 2).
4. An empty Git list says why: "this repository has no commits yet" / "bare repository, no commits yet".
5. UTF-16LE with a byte order mark: no defect, ugrep reads it as text; the corpus file `shapes/utf16le.txt`
   holds only "hello" in UTF-16 (its " needle" is ASCII bytes). An e2e case guards it.

Second round (same card):

6. `tests/platform_action.sh` puts `fd`'s folder in its fzf pane's PATH (on vpsbilal `fd` is only in
   `~/.local/bin`, so `file_list` listed nothing).
7. A long text file's preview sends its first screen (200 lines or 64 KiB of bat's output) at once and the
   whole text (up to 4 MiB, as before) after it, same generation; the scroll stays when the rest comes.
   Previews with a count (Content, Deep) come whole, so their header never flickers. First paint, release,
   140×40 tmux, from `sf files NAME` to the first line: `huge.txt` (17 MB) 363–401 → 94–109 ms,
   `longline.txt` (1 MB) 122–165 → 82–129 ms, `utf8.txt` 78–96 → 76–93 ms.

Tests after the card: `cargo test` 479 passed, 2 ignored; `tests/e2e.sh` 215/215; `tests/sub_functions.sh`
7/7; `tests/platform_action.sh` 20/20; `cargo clippy`: the 6 old warnings only. On vpsbilal (2 CPUs) the
release speed test `speed_1m_items` takes 31–54 ms against its 30 ms limit, before the card too.
vpsbilal has no `file`, `delta` or `zip`: the runs used them unpacked from Debian's packages, through a
release package's layout (`bin/sf` beside `lib/sf`); `sudo apt install file zip git-delta` makes that needless.

Still open:
- Matching lines and Each match split ugrep's records at a newline, so a newline in a path loses its first
  part (window, `--print`, `--print0`); file lists are fine.
- A long file's preview comes in two steps (first screen, then the whole text up to 4 MiB), not as a stream:
  bat still makes the whole 4 MiB before the rest shows (~400 ms for 17 MB), and Content's and Deep's
  previews with a count still wait for all of it. Kept so (card t_264a5864): the reasons and what a stream
  would need are in `docs/sf-tui-design.md`, "Decided while building".

## The fzf look (card t_74cd7243, 2026-10-31, on vpsbilal)

The main screen looks as Bilal's fzf screen (`f()` in `find_files`: `fzf --style full --border top
--border-label`, measured from a 150×42 tmux pane). Keys, tabs, output, the preview worker and the views
did not change. The views keep their full-screen layout.

```
row 0     ──────────────────  ~/Documents/Projects/app  ──────────────────   top border, root in the middle
row 1      Files  Content  Deep  Folders  Git  Help      Enter open · ^S options  ⚙ Config
row 2     ╭──────────────────────────────╮╭──────────────────────────────╮
          │   src/lib.rs                 ││ 1  needle in main            │   list: left half (w/2)
          │▌ src/main.rs                 ││ 2  second line               │   preview: right half
row h-4   ╰──────────────────────────────╯╰──────────────────────────────╯   both boxes end here
row h-3   ╭─────────────────────────────────────────────────────────────╮
row h-2   │ > Files › type to filter file names            ignored 3/40 │   prompt box, full width
row h-1   ╰─────────────────────────────────────────────────────────────╯
```

- Top border: `─` in the border colour, the root (`~` for the home folder, path colours) with one space on
  each side, in the middle. A root wider than the pane is cut at the right.
- Tab bar: no block and no border. The selected tab is bold in the prompt colour, the others in info. The key
  hints sit at the right, before ⚙ Config. ⚙ Config has the accent background while the popup is open.
- List and preview side by side: the preview's left edge at column `w/2`. The root is no longer on the
  preview's border: the top border shows it. The preview keeps
  `pattern · N matches` and Ctrl-F's count at its top right. The status ("Copied …", errors) is on the
  preview's bottom border, or on the list's when the preview is hidden.
- Prompt box: `>` in the prompt colour, then the same text as the old input line; the counter in info at the
  right, the loading ` …` in the spinner colour. On a narrow screen the hint and the options are cut with
  `…`; the typed text, ` · pattern: X` and the counter stay.
- Ctrl-L (unchanged) hides the preview: the list takes the full width, the prompt box stays. Help: the manual
  above the prompt box, full width.
- The Ctrl-S popup hangs from row 2 and ends above the prompt box (3 rows fewer per column than before). At
  90×24 the options and a few file types fit. With more types it scrolls, as on small screens before. Its
  frame keeps the accent colour (161), so it stands out from the boxes.
- The whole main screen has the bg colour. A cell with no colour of its own gets fg/bg: also where an
  engine's output (bat, git) resets its colours, as fzf does. A colour an engine sets (delta's) stays.

Palette (`theme.rs`), the defaults as measured; each one can be changed in `~/.config/sf/config.toml`:

```toml
[theme]
fg = 252          # body text, list text, the typed query
fg_sel = 252      # the selected row's text
bg = 234          # the screen's background
bg_sel = 235      # the selected row's background
hl = 5            # matched words: the filter's, Ctrl-F's, the preview's
hl_sel = 5        # matched words in the selected row
info = 144        # counter, key hints, secondary labels, dim explanations
border = 240      # every border, the top border's fill, faint text
prompt = 161      # >, the selected tab, titles, the accent (scroll bar thumb, popup frame)
pointer = 161     # the ▌ in column 1 of the selected row
marker = 118      # a success in the status
spinner = 118     # the loading …
header = 168      # section headers (Help, the popup's rules), the preview's found title, Match #N
path_dir = 4      # folders in a path (also a version's hash)
path_slash = 3    # every / (also a version's name and branch)
path_name = 2     # a path's last part
error = 1         # sf's own errors (a match is hl, not error)
```

All keys are optional, integers 0–255. A value outside that range is a config error, like any other invalid
value. While the file has an error, sf does not save over it. A custom theme is saved with the options.
The default theme is not written. Tests never set the global palette: they see the defaults.

Tests after the card:

- `cargo test`: 486 passed, 2 ignored. New tests: the palette defaults; `[theme]` partial, invalid and saved;
  the screen at 150×42 and 90×24 (top border, `╮╭` and `╯╰` at `w/2`, the prompt box the last 3 rows, `>` and
  the counter, the selected row bg 235 and `▌` in column 1, no `Reset` background); the popup at 90×24; a
  narrow prompt box. The old drawing tests read the new rows.
- `tests/e2e.sh`: 226/226. New checks: the top border (only `─` and the root, in the middle ±1), `╮╭` at
  `w/2 - 1`, preview text right of it, the boxes ending at h-4, the prompt box the last 3 lines across the
  width with `>` and `N/M`; the same with the preview hidden and shown again; a 90×24 capture
  (`screens/narrow.ansi`); `38;5;240` borders by default and `38;5;99` with a separate `XDG_CONFIG_HOME`
  that holds `[theme] border = 99` (its file stays untouched). `red_on` looks for `38;5;5m`.
  "views: Ctrl-G shows the change" waits for delta's `│changed by e2e` now. Files' preview shows that line too
  (bat), so the check could pass before the view's text came. The search typed next then found nothing:
  1 run of 5 failed so. Green 3 times in a row after the change (226/226).
- `tests/platform_action.sh` 20/20; `tests/sub_functions.sh` 7/7; `cargo clippy --all-targets`: the 6 old
  warnings only; `cargo fmt --check` clean.
- How the scripts run on vpsbilal: see "How to run the tests on vpsbilal" in the section of card t_264a5864
  below.

Open:
- A scratch git worktree of e4ae243 stays under Claude Code's scratchpad
  (`~/.hermes/profiles/developer/cache/scratch/…/scratchpad/base`, built in `…/base-target`). The guard
  counts that folder as protected and Bilal said to leave it. `git worktree list` shows it. If the folder goes,
  `git worktree prune` clears the entry.

## Empty files and test runs without engines (card t_264a5864, 2026-10-31, on vpsbilal)

Fixed, one commit each:

1. An empty file's preview (Files; Content and Deep without a pattern) shows one dim line, `empty file`.
   Before, it said "No text preview: binary file". `is_text` still calls an empty file no text; the preview
   reads one byte to tell an empty file from a binary.
2. Without `file`, a binary's preview says `No text preview: binary file (install `file` for its type)`.
3. `tests/tools/make_fixture.sh` builds `docs/arch.zip` with `zip`, or with `python3 -m zipfile` when `zip` is
   missing (the same member `inzip.txt`). Without both, it stops and names them. Before, a missing `zip` left
   `inzip.txt` at the root and no archive: 18 e2e checks failed on counts.
4. `tests/e2e.sh`:
   - The pane's PATH is `$STUBS:/usr/local/bin:/usr/bin:/bin:` and then the caller's PATH, in one variable
     (`PANE_PATH`, quoted for WSL's spaces). A run gets delta or file through PATH.
   - `fd` or Debian's `fdfind`, found once (`$FD`), for the file count and the bundle link. Neither: the run
     stops.
   - Before tmux starts, `sf doctor` with the pane's PATH says whether delta and file are found. A missing
     engine's check prints a `SKIP` line, and the run checks what sf shows instead: git's own `+changed by e2e`
     for delta; the install hint for file (file must also run `file -b /dev/null`). The last line is
     `pass=N fail=N skip=N`; only fail sets the exit code.
   - New checks: an empty file in Files (`│empty file`), `src/blob.bin` in Files, and a name with a newline
     (`x<LF>y.txt`, in its own folder): one row in Content and in Deep, and Enter opens it (the OPEN stub logs
     both halves of the path).
5. The two-step preview stays (a stream needs more than 40 lines): reasons in `docs/sf-tui-design.md`,
   "Decided while building".

Tests after the card:

- `cargo test`: 487 passed, 2 ignored (one new test; the old binary-file test expects the new text).
- Run (a), zip and delta through PATH: `pass=237 fail=0 skip=1` (the skip: file).
- Run (b), `env -i HOME=$HOME PATH=/usr/local/bin:/usr/bin:/bin`: `pass=237 fail=0 skip=2` (file, delta).
  Before the card: 195 pass, 31 fail.
- `tests/platform_action.sh` 20/20; `tests/sub_functions.sh` 7/7; `cargo clippy --all-targets`: the 6 old
  warnings only; `cargo fmt --check` clean.

How to run the tests on vpsbilal (no system `zip`, `file` or `delta`):

```
C=~/workspace/sf-live/t_264a5864        # tools-a/ holds links named zip and delta
cd ~/.SearchFunctions/sf-rs && cargo build
PATH=$C/tools-a:$PATH bash tests/e2e.sh $C/<new empty dir>               # with delta
env -i HOME=$HOME PATH=/usr/local/bin:/usr/bin:/bin bash tests/e2e.sh $C/<new empty dir>   # Debian only
```

The links point to Debian's binaries, unpacked in `~/workspace/sf-live/card-t_7c990be7/tools/root/usr/bin/`.
The `file` there does not run (`libmagic.so.1` is missing). Keep the workdir path short: at about 150
characters, path-dependent lines are cut at 160 columns. `sudo apt install file zip git-delta` makes the links
needless. `SF_STRICT_SKIPS=1` makes a skip a failure; the release runs use it, a local run should not need it.

Open:
- The e2e check `No text preview: data` (file found and working) did not run: vpsbilal has no working `file`.
  Run it once on a machine with `file`, e.g. pcbilal.
- The fixture's branch "neither zip nor python3" did not run: the guard denied the test that hid both tools
  (links made with `ln -sf`).
- Still open from card t_7c990be7: Matching lines and Each match split ugrep's records at a newline in a path.

## Release 0.13.0 (card t_018e8b5b, 2026-10-31)

### Speed ratio (before a tag)

```
cd ~/.SearchFunctions/sf-rs && cargo build --release && bash tests/speed_ratio.sh
```

- What it measures, on a tree of 100,000 files: the time from Enter in a waiting bash (a private tmux pane,
  140x40, made before the timer starts) to the first file name on the screen, then `sf files --print` on the same
  tree (wall clock), back to back, so each pair sees the same load.
- The rule (Bilal, 2026-10-31): one warm-up run, then 5 runs; the **overhead** is the median TUI time minus the
  median `--print` time, and it fails (exit 1) above `SF_SPEED_OVERHEAD_MS` (default 150 ms). The ratio is printed
  for information only.
- Why not the ratio: Files collects the whole list before the first frame (sorted; `read_items` in
  `src/runner.rs`), so the TUI time is the list time plus sf's own start and first frame. A ratio divides a ~40–80
  ms constant by a baseline that moves with the tree and the load; on this VPS it sat exactly at the old 1.5 limit
  and flipped between pass and fail on an unchanged tree (1.34, 1.57, 1.54 in three runs). The overhead is the part
  a change in sf can make worse, and it stayed at 79 ms in the run that produced ratio 1.58.
- The script makes its own tree at `sf-rs/target/speed-ratio-tree` (100,000 empty files `f000000` … `f099999`)
  unless it is given one: `bash tests/speed_ratio.sh TREE`. It needs tmux and a release build.
- The card's "first row ≈170 ms" was a rough poll, not this harness; the harness adds its own ~16 ms.
- Measured 2026-10-31 23:20 on vpsbilal (2 vCPU), after the switch to the overhead rule:

  | Run | TUI / `--print` (ms) | Ratio | Overhead |
  |---|---|---|---|
  | warm-up | 193 / 123 | 1.57 | 70 ms |
  | 1 | 211 / 162 | 1.30 | 49 ms |
  | 2 | 241 / 127 | 1.90 | 114 ms |
  | 3 | 186 / 141 | 1.32 | 45 ms |
  | 4 | 215 / 124 | 1.73 | 91 ms |
  | 5 | 223 / 136 | 1.64 | 87 ms |

  Median: TUI 215 ms, `--print` 136 ms → **overhead 79 ms (limit 150) PASS**; the ratio of the medians is 1.58,
  and the median of the 5 run ratios (the old rule's number) is 1.64, which the old 1.5 rule would have failed on
  the same unchanged tree.
- Earlier the same day, with three stale debug sf processes using about 30 % of one CPU (load 0.58–0.94), the
  median ratios were 1.41, 1.54 and 1.55 — the same flapping.

### WSL

`sf doctor` warns when the search root (sf's current folder) is `/mnt` or under `/mnt/`, as WSL mounts Windows
drives there; the numbers wait for Bilal's WSL machine.

### Scale measurements

Measured 2026-10-31, 22:41–22:44, on vpsbilal (2 vCPU, 3.8 GiB RAM, load average 0.8–2.1 during the runs) with
the release build `sf-rs/target/release/sf` (sf 0.13.0, HEAD 0db7d42) and the system's engines (no delta in
PATH: diffs are git's own). Each case: one warm-up run, then 3 runs, each shown; the median is the middle one.

How each number was taken (the method of `tests/speed_ratio.sh`): a private tmux server (`-L sf-scale -f
/dev/null`), a 140x40 pane with a waiting `bash --noprofile --norc` made before the timer starts, a fresh
`XDG_CONFIG_HOME` per run, a new sf per run. A TUI time runs from the send-keys call (Enter, or the key) to a
pattern in a capture of the pane, polled every ~5 ms; the harness alone takes about 16 ms (12–30), so a time
near 16 ms means "at the first poll". A `--print` time is the wall clock of the plain command, stdout to
`/dev/null`. Peak memory is the kernel's VmHWM of the process. The scripts, kept for repeating: in
`/home/bilal/workspace/sf-live/scale-tools/` `case_1m.sh`, `case_git.sh`, `case_bigline.sh` (helpers `lib.sh`,
`peak_mem.py`); their output is in `results-1m.txt`, `results-git.txt`, `results-bigline.txt` there.

**1. A flat folder of 1,000,000 empty files** (`/home/bilal/workspace/sf-live/scale-1m`, `f0000000` …
`f0999999`, made in 58 s with `seq -f 'f%07.0f' 0 999999 | xargs touch`; it costs 1,000,000 inodes and a
22,642,688-byte directory, no data blocks; free before: 34 GB, 3.4M inodes)

| What | Runs (ms) | Median |
|---|---|---|
| list: `sf files --print` | 2162, 2228, 2052 | 2162 ms |
| filter: `sf files f09999 --print` (100 rows: `f0999900` … `f0999999`) | 2011, 1949, 2044 | 2011 ms |
| TUI first row: Enter on `sf files` to the first `f0nnnnnn` on screen | 2410, 2218, 2206 | 2218 ms |

Peak memory: `sf files --print` sf 141.2 MiB, fd 29.7 MiB; the window with all 1,000,000 rows listed, sf
142.3 MiB. The TUI's first row over the list time is 1.03 here (at 100k files the median run ratios were
1.41–1.55, see "Speed ratio"): sf's own start weighs less at 1M.

**2. A repository with 100,000 commits** (`/home/bilal/workspace/sf-live/scale-git/repo`, built in 17 s by
`python3 -I /home/bilal/workspace/sf-live/scale-git/make_repo.py repo 100000` run in `scale-git/`, through
`git fast-import`: one branch `main`, 100 files `src/file_00.txt` … `src/file_99.txt` of 20 lines, each commit
changes one line of one file, so each file has 1,000 commits; 400,000 objects in one 51.84 MiB pack as
fast-import wrote it, no gc; `.git` 52 MB)

| What | Runs (ms) | Median |
|---|---|---|
| Git tab first row: Enter on `sf git` to a numbered commit row | 2548, 2550, 2543 | 2548 ms |
| Enter on the newest commit to the commit view (its `╭ commit <hash>` border) | 14, 19, 14 | 14 ms |
| Ctrl-U on `src/file_00.txt` in Files (the file's history with its changes, a tmux window) to its first commit | 1964, 2037, 2041 | 2037 ms |

- Ctrl-U on a commit in the Git tab does nothing, by design: the file keys are not bound in Git (`src/app.rs`,
  "a file's keys are not bound in Git (Bilal): nothing happens, nothing is said"). Checked: the screen was the same
  2 s after Ctrl-U, and no tmux window opened. The card's "Ctrl-U history" is a file's key, so the third row
  measures it on a file.
- Peak memory: the Git tab with 100,000 rows listed, sf 120.1 MiB.
- One exploratory run right after building the two cases (load 2.45) took 4325 ms to the Git tab's first row:
  the number moves with the load.

**3. One line of 50 MiB** (`/home/bilal/workspace/sf-live/scale-bigline/bigline.txt`, 52,428,800 bytes of `a`,
no newline, made with `head -c 52428800 /dev/zero | tr '\0' a`)

| What | Runs (ms) | Median |
|---|---|---|
| Files, Enter on `sf files` to the preview's first screen (a row of 60 `a`) | 442, 551, 508 | 508 ms |
| 2,000 Alt-Down in one `tmux send-keys -N 2000 M-Down` to `… preview cut at 4 MiB` on screen | 15, 17, 18 | 17 ms |

- Before the scroll the preview shows `1 aaaa…`, the line wrapped at 58 `a` per row; after it, the preview's top
  row is `… preview cut at 4 MiB` (the scroll stops at the last line). Each Alt-Down scrolls one preview page;
  1,000 pages were not enough in a trial, 2,000 were. The scroll keys were sent 6 s after the first screen, when
  the whole 4 MiB had come (the cut line belongs to it); the time until the whole text comes was not measured.
- 17 ms is the harness's floor: sf takes the 2,000 keys and draws within one poll.
- Peak memory: sf 25.3 MiB after the scroll. bat's own peak was not measured (it is a short-lived child).
- Not explained: in an 80x24 pane, 7,000 Alt-Down never showed the cut line, while 140x40 reached it with
  2,000. Nothing hung. A lead, not a finding: `scroll_preview` in `src/app.rs` casts the clamped position with
  `as u16`, so a preview of more than 65,535 rows would wrap; a two-step test of that (3,856 pages down, past
  row 65,535 if a page is 17 rows as the 80x24 layout gives; then one up; look for row 1) did not show the wrap.

Nothing panicked, hung or ran out of memory. Not measured: WSL (no WSL machine here), delta's diffs (no delta in
PATH), the time until a long preview's whole text has come.

Folders left for Bilal to delete when he no longer needs them: `/home/bilal/workspace/sf-live/scale-1m`
(1,000,000 files), `/home/bilal/workspace/sf-live/scale-git`,
`/home/bilal/workspace/sf-live/scale-bigline` (50 MiB), `/home/bilal/workspace/sf-live/scale-tools` (scripts,
results and the per-run config folders in `work/`).

## Release 0.13.3 (2026-11-01)

Content: the fixes of the OCR review of `v0.12.1..v0.13.2` (tmux `extended-keys-format xterm` lost Shift-Tab,
Ctrl-Alt-F and Ctrl-Alt-H; `sf doctor`'s `/mnt` warning only in WSL; help rows) and two more review rounds with
fixes (`96a2432..401964a`; the third round found nothing). Notes: `CHANGELOG.md` `## [0.13.3]`.

- Test run without a tag: run 41298189486 on `1e365f4`, green. Release run: 41298621497 on tag `v0.13.3`, green;
  it published `sf-linux-x86_64.tar.gz` (13,385,500 bytes) and its `.sha256`.
- The package test now runs in four images. Ubuntu 22.04, Debian 12 and Ubuntu 24.04 (tmux before 3.5)
  gave `pass=252 fail=0 skip=0`; Debian 13 (tmux 3.5a, with the xterm-form checks) gave `pass=266 fail=0 skip=0`.
- Checked after the release: the downloaded package matches its sha256, and its `bin/sf --version` prints
  `sf 0.13.3`. `~/.cargo/bin/sf` on vpsbilal is 0.13.3 (`cargo install --locked --path sf-rs`).
- Open: `.github/workflows/release.yml` (the comment on line 2 and the step name) still says three images. The
  narrow token cannot push workflow files, so Bilal changes these lines with his next workflow change.

