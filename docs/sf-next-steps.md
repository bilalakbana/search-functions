# sf: state and next steps (handoff, 2026-10-17)

Read this first in a new session, then `docs/sf-tui-design.md` (the approved spec plus every decision made while
building, see "Decided while building"). `docs/sf-tui-plan.md` and `docs/sf-tui-handoff.md` are history: the plan
was executed, the old handoff describes the state before that.

How Bilal starts the next session (paste):

> ~/.SearchFunctions'taki sf işine devam ediyoruz. Önce docs/sf-next-steps.md'yi oku, sonra sf-tui-design.md'yi.
> <yapılacak iş: "Next steps"in sonunda açık kalanlar veya yeni bir istek>. Inline, alt agent olmadan, testleriyle.
> Commit/push/silme için her seferinde benden onay al. Bana Türkçe kısa ara raporlar ver.

## State

- `~/.SearchFunctions/sf-rs`: the Rust app `sf` (ratatui 0.30, ~7300 lines incl. tests). Installed as
  `~/.cargo/bin/sf` (`cargo install --path ~/.SearchFunctions/sf-rs`); nothing else in PATH is named sf. The fzf
  prototype `~/.SearchFunctions/sf` still exists (README suggests `alias sf-fzf=…`).
- Git: `origin` (github.com/bilalakbana/search-functions, public) has Steps 1 to 4, the Deep tab and the command
  line; releases `v0.1.0` to `v0.6.0` (2026-10-17); later commits: `git log origin/main..main`.
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
- Every commit, push, tag, branch, rebase: his explicit OK per action. `git commit` from Claude is blocked by the
  auto-mode classifier: stage nothing, give him the exact `! cd ~/.SearchFunctions && git add … && git commit -m …`.
- Every deletion or overwrite of a file not just created: his OK for exactly those paths. Never put `rm` into a
  chained command (a stray `rm -f /dev/null` happened once).
- No subagents unless he says so (state count and cost first).
- Tests stub every desktop program (xdg-open, wl-copy, dbus-send, nvim, powershell.exe, wslpath…); an
  unstubbed `dbus-send` would open his real file manager. tmux tests only in a private socket
  (`tmux -L name -f /dev/null`), never his server.
- Verify flags against the installed tool (`--help`), and look at rendered screens, not text dumps.
- The background security review runs on commits: fix real findings with a failing test first.

## Map of sf-rs/src

| File | What |
|---|---|
| `model.rs` | `Tab` {Files, Content, Deep, Folders, Git} (`from_name("search")` still → Content; `searches()`: Content and Deep), `Opt`, `Options` (serde; field `content` has `alias = "search"`), defaults |
| `config.rs` | load/save `$XDG_CONFIG_HOME/sf/config.toml`; an invalid file is reported and never overwritten |
| `engines.rs` | finds fd/fdfind, bat/batcat, ugrep, ugrep+, git, delta, file, ls in PATH; `$BAT_THEME`; `bundle_dir` / `with_bundle` (a release package's `lib/sf`, first in PATH) |
| `commands.rs` | pure: `list_plan` (engine command per tab/options/query; `Search` says how Content or Deep searches) and `preview_plan` (Sniff/Numbered/Run/Pipe, Content/Deep `Header` with `ugrep -c -o` counts) |
| `items.rs` | compact rows (`Look::{Path, Match, Styled}`, ~160 B/row), parsers, `path_spans` (folders blue, `/` yellow, name green), `printable`, `ansi_text` (no escape gets through) |
| `filter.rs` | fzf `-i -e --no-sort` semantics, parallel from 50 000 rows |
| `runner.rs` | list child in its own process group (kill on cancel), bounded records (64 KiB), batches, sorting/numbering |
| `preview.rs` | preview worker (newest wins, kills the old), `is_text`, header lines, Content's numbered matches (`numbered_blocks`, `numbered_lines`) |
| `app.rs` | state machine: keys, mouse, effects; list fills bottom-up like fzf (Up → next row, Home → top) |
| `ui.rs` | drawing: tab bar with the keys centred, preview, list (bottom-up, highlight of typed words, scrollbars), input |
| `actions.rs` | Enter/^O/^E/^C: open/reveal/copy through `platform.rs`, the editor (`find_editor`) in a tmux window via `sh -c 'cd -- "$1" && shift && exec "$@"'` (never `tmux -c DIR`: format injection), nvim pattern via `$SF_QUERY` |
| `platform.rs` | `Platform::detect()` / `from_parts()` (Linux or WSL, display, tmux, PATH), `open`, `reveal` (D-Bus FileManager1, fallback xdg-open on the folder; WSL Explorer), `copy_path`/`copy_text` (`Clipboard`: wl-copy, xclip, xsel, PowerShell, tmux OSC 52, OSC 52), `command()` for `sf open|reveal|copy-path|copy-text`, `launch()` |
| `doctor.rs` | `sf doctor`: the report from injected `run` (versions, D-Bus questions), `run_program` with a timeout |
| `cli.rs` | `sf [TAB] [QUERY] [--print] [-- ENGINE-OPTIONS]`, the commands, `doctor`; refuses output-changing engine options |
| `print.rs` | `--print` / piped output (rows as the window draws them on a terminal, real bytes into a pipe) |
| `views.rs` | the views' git/delta plans (diff, history, log, commit) |
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
