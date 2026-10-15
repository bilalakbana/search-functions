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
| `Ctrl-c` | Copy relative path of selected file (use `Esc` to quit) |
| `Ctrl-o` | Open the file in nvim in a new tmux window |
| `Ctrl-t` | Search inside the selected file in a new tmux window |
| `Ctrl-u` / `Ctrl-g` / `Alt-g` / `Alt-u` | git diff / diff (delta) / short diff / log of the file |
| `Alt-j` / `Ctrl-j` | Folder list / file list in a new tmux window |
| `Ctrl-l` | Toggle the preview                       |

> These are customizable if you tweak your FZF or terminal bindings.

---

## 🔧 Installation

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
```

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