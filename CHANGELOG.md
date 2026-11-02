# Changelog

sf searches file names, file contents (also PDF, Office files and archives) and git history in one terminal window.
This file follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the newest release comes first.

## [Unreleased]

### Changed
- Files lists in the same order as Content: in each folder its own files first, then its subfolders, each
  A–Z (as `ugrep --sort=name`). Before, Files sorted the whole path as text, so `bin/x` came between
  `README.md` and `build.sh`. Listing 1,000,000 files takes about 0.18 s longer (0.69 → 0.87 s).

## [0.13.4] - 2026-11-02

### Security
- A `.ugrep` file in the searched folder no longer runs a command. With Archives & documents (Content) or
  Documents (Deep) on, sf runs `ugrep+`, which runs `ug`, and `ug` loaded the folder's `.ugrep`: its
  `filter=` ran a program on every file and could hide files from the list. Every `ugrep+` call now
  starts with `--no-config` (ugrep 5.0.0 or newer; `sf doctor` warns about an older one). A `~/.ugrep`
  of your own is not read either.
- A pattern that starts with `---` (as the `---` of Markdown and YAML) no longer loads a config file.
  ugrep reads such an argument as `--config=FILE`, also after `-e`: the folder's `.ugrep` was loaded,
  also in the default mode, and the search found nothing. sf now gives ugrep each pattern as one
  argument, `--regexp=PATTERN`.
- A repository's own git config no longer runs a program while you browse. A tree with its `.git/` or a
  bare repository inside a clone ran its textconv program (Git tab preview, Ctrl-U), its external diff,
  fsmonitor, clean filter and `post-index-change` hook (Ctrl-G; a hook file, or from git 2.54 on a hook
  that its config defines), its gpg program (Git list), a submodule's own filter and external diff
  (Ctrl-G and Ctrl-U on a folder) and, in a partial clone, its `core.sshCommand` (each view of an old
  version). Each git that sf starts gets
  `-c core.fsmonitor=false -c log.showSignature=false -c core.hooksPath=/dev/null -c diff.submodule=short`
  and `GIT_NO_LAZY_FETCH=1 GIT_ALLOW_PROTOCOL=none`; `git diff`, `log -p` and `show` get
  `--no-ext-diff --no-textconv`. The diff view turns off each filter (`clean`, `process`) that the
  repository's own config defines and each hook defined in config (`hook.NAME.enabled=false`), and does
  not look for changes inside submodules (`--ignore-submodules=dirty`). With git older than 2.26 it
  shows no diff: such a git cannot name the repository's filters (`sf doctor` warns). Your own filters,
  such as git-lfs, stay. Programs that sf opens (your editor, a shell) keep your git config.
  What you see changes in three places:
  - In a partial clone (`git clone --filter=blob:none`), a version whose content is not fetched yet
    shows git's error (for example "lazy fetching disabled") instead of being fetched. Run the same
    `git log -p` or `git show` once yourself to fetch it.
  - A submodule's new commit shows as its old and new "Subproject commit" lines, also when your own
    config says `diff.submodule=log` (a list of the submodule's commits).
  - Ctrl-G on a folder does not show uncommitted changes inside a submodule; open the submodule's own
    folder for them.
- `sf in` and `sf open` show a missing name without its control characters.
- No raw escape sequence reaches the terminal from `sf grep-versions` on a terminal. Bidirectional
  embeddings, overrides and isolates (U+202E) in names show as U+FFFD; the marks LRM and RLM of Arabic
  and Hebrew text stay. A file's own "concealed" colour no longer hides its text in the preview.

### Fixed
- A search engine no longer runs on after sf ends by a signal (the terminal closed, SIGTERM). An
  `sf` started with SIGHUP ignored (`nohup`) keeps ignoring it.
- Matching lines, Each match and the versions search keep at most 1,000,000 rows, say so, and end
  the engine. A one-letter pattern in a large tree used to take all the memory.
- The Git tab on a repository with a wide graph used much less memory: git/git with All branches,
  1.75 GB before, 177 MB now.
- A path with a newline, or an escape sequence in a name, no longer breaks Matching lines and Each
  match rows (the row named another file). A path with 64 newlines or more is still listed under its
  last line.
- The preview scrolls to the end of a text longer than 65,535 lines (a 4 MiB line at 80 columns).
- Each match counts ugrep's "cannot read" warnings as "N paths could not be read" instead of listing
  them with colour remnants.
- A burst of keys (Ctrl-R Ctrl-R) no longer drops the "…" while the list still loads.
- Folders does not go into a folder it cannot read; it says "Cannot open NAME: Permission denied".
- The Help tab wraps the manual at narrow widths instead of cutting it.
- A pattern that is not a regular expression gets one line that says what is wrong.
- `sf in` with a missing file says so. Ctrl-T on a file named `-` lists its lines.
- A PDF of text only previews as a document. The Git preview's stat bars fit the preview.
- An empty folder says "No files here" in Files.
- sf gives its tmux window a name with `#` back as it was.
- `sf grep-versions | head` ends at once.

### Tests
- New end-to-end cases: a closed terminal ends the engine, `nohup` keeps SIGHUP ignored, a folder's
  `.ugrep` (also with the pattern `---`) and a repository's git config (also its hook, a submodule's
  config and a partial clone's fetch) run nothing, a program sf opens
  gets no git settings of sf's, a locked folder, `sf in` and `sf open` with a missing name that holds an
  escape sequence, a window name with `#`. Unit tests check that a pattern such as `---` reaches ugrep as
  one `--regexp=` argument in every search mode, in Ctrl-T and in `sf grep-versions`.

## [0.13.3] - 2026-11-01

### Fixed
- In tmux with `extended-keys on` and the default `extended-keys-format xterm`, Shift-Tab, Ctrl-Alt-F and
  Ctrl-Alt-H work again (0.13.2 lost them). sf asks tmux for modified keys only when tmux sends them in the
  csi-u form that sf reads. In the xterm form, Ctrl-Shift-Alt-F arrives as Ctrl-Alt-F, so Search in versions
  has no key there. The README and the help say so.
- `sf doctor` warns about a search root under `/mnt/` only in WSL, where it is a slow Windows drive mount.
  Plain Linux also mounts disks there.

### Changed
- `sf doctor` warns in tmux when `extended-keys always` meets the xterm form. tmux then sends Shift-Tab,
  Ctrl-Alt-F and Ctrl-Alt-H in a form that sf cannot read.
- The help names tmux's three settings for Ctrl-Shift-Alt-F (`extended-keys on`, `extended-keys-format csi-u`,
  `xterm*:extkeys`) on rows of their own, so 80 columns show them whole.

### Tests
- The end-to-end suite also checks the xterm form. On tmux before 3.5 (no `extended-keys-format`), it prints
  one `INFO` line for that check.
- The package test also runs in Debian 13 (tmux 3.5a), so four images now.

## [0.13.2] - 2026-11-01

First release of this line: 0.13.0 and 0.13.1 were built but never published (the package step of 0.13.0
stopped on an outdated constant in `packaging/bundle.toml`, then the package test of 0.13.1 failed on all three
images), so everything below ships as 0.13.2.

### Added
- `sf --version` prints `sf <version>` (the crate version at build time) and exits 0. After `--` it stays
  the engine's option.

### Changed
- The fzf look: the search root is centred on the top border in the path colours, and the tab bar is quiet,
  with the keys at its right. The list fills the left half and the preview the right half. A prompt box
  (`>` and the counter) goes across the full width under both.
- The palette is fzf's 256-colour one. You can set each colour role in the `[theme]` table of
  `~/.config/sf/config.toml`.
- In the Git tab, Ctrl-Shift-Alt-F opens Search in versions. Ctrl-Alt-F stays the result filter of Content
  and Deep, and the filter's input line names itself.
- `sf doctor` warns when the search root is under `/mnt/` (WSL drive mounts are much slower), and the README
  says so.

### Fixed
- Files shows its preview at once. A preview asked for before the frame that lays it out is asked for again
  at the real width (bat had wrapped at 20 columns after Ctrl-L).
- A long text file's preview sends its first screen (200 lines or 64 KiB of bat's output) at once, then the
  whole text (up to 4 MiB). The scroll position stays when the rest comes. Previews with a count (Content,
  Deep) come whole.
- `--print0` puts a NUL after each path instead of the newline (as `fd -0` and `ugrep -0` do), so a newline
  in a file name survives `xargs -0`. A terminal still gets the rows as drawn.
- Paths that ugrep cannot read are said in one line, for example "2 paths could not be read", in the window
  and with `--print`. ugrep runs without `-s`, and the exit status stays 2.
- An empty Git list says why: "this repository has no commits yet" or "bare repository, no commits yet"
  (in the window and on `--print`'s stderr).
- An empty file's preview says "empty file" (it said "No text preview: binary file"). Without `file`
  installed, a binary file's preview tells you to install it.
- With tmux 3.2a extended keys, a control character with modifiers is read as Ctrl and its letter.
- The release package builds again: the Alpine image ships brotli 1.2.1, and `packaging/bundle.toml` now names
  that version (the license file is unchanged, same sha256).

### Tests
- The end-to-end suite (`sf-rs/tests/e2e.sh`) runs without `delta`, `file` or `zip`. The check of a missing
  engine prints a `SKIP` line and checks what sf shows instead. The last line is `pass=N fail=N skip=N`,
  and only failures set the exit code.
- The suite finds `fd` or Debian's `fdfind`, and the test pane uses the caller's `PATH`.
- The fixture builds `docs/arch.zip` with `python3 -m zipfile` when `zip` is missing.
- New end-to-end cases: an empty file, a binary file without `file`, a name with a newline, and a UTF-16LE
  file with its byte order mark. A run without `file` and `delta` gives 238 pass, 0 fail and 2 skips.
- The release workflow and `packaging/test_package.sh` run the suite with `SF_STRICT_SKIPS=1`, so an
  unexpected skip fails the release.
- `tests/speed_ratio.sh` measures the TUI's own overhead on 100,000 files: the median time to the first row
  minus the median `sf files --print` time over 5 paired release runs, failing above 150 ms. The ratio of the
  two is printed for information; it sat at 1.5 and flapped there on an unchanged tree.
- The package test runs the suite in each container as a non-root user, so the `chmod 000` cases really run
  (a root run skips them with one line). It names the crate version with `SF_EXPECT_VERSION`, because the
  container mounts only `tests/`: the old check compared against `sf ` and failed on every image.

## Earlier releases

Releases up to 0.12.1 came out before this changelog. Their notes are on the
[GitHub releases page](https://github.com/bilalakbana/search-functions/releases).

[0.13.4]: https://github.com/bilalakbana/search-functions/compare/v0.13.3...v0.13.4
[0.13.3]: https://github.com/bilalakbana/search-functions/compare/v0.13.2...v0.13.3
[0.13.2]: https://github.com/bilalakbana/search-functions/compare/v0.12.1...v0.13.2
