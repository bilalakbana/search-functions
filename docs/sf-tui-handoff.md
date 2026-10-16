# Handoff: sf Rust TUI (written 2026-10-16 before a context reset)

> Update 2026-10-17: the plan has been executed (all 12 tasks, inline). This file describes the state before
> that; the decisions made while building are in `docs/sf-tui-design.md` ("Decided while building").

Read this, then `docs/sf-tui-design.md` (approved by Bilal: "onaylıyorum hepsini başla full") and
`docs/sf-tui-plan.md` (12 tasks, not started yet). Execute the plan **natively, inline** (superpowers:
executing-plans): Bilal's CLAUDE.md forbids subagents unless he says yes, and he said "başla full".

## State

- `~/.SearchFunctions/sf-rs/`: `cargo new` done (crate/binary `sf`, edition 2024). `Cargo.toml` deps:
  ratatui 0.30.2, ansi-to-tui 8.0.1, serde 1 (derive), toml 1, dirs 7, anyhow 1. nucleo and a direct crossterm
  were removed on purpose (use `ratatui::crossterm`, which is 0.29; nucleo always ranks by score, Bilal's fzf is
  `--no-sort -i -e`). `src/main.rs` is still the hello-world stub. Task 1 is next.
- Verified APIs: `ratatui::init()/restore()/try_init()`, `ratatui::crossterm` re-export; ansi-to-tui
  `IntoText::into_text(&self) -> Result<Text<'static>, Error>` (impl for byte slices/strings).
- `sf-rs/tests/tools/`: `ansi2png.py` (renders `tmux capture-pane -e -p` output to PNG with Intel One Mono and the
  Ghostty Catppuccin palette — look at screenshots, don't judge the design from text), `make_fixture.sh <dir>`
  (the test tree: tricky names, zip member, binary, hidden/ignored, `src/evil.txt` with 0x1f for the Ctrl+O
  exploit, git repo with 2 commits), `prototype_e2e.sh` (the prototype's tmux e2e, for reference; its paths point
  into an old scratchpad).
- fzf 0.58.0 (minimum version, checksum-verified) was downloaded to the old session's scratchpad; download again
  if needed: github.com/junegunn/fzf/releases/download/v0.58.0/fzf-0.58.0-linux_amd64.tar.gz.
- The fzf prototype `~/.SearchFunctions/sf` (works, Catppuccin, F7/F8 toggles, security fixes) and its changes
  are **uncommitted** in the main repo: `?? sf`, `M README.md`, `M sub-functions/search_in_single_file`
  (Catppuccin Ctrl+T window), `?? docs/`, `?? sf-rs/`. Nothing of this is pushed; ask before any commit/push.
- Engines on the PC: ugrep 7.8.5 (`ugrep+` with pdftotext/antiword/pandoc/exiftool), fd 10.3, bat 0.25
  (`--theme base16` = terminal palette), delta 0.18.2, git, fzf 0.74.5 in `~/.local/bin`, tmux 3.5a, zsh at
  `/usr/local/bin/zsh`, nvim `/usr/local/bin/nvim`. Prototype facts worth reusing: `ugrep --format` with fields
  first (`%f\x1f%n\x1f…`) is ~3x faster than post-processing; ugrep exits 2 as soon as one file is unreadable, so
  exit codes are no failure signal; `GREP_PALETTE` and catppuccin/delta options are in the prototype `sf`.

## Rules that matter here (from Bilal's CLAUDE.md and this session)

- Conversation in Turkish, code/comments/commits in English. Short, result first.
- No git write (commit/push/branch…) without his explicit OK per action; no deletion of any file without his OK
  for exactly those paths (rename to `.tobedeleted` only if in the way).
- No subagents without asking (state count/cost).
- Tests must stub every desktop command (xdg-open, wl-copy, nvim, open_in_file_manager, windows_action) for every
  branch they run; run e2e in a **private tmux socket** (`tmux -L <name> -f /dev/null`), never his tmux.
- Verify CLI flags against the installed version (`--help`) before proposing commands (gh here is 2.46).
- Verify before presenting; self-audit stale references; keep memory updated.
- The automated security review runs on commits/edits: fix real findings (it found two real ones in the
  prototype), explain non-findings.
