# Changelog

sf searches file names, file contents (also PDF, Office files and archives) and git history in one terminal window.
This file follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the newest release comes first.

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

[0.13.3]: https://github.com/bilalakbana/search-functions/compare/v0.13.2...v0.13.3
[0.13.2]: https://github.com/bilalakbana/search-functions/compare/v0.12.1...v0.13.2
