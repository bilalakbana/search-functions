# 🔍 Powerful Interactive Recursive Search Functions

A collection of efficient and flexible search tools to search inside file contents and paths — supporting everything from plain text to binary, PDF, Word, and more.

---

## 📑 Table of Contents

- [✨ Features](#-features)
- [🧭 Usage](#-usage)
- [🎹 Keyboard Shortcuts](#-keyboard-shortcuts)
- [🔧 Installation](#-installation)
- [🖼️ Examples](#-examples)
- [🙌 Credits](#-credits)

---

## ✨ Features

| Function          | Description                                                                 |
|-------------------|-----------------------------------------------------------------------------|
| `sf`              | All of it in one window (a compiled app): tabs Files · Content · Deep · Folders · Git · Help (the manual), options under ⚙ Config, mouse and keys |
| `advancedsearch`  | Search in text, binary, and most file types                                 |
| `advancedsearch_plus` | Like `advancedsearch` + support for PDF, Word, DOCX                     |
| `detailedsearch`  | Like `advancedsearch` + displays matching lines for filtering               |
| `ugrep-fzf`       | Interactive search using `ugrep` and `fzf`, recursive in folders            |
| `ugrep-fzf-plus`  | Like `ugrep-fzf` + support for PDF, Word, DOCX                              |
| `rga-fzf`, `rga-fzfS` | Interactive search like `ugrep-fzf-plus`, slightly slower but powerful (`S`: sorted list) |
| `f`               | Search in file paths only (respects .gitignore; pass `--hidden`/`--no-ignore` through to fd) |
| `ugf`, `ugf-plus`, `rgf`, `rgfS` | Search both file contents and file paths recursively (`-plus`: PDF/Word/DOCX, `S`: sorted) |
| `fbr`             | Browse the definitions of all shell functions                               |
| `gfzf`, `gfzf-sbs`, `gfzf-full`, `gfzf-full-sbs` | Interactive git log with commit preview (side-by-side / full-file diffs) |
| `logs`, `logs-and-diffs` (+ `-sbs`, `-full`, `-full-sbs`) | Numbered git log (optionally with diffs) in `less` |

---

## 🧭 Usage

### 🔹 Everything in One Window

```bash
sf                         # starts in Files
sf content needle          # Content searching for "needle", typing then filters the files found (Ctrl-Alt-F)
sf deep needle -- -i       # Deep: binaries and archives too, as advancedsearch (-i: ignore case)
sf in src/main.rs 'ankara izmir'   # inside one file, as Ctrl-T (the lines with both words)
sf help                    # the manual: tabs, keys, options, patterns, commands with examples
sf history src/main.rs     # the file's commits with their changes (also: log, diff, diff --short)
sf files -- -e rs -d 3     # options after -- go to the tab's engine: fd, ugrep or git log
sf content TODO | wc -l    # into a pipe or a file (or with --print) sf prints the list, no window
```

Options that change what the engine prints (`-l`, `--color`, `-c`, `--json`, `-C 3`, `--oneline`, …) are
refused with the reason. Into a pipe or a file the real paths go (Content and Deep: ugrep's own `path` or
`path:line:text` lines); on a terminal, with `--print`, the rows as the window shows them, never a raw control
character. The exit status is 0 when something was listed, 1 when nothing, 2 on an error.

| Tab | Lists | Typing | Options (⚙ Config, defaults) |
|-----|-------|--------|------------------------------|
| Files | every file below the folder | filters the names | Hidden files ✗, Ignored files ✓, Match case ✗ |
| Content | the files that contain the pattern (ugrep); every text file while it is empty | the pattern, searched 150 ms after the last key; the preview shows each match numbered ("Match #1", …) with two lines around it, and at its top right the pattern and how often it occurs | Matching lines ✗ (list the lines instead), Each match ✗ (a row for every match with the text around it, as `detailedsearch`; in both the typed words, apart by spaces, are each on the line or in the file's path, `"a b"` one word), Archives & documents ✗ (zip, tar, 7z, PDF, Office via `ugrep+`), Hidden files ✗, Ignored files ✓, All words on a line ✗ / All words in the file ✗ (`ankara izmir`: both on one line / both in the file), Whole word ✗, Regular expression ✓ (off: the text as typed, `foo(` or `a.b`), Smart case ✗ (an upper-case letter counts the case), Match case ✗ (both off: case aside) |
| Deep | as Content, in every file: binaries (dll, exe, …) and archives (zip, tar, 7z, gz) too; every file while the pattern is empty. Together `advancedsearch`, `advancedsearch_plus` and `detailedsearch` | the pattern, as in Content; the preview numbers the matches, in a binary the first 300 each with the bytes around it, and starts once the cursor rests | Each match ✗ (`detailedsearch`: every match with the text around it, in binaries too), Documents ✗ (PDF, Office, e-books as text via `ugrep+`, as `advancedsearch_plus`; slower), Hidden files ✗, Ignored files ✓, All words on a line ✗ / All words in the file ✗ (`ankara izmir`: both on one line / both in the file), Whole word ✗, Regular expression ✓, Smart case ✗ (an upper-case letter counts the case), Match case ✗ (both off: case aside) |
| Folders | every folder, `..` first | filters the names; `Enter` goes inside | Hidden folders ✗, Ignored folders ✓, Match case ✗ |
| Git | the commit graph, numbered | filters the commits; `Enter` shows one; `Ctrl-Shift-Alt-f` is Search in versions: the files of every version searched (`git grep`; rows `hash version path:line:text`, the version the tag it is in, `v7.8.5~3`, or its date; nothing typed: each file's every change once; `Enter` the file as it was) | All branches ✓, Whole files ✗ (a commit and `Ctrl-u` show the changed files whole), Match case ✗; searching versions has its own: Whole word ✗, Regular expression ✓ (Perl's), Binary files ✗, Documents ✗ (PDF, Office, e-books, archives, read from git, never copied to disk), Match case ✗, and the versions to search in (all, or those chosen: hash, date, author, branch, subject) |
| Help | the manual: tabs, keys, options, patterns, commands with examples | finds in it, `Enter` the next one | — |

As in fzf, the list fills from the bottom: the first row sits right above the input line, `Up` goes to the next
one, `Home` to the top. Click a tab or use `Tab` / `Shift-Tab`; click **⚙ Config** (or `Ctrl-S`) for the tab's options, `Space` or a click
toggles one. A double click is `Enter`, the wheel scrolls the list or the preview. `Alt-Up` / `Alt-Down` scroll the
preview, `Ctrl-L` hides or shows it (each tab keeps its own; Files starts without one), `Ctrl-R` runs the list
again, `Esc` quits. The keys for the selected item are those under
[Keyboard Shortcuts](#-keyboard-shortcuts) and are shown in the middle of the tab bar. A bar on the right of the list and
the preview shows where you are in them. Filtering is case-insensitive and
every word must match (as `fzf -i -e`, with its syntax: `!test` leaves out, `^src/` at the start, `.rs$` at the end,
`.rs$ | .md$` either, `my\ file` a space in a word); the matches are shown red. The options are remembered in `~/.config/sf/config.toml`
(`$XDG_CONFIG_HOME/sf/config.toml`).

`sf` works the same on any Linux desktop and on WSL. `Enter` opens with the default application (on WSL Windows'
own), `Ctrl-e` shows the item selected in the file manager (Nautilus, Dolphin, Nemo, Caja, Thunar through D-Bus; on
WSL Explorer), `Ctrl-c` copies (wl-copy, xclip or xsel; on WSL the Windows path to the Windows clipboard; without any
of them OSC 52 through the terminal). `sf doctor` shows what it found and how each action works. `sf open|reveal|copy-path PATH`
and `sf copy-text TEXT` do one action; the fzf functions below use them when `sf` is installed.

The earlier fzf version (`~/.SearchFunctions/sf`, modes on F2–F8) still works until the app has replaced it.

### 🔹 Basic Content Search

```bash
advancedsearch 'search string'
advancedsearch 'search string' --glob='*.cpp,*.py'

detailedsearch 'search string'
detailedsearch 'search string' --glob='*.md,*.txt'
```

### 🔹 Advanced Search in PDFs, DOCX

```bash
advancedsearch_plus 'keyword'
```

### 🔹 Interactive Search with FZF

```bash
ugrep-fzf
ugrep-fzf-plus
rga-fzf
rga-fzfS
```

### 🔹 File Path Search Only

```bash
f                          # respects .gitignore
f --hidden --no-ignore     # extra arguments go to fd
f --glob='*.cpp,*.py'     # by file-name pattern
```

### 🔹 Full Recursive Search (content + path)

```bash
ugf    # ugf-plus for PDF/Word/DOCX
rgf    # rgfS for a sorted list
(the query is typed inside fzf)
```

---

## 🎹 Keyboard Shortcuts

These shortcuts apply when using the interactive search tools (`fzf`, `ugrep-fzf`, `rga-fzf`, etc.):

| Shortcut | Action                                   |
|----------|------------------------------------------|
| `Enter`  | Open the selected file                   |
| `Ctrl-e` | Open file location in file manager       |
| `Ctrl-c` | Copy relative path of selected file (on WSL its Windows path; use `Esc` to quit) |
| `Ctrl-o` | Open the file in nvim in a new tmux window (`sf`: at the matching line, with nvim or else `$VISUAL`, `$EDITOR`, vim, vi; outside tmux in the same terminal) |
| `Ctrl-t` | Search inside the selected file in a new tmux window (`sf`: in tmux a window of its own, else inside sf; the lines with all the typed words, `Ctrl-s` for all in the file or any of them, Match case; `Esc` back) |
| `Ctrl-u` / `Ctrl-g` / `Alt-g` / `Alt-u` | git diff / diff (delta) / short diff / log of the file (`sf`: history with changes / changes against HEAD / short / log; in tmux each in a window of its own (`sf history|diff|log FILE`), else inside sf; `/` searches, `Esc` back; `Enter` in Git shows a commit) |
| `Ctrl-f` | `sf`: find in the preview, as Notepad++'s Find (every match red, the current one underlined; what is looked for and `3/17` stay at the preview's top right; `Enter` `↓` the next, `↑` the one before, `Ctrl-s` Match case, Whole word, Regular expression (Perl's syntax); `Esc` back), e.g. a word in a Git commit. In tmux a root binding of `C-f` (`bind -n C-f …`) would take the key before sf sees it (`tmux list-keys -T root`) |
| `Alt-j` / `Ctrl-j` | Folder list / file list in a new tmux window |
| `Ctrl-p` | `sf`, Content and Deep: the files to search in, chosen from those listed (all at first); the others leave the list and the next searches look only in the chosen ones; All files: as before. The file types (`Ctrl-s`) stand above it: changing them ends the choice |
| `Ctrl-l` | Toggle the preview                       |
| `Ctrl-s` | `sf`: the tab's options (⚙ Config); in Files, Content and Deep also the file types of the list, all on: `Space` turns one off or on, `a` all of them (each tab its own, not saved); it opens under ⚙ Config at the right edge and takes more columns, growing to the left, when the list is long (`←` `→` go across) |
| `Ctrl-Alt-f` | `sf`, Content and Deep: keep the pattern, typing filters the rows found by their names and lines (as `advancedsearch PATTERN`); `Esc` back to the pattern. In Git `Ctrl-Shift-Alt-f` is Search in versions; sf asks the terminal to tell the two apart, which in tmux needs `set -s extended-keys on`, `set -s extended-keys-format csi-u` and `set -as terminal-features ',xterm*:extkeys'` |

> These are customizable if you tweak your FZF or terminal bindings.

---

## 🔧 Installation

### `sf` in one line (Linux and WSL, x86_64)

```bash
mkdir -p ~/.local && curl -fsSL https://github.com/bilalakbana/search-functions/releases/latest/download/sf-linux-x86_64.tar.gz | tar -xz -C ~/.local
sf doctor   # what it found, how open / reveal / copy work here
```
Without curl: `mkdir -p ~/.local && wget -qO- https://github.com/bilalakbana/search-functions/releases/latest/download/sf-linux-x86_64.tar.gz | tar -xz -C ~/.local`.

The package brings its own fd, bat, ugrep (with `ug` and `ugrep+`) and delta in `~/.local/lib/sf`; only sf
uses them. If `sf` is not found, `~/.local/bin` is not in your `PATH` yet: log in again (Ubuntu and Debian add it
once it exists) or add `export PATH="$HOME/.local/bin:$PATH"` to your shell's rc file. Taken from the system when
wanted: git (Git tab, diffs), tmux (actions in windows of their own), an editor for `Ctrl-o` (nvim, else
`$VISUAL`, `$EDITOR`, vim or vi), and for PDF and Office files
in Content and Deep `sudo apt install poppler-utils antiword pandoc`. Each release names the package's sha256 (also in
`sf-linux-x86_64.tar.gz.sha256`); what is inside and the licenses: `~/.local/share/doc/sf/`. To update, run the
line again.

### The fzf functions

1. Install required tools (latest versions recommended):

```bash
sudo apt install fzf ugrep git ripgrep git-delta
sudo npm install -g git-split-diffs
cargo install ripgrep-all  # if using Rust
```

2. Download the repo, create the following drectory and extract the content to ~/.SearchFunctions.
```bash
mkdir ~/.SearchFunctions
```

3. Following extraction files to the directory in step 2, make the helper scripts executable
```bash
chmod +x ~/.SearchFunctions/sub-functions/* ~/.SearchFunctions/sub-functions/launchers/*
```

4. Then add those to your shell for example ~/.zshrc
```bash
source ~/.SearchFunctions/advancedsearch                  # advancedsearch
source ~/.SearchFunctions/advancedsearch_plus             # advancedsearch_plus
source ~/.SearchFunctions/detailedsearch                  # detailedsearch
source ~/.SearchFunctions/search_in_file_content          # ugrep-fzf | ugrep-fzf-plus | rga-fzf | rga-fzfS
source ~/.SearchFunctions/search_in_file_path_and_content # ugf | ugf-plus | rgf | rgfS
source ~/.SearchFunctions/find_files                      # f
source ~/.SearchFunctions/sub-functions/list_functions    # fbr
source ~/.SearchFunctions/sub-functions/git_logs          # gfzf* | logs* (need git-delta; show_diffs needs git-split-diffs)
alias sf-fzf=~/.SearchFunctions/sf                        # the fzf version of sf (a script, not a function)
```

### `sf` from source

Rust 1.88 or newer (ratatui 0.30 needs it); sf then runs the fd, ugrep, bat, git and delta it finds in `PATH`, and
`ugrep+` for archives and documents ([fd](https://github.com/sharkdp/fd), [ugrep](https://github.com/Genivia/ugrep),
[bat](https://github.com/sharkdp/bat), [delta](https://github.com/dandavison/delta)):
```bash
cargo install --path ~/.SearchFunctions/sf-rs   # installs ~/.cargo/bin/sf
sf doctor
```
Content needs ugrep 3.6.0 or newer for Matching lines; before 5.0.0 only the first match of a line is marked red
(Ubuntu 22.04 ships 3.7.2, Debian 12 3.11.2; Ubuntu 24.04 and Debian 13 are fine). With `sf` installed, `Enter`,
`Ctrl-e` and `Ctrl-c` of the fzf functions go through it (`sub-functions/platform_action`), so they work on WSL too;
without it they use xdg-open, `open_in_file_manager` and wl-copy.

The release package is built by `python3 -I packaging/build.py NEW_EMPTY_FOLDER` from the versions pinned in
`packaging/bundle.toml` (needs docker and `rustup target add x86_64-unknown-linux-musl`) and tested in clean
Ubuntu 22.04, Debian 12 and Ubuntu 24.04 containers by `packaging/test_package.sh PACKAGE NEW_EMPTY_FOLDER`;
`.github/workflows/release.yml` does both for a pushed tag `vX.Y.Z` and publishes the release.

Tests: `cargo test` in `sf-rs`, `sf-rs/tests/e2e.sh NEW_EMPTY_FOLDER` (runs it in a private tmux server),
`sf-rs/tests/platform_action.sh NEW_EMPTY_FOLDER` (the wrapper of the fzf functions),
`sf-rs/tests/sub_functions.sh NEW_EMPTY_FOLDER` (the scripts sf's keys open) and `python3 -I packaging/test_build.py`.

---

## 🖼️ Examples
To be added

---

## 🙌 Credits

Built and maintained by Bilal Akbana.

A special thanks to the developers and contributors of the incredible open-source tools that power this project:

[`fzf`](https://github.com/junegunn/fzf) [`ugrep`](https://github.com/Genivia/ugrep) [`ripgrep`](https://github.com/BurntSushi/ripgrep) [`ripgrep-all`](https://github.com/phiresky/ripgrep-all) [`git-delta`](https://github.com/dandavison/delta) [`git-split-diffs`](https://github.com/banga/git-split-diffs)

Without your dedication to open source, this project wouldn't exist — thank you!

> 💬 Pull Requests are welcome! If you have improvements, ideas, or bugfixes, feel free to contribute.