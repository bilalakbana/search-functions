# sf: state and next steps (handoff, 2026-10-17)

Read this first in a new session, then `docs/sf-tui-design.md` (the approved spec plus every decision made while
building, see "Decided while building"). `docs/sf-tui-plan.md` and `docs/sf-tui-handoff.md` are history: the plan
was executed, the old handoff describes the state before that.

How Bilal starts the next session (paste):

> ~/.SearchFunctions'taki sf işine devam ediyoruz. Önce docs/sf-next-steps.md'yi oku, sonra sf-tui-design.md'yi.
> "Next steps"teki Adım 2'yi (release paketi) uygula: inline, alt agent olmadan, testleriyle. Commit/push/silme için
> her seferinde benden onay al. Bana Türkçe kısa ara raporlar ver.

## State

- `~/.SearchFunctions/sf-rs`: the Rust app `sf` (ratatui 0.30, ~7300 lines incl. tests). Installed as
  `~/.cargo/bin/sf` (`cargo install --path ~/.SearchFunctions/sf-rs`); nothing else in PATH is named sf. The fzf
  prototype `~/.SearchFunctions/sf` still exists (README suggests `alias sf-fzf=…`).
- Git: `origin` (github.com/bilalakbana/search-functions, public) has everything up to `4289891` (Step 1, the sf
  part, pushed by Bilal 2026-10-17). Step 1's second commit (fzf functions through `platform_action`, README,
  docs) came after it: check `git status` / `git log` for whether it is committed and pushed.
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
| `model.rs` | `Tab` {Files, Content, Folders, Git} (`from_name("search")` still → Content), `Opt`, `Options` (serde; field `content` has `alias = "search"`), defaults |
| `config.rs` | load/save `$XDG_CONFIG_HOME/sf/config.toml`; an invalid file is reported and never overwritten |
| `engines.rs` | finds fd/fdfind, bat/batcat, ugrep, ugrep+, git, delta, file, ls in PATH; `$BAT_THEME` |
| `commands.rs` | pure: `list_plan` (engine command per tab/options/query) and `preview_plan` (Sniff/Run/Pipe, Content `Header` with `ugrep -c -o` counts) |
| `items.rs` | compact rows (`Look::{Path, Match, Styled}`, ~160 B/row), parsers, `path_spans` (folders blue, `/` yellow, name green), `printable`, `ansi_text` (no escape gets through) |
| `filter.rs` | fzf `-i -e --no-sort` semantics, parallel from 50 000 rows |
| `runner.rs` | list child in its own process group (kill on cancel), bounded records (64 KiB), batches, sorting/numbering |
| `preview.rs` | preview worker (newest wins, kills the old), `is_text`, header lines |
| `app.rs` | state machine: keys, mouse, effects; list fills bottom-up like fzf (Up → next row, Home → top) |
| `ui.rs` | drawing: tab bar with the keys centred, preview, list (bottom-up, highlight of typed words, scrollbars), input |
| `actions.rs` | Enter/^O/^T/^U/^G/Alt-G/Alt-U/^E/^C: open/reveal/copy through `platform.rs`, tmux windows via `sh -c 'cd -- "$1" && shift && exec "$@"'` (never `tmux -c DIR`: format injection), nvim pattern via `$SF_QUERY` |
| `platform.rs` | `Platform::detect()` / `from_parts()` (Linux or WSL, display, tmux, PATH), `open`, `reveal` (D-Bus FileManager1, fallback xdg-open on the folder; WSL Explorer), `copy_path`/`copy_text` (`Clipboard`: wl-copy, xclip, xsel, PowerShell, tmux OSC 52, OSC 52), `command()` for `sf open|reveal|copy-path|copy-text`, `launch()` |
| `doctor.rs` | `sf doctor`: the report from injected `run` (versions, D-Bus questions), `run_program` with a timeout |
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

- Bilal: reinstall sf so the fzf functions use it (`cargo install --path ~/.SearchFunctions/sf-rs`, replaces
  `~/.cargo/bin/sf`). Until then `platform_action` sees the old build's usage and keeps using the Linux tools.
- Bilal, on Windows/WSL (only he can): install sf there and go through this list:
  1. `sf doctor`: Platform `WSL (<distro>)`, Windows `powershell.exe /mnt/c/…`, Paths `wslpath /usr/bin/wslpath`.
  2. In `sf` (Files tab): `Enter` on a `.txt` opens it in its Windows application.
  3. `Ctrl-E` opens Explorer with the file selected (also for a name with spaces, `,`, `&`, `é`).
  4. `Ctrl-C`, then paste in Windows: the Windows path (`\\wsl.localhost\<distro>\…` or `C:\…` under `/mnt/c`).
  5. Git tab, `Ctrl-C`: the hash is in the Windows clipboard.
  6. A fzf function (`f`): `Enter`, `Ctrl-e`, `Ctrl-c` do the same through `platform_action`.
  7. A name with `"` and `Ctrl-E`: sf refuses with "Explorer cannot select a name containing '"' …".
  If all works, the WSL repo (`~/.SearchFunctionsWSL`, github bilalakbana/search-functions-wsl) can retire.
- Bilal: whether a Dolphin window showed `sf reveal`'s test file selected (`scratchpad/reveal-test/a b,c/sf reveal
  test é.txt` of the 2026-10-17 session): the only real check of the D-Bus path on this PC.

This PC: KDE Plasma on Wayland, `org.freedesktop.FileManager1` activatable (running), `wl-copy` installed, no
xclip/xsel; Docker usable (Bilal is in group `docker`).

### Step 2: one release package (approved in principle, after Step 1)

- `bundle.toml` in the repo: pinned upstream versions + sha256 of their official static release assets.
  Checked 2026-10-17: fzf 0.74.5 `fzf-0.74.5-linux_amd64.tar.gz` (MIT), fd 10.5.0
  `fd-v10.5.0-x86_64-unknown-linux-musl.tar.gz` (MIT/Apache-2.0), bat 0.26.1
  `bat-v0.26.1-x86_64-unknown-linux-musl.tar.gz` (MIT/Apache-2.0), delta 0.20.1
  `delta-0.20.1-x86_64-unknown-linux-musl.tar.gz` (MIT). ugrep 7.8.5 (BSD-3-Clause) publishes **no Linux binaries**:
  build it in CI with `./build.sh --enable-static` (README: may need `--without-brotli`); ship `ugrep`, `ug` and the
  `ugrep+` script.
- GitHub Actions: on a tag build sf for `x86_64-unknown-linux-musl` (static), build ugrep, download the others with
  checksum checks, add all license texts and the sub-function scripts, publish
  `sf-linux-x86_64.tar.gz` (layout `bin/sf`, `lib/sf/{fzf,fd,bat,delta,ugrep,ug,ugrep+,sub-functions/}`,
  `share/doc/sf/licenses/`). A weekly workflow checks upstream releases, bumps `bundle.toml` and opens a PR.
- sf looks for engines and sub-functions next to its own executable (`../lib/sf/`) first, then PATH / `$SF_SUBFUNCTIONS`.
- README: one-line install `curl -fsSL https://github.com/bilalakbana/search-functions/releases/latest/download/sf-linux-x86_64.tar.gz | tar -xz -C ~/.local`;
  optional `sudo apt install poppler-utils antiword pandoc` for PDF/Office; build-it-yourself
  (`cargo install --git https://github.com/bilalakbana/search-functions sf` + install the engines) with links to
  every upstream project.
- Needs Bilal's OK: `rustup target add x86_64-unknown-linux-musl` (download), pushing the workflows, every tag.
- Considered and rejected as the main way (keep in mind): forking/mirroring upstream repos (only ugrep needs a
  build), embedding the binaries into sf (40 MB, writes executables at runtime), apt only (old versions on Ubuntu
  LTS/WSL), Homebrew tap (possible extra channel later).

### Step 3: the terminal windows inside sf (later)

Ctrl-T (in-file search, today `search_in_single_file`, bash + fzf), Ctrl-U/Ctrl-G/Alt-G (diffs; `show_diffs` needs
the Node tool git-split-diffs, replaceable by `delta --side-by-side`), Alt-U (log) as native views. Then the package
drops fzf and the scripts. The engines (fd, ugrep, bat, git, delta) stay external on purpose: reimplementing them
would change the regex dialect and lose ugrep+'s archive/PDF search.

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
- `cargo clippy` names 7 older spots (collapsible `if`s in app.rs, ui.rs, main.rs, preview.rs, runner.rs; a large
  enum variant in commands.rs; a manual range check in a preview test); none in platform.rs or doctor.rs.
