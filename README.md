# 🔍 Powerful Interactive Recursive Search Functions

A collection of efficient and flexible search tools to search inside file contents and paths — supporting everything from plain text to binary, PDF, Word, and more.

## 📑 Table of Contents

- [Features](#-features)
- [Usage](#-usage)
- [Installation](#-installation)
- [Examples](#-examples)
- [Recommended Tools](#-recommended-tools)
- [Credits](#-credits)

---

## ✨ Features

| Function         | Description                                                                 |
|------------------|-----------------------------------------------------------------------------|
| `advancedsearch` | Search in text, binary, and most file types                                 |
| `advancedsearch+`| Like `advancedsearch` + support for PDF, Word, DOCX                         |
| `detailedsearch` | Like `advancedsearch` + displays matching lines for filtering               |
| `ugrep-fzf`      | Interactive search using `ugrep` and `fzf`, recursive in folders            |
| `ugrep+fzf`      | Like `ugrep-fzf` + support for PDF, Word, DOCX                             |
| `rga-fzf`        | Interactive search like `ugrep+fzf`, slightly slower but powerful           |
| `f`, `fg`, `fh`  | Search in file paths only (normal, gitignored, hidden respectively)         |
| `ugf`, `rgf`     | Search both file contents and file paths recursively                        |

---

## 🧭 Usage

### Basic Content Search

advancedsearch 'search string'
advancedsearch 'search string' --glob='*.cpp,*.py'

detailedsearch 'search string'
detailedsearch 'search string' --glob='*.md,*.txt'

### Advanced Search in PDFs, DOCX

advancedsearch+ 'keyword'

### Interactive Search with FZF
ugrep-fzf
ugrep+fzf
rga-fzf

### File Path Search Only
f
fg   # respects .gitignore
fh   # also includes hidden files

### Full Recursive Search (content + path)
ugf
rgf

## 🔧 Installation
sudo apt install fzf ugrep git ripgrep git-delta
sudo npm install -g git-split-diffs
cargo install ripgrep-all  # if using Rust

## Examples
