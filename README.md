# 🔍 Powerful Interactive Recursive Search Functions

A collection of efficient and flexible search tools to search inside file contents and paths — supporting everything from plain text to binary, PDF, Word, and more.

---

## 📑 Table of Contents

- [✨ Features](#-features)
- [🧭 Usage](#-usage)
- [🎹 Keyboard Shortcuts](#-keyboard-shortcuts)
- [🔧 Installation](#-installation)
- [🖼️ Examples](#-examples)
- [🧰 Recommended Tools](#-recommended-tools)
- [🙌 Credits](#-credits)

---

## ✨ Features

| Function          | Description                                                                 |
|-------------------|-----------------------------------------------------------------------------|
| `advancedsearch`  | Search in text, binary, and most file types                                 |
| `advancedsearch+` | Like `advancedsearch` + support for PDF, Word, DOCX                         |
| `detailedsearch`  | Like `advancedsearch` + displays matching lines for filtering               |
| `ugrep-fzf`       | Interactive search using `ugrep` and `fzf`, recursive in folders            |
| `ugrep+fzf`       | Like `ugrep-fzf` + support for PDF, Word, DOCX                              |
| `rga-fzf`         | Interactive search like `ugrep+fzf`, slightly slower but powerful           |
| `f`, `fg`, `fh`   | Search in file paths only (normal, gitignored, hidden respectively)         |
| `ugf`, `rgf`      | Search both file contents and file paths recursively                        |

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
advancedsearch+ 'keyword'
```

### 🔹 Interactive Search with FZF

```bash
ugrep-fzf
ugrep+fzf
rga-fzf
```

### 🔹 File Path Search Only

```bash
f
fg   # respects .gitignore
fh   # also includes hidden files
```

### 🔹 Full Recursive Search (content + path)

```bash
ugf 'search string'
rgf 'search string'
```

---

## 🎹 Keyboard Shortcuts

These shortcuts apply when using the interactive search tools (`fzf`, `ugrep-fzf`, `rga-fzf`, etc.):

| Shortcut | Action                                   |
|----------|------------------------------------------|
| `Enter`  | Open the selected file                   |
| `Ctrl-e` | Open file location in file manager       |
| `Ctrl-c` | Copy relative path of selected file      |
| `Ctrl-t` | Open selected file in a new tmux window  |
| `Ctrl-p` | Pop out the preview in a separate tmux pane |

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
3. Then add those to your shell for example ~/.zshrc
```bash
source ~/.SearchFunctions/advancedsearch                  # advancedsearch | advancedsearch_respect_gitignore
source ~/.SearchFunctions/advancedsearch+                 # advancedsearch+ | advancedsearch_respect_gitignore+
source ~/.SearchFunctions/detailedsearch                  # detailedsearch | detailedsearch_respect_gitignore
source ~/.SearchFunctions/search_in_file_content          # ugrep-fzf | ugrep+fzf | rga-fzf
source ~/.SearchFunctions/search_in_file_path_and_content # ugf | rgf
source ~/.SearchFunctions/find_files                      # f | fh| fg
source ~/.SearchFunctions/sub-functions/list_functions    # fbr
source ~/.SearchFunctions/sub-functions/git_logs          # gfzf
```

---

## 🖼️ Examples
To be added
---

## 🧰 Recommended Tools

- [`fzf`](https://github.com/junegunn/fzf): Blazing fast fuzzy finder  
- [`ugrep`](https://github.com/Genivia/ugrep): Ultimate grep with Unicode support  
- [`ripgrep`](https://github.com/BurntSushi/ripgrep): Lightning-fast recursive search  
- [`ripgrep-all`](https://github.com/phiresky/ripgrep-all): Enables `ripgrep` to search PDFs, DOCX, etc.  
- [`git-delta`](https://github.com/dandavison/delta): Syntax-highlighting pager for `git diff`  
- [`git-split-diffs`](https://github.com/bryanbraun/git-split-diffs): Pretty split-view diffs in GitHub  

---

## 🙌 Credits

Built and maintained with ❤️ by [YourName].

A special thanks to the developers and contributors of the incredible open-source tools that power this project:

[`fzf`](https://github.com/junegunn/fzf) [`ugrep`](https://github.com/Genivia/ugrep) [`ripgrep`](https://github.com/BurntSushi/ripgrep) [`ripgrep-all`](https://github.com/phiresky/ripgrep-all) [`git-delta`](https://github.com/dandavison/delta) [`git-split-diffs`](https://github.com/bryanbraun/git-split-diffs)

Without your dedication to open source, this project wouldn't exist — thank you!

> 💬 Pull Requests are welcome! If you have improvements, ideas, or bugfixes, feel free to contribute.