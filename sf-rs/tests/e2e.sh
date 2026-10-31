#!/bin/bash
# End-to-end test of the sf binary in a private tmux server (socket "sfrs-e2e", never Bilal's own):
# the fixture tree of tests/tools/make_fixture.sh, the real engines, and stubs for everything that would
# reach the desktop (xdg-open, wl-copy, dbus-send, nvim, the sub-functions, and WSL's powershell.exe and
# wslpath).
# A run needs tmux, python3, git, fd (or Debian's fdfind), ugrep, ugrep+ and bat (or batcat). It can do
# without delta and file (their cases print SKIP and check what sf shows instead) and without zip (the
# fixture's archive comes from python3). The pane's PATH is the system's, then the caller's: give a run
# delta or file through PATH.
# The config lives in a fresh XDG_CONFIG_HOME. Screens are saved as ANSI captures for ansi2png.py.
#
# usage: tests/e2e.sh WORKDIR        (WORKDIR: a new, empty folder; nothing in it is ever deleted)
#        SF_BIN=path/to/sf tests/e2e.sh WORKDIR   (default: target/debug/sf)
set -u
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
W=${1:?usage: e2e.sh WORKDIR}
SF=${SF_BIN:-$here/../target/debug/sf}
SF=$(realpath -- "$SF") || exit 1
mkdir -p "$W" && W=$(realpath -- "$W")
[[ -z $(ls -A "$W") ]] || { echo "e2e: $W is not empty" >&2; exit 2; }
T=$W/tree LOG=$W/actions.log STUBS=$W/stubs XDG=$W/xdg SHOTS=$W/screens
PANE_PATH=$STUBS:/usr/local/bin:/usr/bin:/bin:$PATH
FD=$(command -v fd || command -v fdfind) || { echo "e2e: neither fd nor fdfind found" >&2; exit 2; }
mkdir -p "$STUBS" "$XDG" "$SHOTS" "$W/nogit"
bash "$here/tools/make_fixture.sh" "$T" >/dev/null || { echo "e2e: fixture failed" >&2; exit 2; }
: > "$LOG"

# stub NAME LABEL [SECONDS]: logs "LABEL [arg] [arg] ..." (and what it read on stdin), then sleeps
stub() {
    local dir=$1 name=$2 label=$3 wait=${4:-0}
    cat > "$dir/$name" <<EOF
#!/bin/bash
line="$label"
for a in "\$@"; do line+=" [\$a]"; done
if [[ ! -t 0 ]]; then input=\$(cat); [[ -n \$input ]] && line+=" <\$input>"; fi
printf '%s\n' "\$line" >> "$LOG"
sleep $wait
EOF
    chmod +x "$dir/$name"
}
stub "$STUBS" xdg-open OPEN
stub "$STUBS" wl-copy COPY
stub "$STUBS" dbus-send REVEAL
stub "$STUBS" nvim NVIM 1
# WSL: wslpath answers C:\e2e and the path with backslashes; powershell.exe logs $SF_VALUE, $WSLENV and
# its last argument (the statement)
cat > "$STUBS/wslpath" <<'EOF'
#!/bin/bash
p=$2; printf 'C:\\e2e%s\n' "${p//\//\\}"
EOF
cat > "$STUBS/powershell.exe" <<EOF
#!/bin/bash
printf 'PS {%s} [%s] %s\n' "\$SF_VALUE" "\$WSLENV" "\${!#}" >> "$LOG"
EOF
chmod +x "$STUBS/wslpath" "$STUBS/powershell.exe"
# the tree as sf names it to D-Bus (a file:// URI) and as the wslpath stub gives it
uT=$(python3 -I -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$T")
winT="C:\\e2e${T//\//\\}"

tm() { tmux -L sfrs-e2e -f /dev/null "$@"; }
pane() { tm capture-pane -p -t e2e | sed -E 's/\x1b\[[0-9;]*m//g'; }
PASS=0 FAIL=0 SKIP=0
ok() { echo "PASS  $*"; PASS=$((PASS + 1)); }
skip() { echo "SKIP  $*"; SKIP=$((SKIP + 1)); }
bad() { echo "FAIL  $*"; FAIL=$((FAIL + 1)); pane | grep -v '^\s*$' | tail -15; }
wait_pane() { local i; for i in $(seq 60); do pane | grep -qE -- "$1" && return 0; sleep 0.1; done; return 1; }
expect() { if wait_pane "$2"; then ok "$1"; else bad "$1 (no /$2/ on screen)"; fi; }
expect_not() { sleep 0.4; if pane | grep -qE -- "$2"; then bad "$1 (/$2/ still on screen)"; else ok "$1"; fi; }
expect_log() { local i; for i in $(seq 50); do grep -qF -- "$2" "$LOG" && { ok "$1"; return; }; sleep 0.1; done; bad "$1 (log lacks: $2)"; tail -3 "$LOG"; }
keys() { tm send-keys -t e2e "$@"; }
type_() { tm send-keys -t e2e -l "$1"; }
shot() { tm capture-pane -e -p -t e2e > "$SHOTS/$1.ansi"; }
# red_on TEXT: TEXT is drawn in the theme's hl colour (256-colour 5 by default) somewhere on the screen
# (a highlighted match); tmux writes each colour as its own sequence, so the text follows it
red_on() { local i; for i in $(seq 30); do tm capture-pane -e -p -t e2e | grep -qF -- "38;5;5m$1" && return 0; sleep 0.1; done; return 1; }
# border_in N: a border glyph is drawn in the 256-colour N (other sequences may come between)
ESC=$'\e'
border_in() { local i; for i in $(seq 30); do tm capture-pane -e -p -t e2e | grep -qE -- "38;5;$1m($ESC\[[0-9;]*m)*(─|╭|╮|╰|╯|│)" && return 0; sleep 0.1; done; return 1; }
# root_of DIR: the folder as sf's top border names it, ~ for the home folder
root_of() { if [[ $1 == "$HOME" ]]; then echo '~'; elif [[ $1 == "$HOME"/* ]]; then echo "~/${1#"$HOME"/}"; else echo "$1"; fi; }
# layout_ok ROOT PREVIEW [TEXT]: the screen is fzf's --style full (lines from 0, widths in characters):
# the top border only ─ with " ROOT " in its middle (±1; cut at the right when wider than the pane);
# PREVIEW=yes: the list and the preview side by side from line 2 to h-4, the preview's left edge at
# width/2 and TEXT right of it; PREVIEW=no: the list alone across the width; then the prompt box, the
# last three lines across the width, its input line with > and N/M. Says what is wrong in $why.
layout_ok() {
    local LC_ALL=C.UTF-8 root=$1 preview=$2 text=${3:-} w h half want box bottom before l found=
    w=$(tm display-message -p -t e2e '#{pane_width}') h=$(tm display-message -p -t e2e '#{pane_height}')
    half=$((w / 2)) why="${w}x$h: "
    mapfile -t L < <(pane)
    want=" $root "
    if (( ${#want} >= w )); then
        [[ ${L[0]} == "${want:0:w}" ]] || { why+="top border not the cut root: ${L[0]}"; return 1; }
    else
        [[ ${L[0]} =~ ^(─+)"$want"(─+)$ ]] || { why+="top border: ${L[0]}"; return 1; }
        (( ${#BASH_REMATCH[1]} - ${#BASH_REMATCH[2]} <= 1 && ${#BASH_REMATCH[2]} - ${#BASH_REMATCH[1]} <= 1 )) \
            || { why+="root not in the middle: ${#BASH_REMATCH[1]} vs ${#BASH_REMATCH[2]}"; return 1; }
    fi
    box=${L[2]} bottom=${L[h-4]}
    if [[ $preview == yes ]]; then
        before=${box%%╮╭*}
        [[ $box == *╮╭* && ${#before} -eq $((half - 1)) ]] || { why+="no ╮╭ at column $((half - 1)): $box"; return 1; }
        before=${bottom%%╯╰*}
        [[ $bottom == *╯╰* && ${#before} -eq $((half - 1)) ]] || { why+="the boxes do not end at line h-4: $bottom"; return 1; }
        if [[ -n $text ]]; then
            for l in "${L[@]:3:h-7}"; do [[ $l == *"$text"* ]] && { found=$l; break; }; done
            before=${found%%"$text"*}
            [[ -n $found ]] && (( ${#before} > half )) || { why+="'$text' not right of the preview's left edge"; return 1; }
        fi
    else
        [[ $box == ╭*╮ && $box != *╮╭* && ${#box} -eq $w ]] || { why+="the list not alone across the width: $box"; return 1; }
        [[ $bottom == ╰*╯ && $bottom != *╯╰* ]] || { why+="the list does not end at line h-4: $bottom"; return 1; }
    fi
    [[ ${L[h-3]} =~ ^╭─+╮$ && ${#L[h-3]} -eq $w ]] || { why+="prompt box top: ${L[h-3]}"; return 1; }
    [[ ${L[h-2]} =~ ^"│ > ".*\ [0-9]+/[0-9]+(\ …)?\ │$ && ${#L[h-2]} -eq $w ]] || { why+="input line: ${L[h-2]}"; return 1; }
    [[ ${L[h-1]} =~ ^╰─+╯$ && ${#L[h-1]} -eq $w ]] || { why+="prompt box bottom: ${L[h-1]}"; return 1; }
}
layout() { local name=$1 i; shift; for i in $(seq 30); do layout_ok "$@" && { ok "$name"; return; }; sleep 0.1; done; bad "$name ($why)"; }
clear_query() { keys C-M-h; }
# back_to_sf: a terminal action opened a tmux window (it has the focus); wait until its stub ends
back_to_sf() {
    local i
    for i in $(seq 60); do [[ $(tm list-windows -t e2e | wc -l) -eq 1 ]] && break; sleep 0.1; done
    tm select-window -t e2e:0
}

# the engines sf finds in the pane, from its doctor rows: a path, or "not" when one is not found
engine() { env -i HOME="$HOME" PATH="$PANE_PATH" XDG_CONFIG_HOME="$XDG" "$SF" doctor | awk -v e="$1" '$1 == e { print $2 }'; }
DELTA=$(engine delta) FILE=$(engine file)
[[ $FILE == /* ]] && "$FILE" -b /dev/null >/dev/null 2>&1 || FILE=   # found, and it must run too

tm kill-server 2>/dev/null
# \$TMUX and \$TMUX_PANE are the private server's (expanded inside the pane), never the caller's own tmux;
# the PATH quoted: WSL's has spaces
tm new-session -d -s e2e -x 160 -y 40 "env -i HOME=$HOME PATH=${PANE_PATH@Q} TERM=xterm-256color \
    COLORTERM=truecolor LANG=C.UTF-8 XDG_CONFIG_HOME=$XDG LESS=-R WAYLAND_DISPLAY=wayland-e2e \
    TMUX=\$TMUX TMUX_PANE=\$TMUX_PANE bash --norc --noprofile"
# as Bilal's tmux: modified keys apart for the programs that ask (sf: Ctrl-Shift-Alt-F is not Ctrl-Alt-F);
# the format is tmux 3.5's, older ones send csi-u anyway
tm set -s extended-keys on
tm set -s extended-keys-format csi-u
keys "PS1='READY> '; cd '$T'" Enter
wait_pane '^READY>' || { echo "e2e: shell did not start" >&2; exit 2; }
tm rename-window -t e2e shell

# the defaults: hidden files off, ignored files on
n_files=$(cd "$T" && "$FD" --type f --no-ignore --exclude .git . | wc -l)
keys "'$SF'" Enter
expect 'start: tab bar with Config'        'Files +Content +Deep +Folders +Git .*⚙ Config'
expect "start: $n_files files listed"      "$n_files/$n_files"
expect 'start: Files shows bat with numbers, no Ctrl-L' '│ *1 needle ignored'
root=$(root_of "$T")
layout 'look: top border, list and preview side by side, prompt box' "$root" yes '1 needle ignored'
border_in 240 && ok 'theme: the default border is 38;5;240' || bad 'theme: no border in 38;5;240'
keys C-l
expect_not 'files: Ctrl-L hides the preview' '│ *1 needle ignored'
layout 'look: no preview, the list across the width, the prompt box stays' "$root" no
keys C-l
expect 'files: Ctrl-L shows it again'      '│ *1 needle ignored'
layout 'look: the preview back on the right half' "$root" yes '1 needle ignored'
expect 'start: footer'                     'Enter open · \^S options · \^O nvim'
[[ $(tm display-message -p -t e2e '#W') == search ]] && ok 'tmux: window named search' || bad 'tmux: window not renamed'
shot files
tm resize-window -t e2e -x 90 -y 24
layout 'narrow: the look at 90x24' "$root" yes '1 needle ignored'
shot narrow
tm resize-window -t e2e -x 160 -y 40
layout 'narrow: back at 160x40' "$root" yes
keys C-s
expect 'types: under the options'          '── File types ──'
keys Down Down Down Down Down Space
expect 'types: .bin turned off'            '\[ \] \.bin'
keys Escape
expect 'types: the list without it'        "Files › .*1 type off +$((n_files - 1))/$n_files"
keys C-s a Escape
expect 'types: a turns all on again'       "Files › .*ignored +$n_files/$n_files"
type_ '^src .txt$'
expect 'files: fzf ^ and $'                "Files › \\^src \\.txt\\$ .* 3/$n_files"
type_ ' !evil'
expect 'files: fzf ! leaves out'           "Files › \\^src \\.txt\\$ !evil .* 2/$n_files"
clear_query; type_ '.md$ | .bin$'
expect 'files: fzf | either'               "Files › \\.md\\$ \\| \\.bin\\$ .* 2/$n_files"
clear_query
type_ 'main'
expect 'files: query filters'              "Files › main .* 1/$n_files"
red_on 'main' && ok 'files: typed text highlighted' || bad 'files: typed text not highlighted'
keys Enter;  expect_log 'files: Enter opens'           "OPEN [$T/src/main.c]"
keys C-c;    expect_log 'files: Ctrl-C copies'         'COPY <src/main.c>'
expect 'files: copy shows a status'        'Copied to the clipboard'
keys C-o;    expect_log 'files: Ctrl-O nvim in tmux'   "NVIM [--] [$T/src/main.c]"; back_to_sf
keys C-e;    expect_log 'files: Ctrl-E reveals'        "REVEAL [--session] [--print-reply] [--dest=org.freedesktop.FileManager1] [--type=method_call] [/org/freedesktop/FileManager1] [org.freedesktop.FileManager1.ShowItems] [array:string:file://$uT/src/main.c] [string:]"
keys C-g
expect 'files: Ctrl-G a view of the diff'  'main\.c · changes against HEAD'
[[ $(tm display-message -p -t e2e '#W') == diff ]] && ok 'files: Ctrl-G in a tmux window, diff' || bad 'files: no diff window'
expect 'files: an unchanged file says so'  'No local changes in .*main\.c'
keys /
expect 'view: / searches it'               '^/ ▏'
keys Escape
expect 'view: Esc ends the search'         'Esc back'
keys Escape; back_to_sf
expect 'files: Esc closes the window'      "Files › main .* 1/$n_files"
keys C-t
expect "files: Ctrl-T the file's lines"    'In main\.c › .* 3/3'
[[ $(tm display-message -p -t e2e '#W') == infile ]] && ok 'files: Ctrl-T in a tmux window, infile' || bad 'files: no infile window'
type_ 'second more'
expect 'in file: all words on a line'      'In main\.c › second more .* 0/0'
keys C-s
expect 'in file: its own options'          'In file options'
keys Down Space Escape
expect 'in file: all words in the file'    'In main\.c › second more .*words in the file +2/2'
keys C-s Space Escape
expect 'in file: on a line again'          'In main\.c › second more .*words on a line +0/0'
keys Escape
expect 'files: Esc closes the window'      "Files › main .* 1/$n_files"
# before the views below write into empty.txt
clear_query; type_ 'empty'
expect 'files: an empty file says so'      '│empty file'
clear_query; type_ 'blob'
if [[ -n $FILE ]]; then
    expect 'files: a binary, its type from file' '│No text preview: data'
else
    expect 'files: a binary, file not installed' '│No text preview: binary file \(install `file` for its type\)'
    skip "files: a binary's type from file (no working file in the pane's PATH)"
fi
clear_query; type_ 'main'
expect 'files: back to main'               "Files › main .* 1/$n_files"

keys Tab
expect 'tab: Search keeps the query'       'Content › main'
expect 'search: one file'                  'src/main\.c'
clear_query; type_ 'needle'
expect 'search: files with needle'         '5/5'
expect 'search: preview numbers the matches' 'Match #1'
expect 'search: preview of the first file' '1: needle ignored'
expect 'search: pattern and count top right' ' needle · 1 match ╮'
expect_not 'search: no header lines in the text' '^│?File +build/out\.txt'
shot search
clear_query; type_ 'zzznotthere'
expect 'search: no-match hint'             'No match\. Archives & documents'
clear_query; type_ 'foo('
expect 'search: broken pattern shown'      'mismatched'
keys C-s Down Down Down Down Down Down Down Down Space Escape
expect 'plain text: named on the input'    'Content › foo\( .*plain text'
content_saved=$(awk '/^\[/ { s = ($0 == "[content]") } s' "$XDG/sf/config.toml" 2>/dev/null)
grep -qx 'regex = false' <<< "$content_saved" && ok 'plain text: saved' || bad 'plain text: not saved'
expect_not 'plain text: foo( is no error'  'mismatched'
clear_query; type_ 'needle|main'
expect 'plain text: | is the character'    'No match\.'
keys C-s Down Down Down Down Down Down Down Down Space Escape
expect 'regex again: | is either'          'Content › needle\|main .* 5/5'
clear_query; type_ 'needl'
expect 'whole word: needl found in needle' 'Content › needl .* 5/5'
keys C-s Down Down Down Down Down Down Down Space Escape
expect 'whole word: needl is no word'      'No match\.'
expect 'whole word: named on the input'    'Content › needl .*whole word'
keys C-s Down Down Down Down Down Down Down Space Escape
clear_query; type_ 'needle'
expect 'search: back to needle'            '5/5'
keys C-p
expect 'files: Ctrl-P lists the files found' ' Files to search in '
keys Down Space Escape
# 4/5 lasts only until the search over the chosen files ends (4/4), often within one look at the screen
expect 'files: one left out of the rows'   'Content › needle .*4 files chosen +4/[45]'
clear_query; type_ 'ignored'
expect 'files: searched in the chosen only' 'No match'
keys C-p a Escape
expect 'files: all files again'            'Content › ignored .* 1/1'
clear_query; type_ 'needle'
expect 'files: back to needle'             'Content › needle .* 5/5'
keys C-p Down Space Escape
expect 'files: one left out again'         '4 files chosen +4/5'
keys C-s a a Escape
expect 'files: types above the choice'     'Content › needle +ignored +5/5'
keys C-s
expect 'config: popup'                     'Content options'
expect 'config: boxes'                     '\[ \] Matching lines'
shot popup
keys Space
expect 'config: Matching lines on'         '\[x\] Matching lines'
grep -q 'matching_lines = true' "$XDG/sf/config.toml" 2>/dev/null && ok 'config: file written' || bad 'config: file written'
keys Escape
expect_not 'config: Esc closes the popup'  'Content options'
expect 'lines: file:line:text'             'src/main\.c:1:needle in main'
expect 'lines: hint changes'               'matching lines'
shot lines
keys C-o;    expect_log 'lines: Ctrl-O at the line'   "NVIM [+1] [--]"; back_to_sf
keys C-t
expect 'lines: Ctrl-T keeps the pattern'   'In out\.txt › needle .* 1/1'
keys Escape
expect 'lines: Esc back to Content'        'Content › needle'
clear_query; type_ 'src needle'
expect 'lines: a word in the path counts'  'Content › src needle .* 3/3'
expect 'lines: src/main.c kept'            'src/main\.c:1:needle in main'
keys C-M-f; type_ 'main'
expect 'lines: Ctrl-Alt-F filters the rows' 'Content filter › main · pattern: src needle .* 1/3'
keys Escape
clear_query; type_ 'needle'
expect 'lines: back to needle'             'Content › needle .* 5/5'
keys C-s Down Space
expect 'config: Each match on'             '\[x\] Each match'
expect 'config: Matching lines goes off'   '\[ \] Matching lines'
keys Escape
expect 'each match: file:line:match'       'src/main\.c:1:needle in main'
expect 'each match: hint changes'          'each match'
clear_query; type_ 'needle|main'
expect 'each match: a row per match'       'Content › needle\|main .* 6/6'
clear_query; type_ 'needle'
expect 'each match: back to needle'        'Content › needle .* 5/5'
keys C-s Space Down Down Space Escape
expect 'archives: zip member line'         'docs/arch\.zip\{inzip\.txt\}:1:needle inside zip'
keys C-s Space Down Down Space Escape
expect 'archives off, lines off'           '5/5'

keys Tab
expect 'tab: Deep keeps the pattern'       'Deep › needle'
expect 'deep: binaries and archives too'   'Deep › needle .* 7/7'
expect 'deep: a binary file'               'src/blob\.bin'
expect 'deep: an archive'                  'docs/arch\.zip'
expect 'deep: the matches numbered'        'Match #1'
shot deep
keys C-s
expect 'deep: its options'                 '\[ \] Documents'
keys Space Escape
expect 'deep: each match in a binary'      'src/blob\.bin:1:'
expect 'deep: each match in an archive'    'docs/arch\.zip\{inzip\.txt\}:1:needle inside zip'
keys C-s Space Escape
expect 'deep: files again'                 'Deep › needle .* 7/7'
keys C-M-f; type_ 'blob'
expect 'deep: Ctrl-Alt-F filters the rows' 'Deep filter › blob · pattern: needle .* 1/7'
keys Escape
expect 'deep: Esc back to the pattern'     'Deep › needle +ignored +7/7'

keys Tab
expect 'tab: Folders'                      'Folders › '
clear_query
expect 'folders: parent row'               '\.\.  parent folder'
type_ 'deep'
expect 'folders: filter'                   'src/deep dir'
keys Enter
expect 'folders: Enter goes inside'        'tree/src/deep dir'
expect 'folders: query cleared'            'Folders › type to filter'
expect 'folders: the new folder listed'    '\.\.  parent folder'
shot folders
keys Tab
expect 'git: commits in a subfolder'       'second \(feat\)'
shot git
keys C-c;    expect_log 'git: Ctrl-C copies the hash'  "COPY <$(git -C "$T" rev-parse HEAD)>"
keys C-e C-u M-u C-t
expect_not 'git: a file'"'"'s keys do nothing'  'Not available|REVEAL|history|infile'
[[ $(tm list-windows -t e2e | wc -l) -eq 1 ]] && ok 'git: no window opened' || bad 'git: a window opened'
keys Tab
expect 'help: the manual'                  'sf — the search functions in one window'
type_ 'ctrl-f'
expect 'help: typing finds in it'          'Help › ctrl-f .*/[0-9]+'
keys Escape
expect 'help: Esc clears the search'       'Help › type to find in the manual'
keys Escape
expect 'esc: back to the shell'            '^READY>\s*$'
[[ $(tm display-message -p -t e2e '#W') == shell ]] && ok 'tmux: window name restored' || bad 'tmux: window name not restored'

keys "cd '$W/nogit' && '$SF' git" Enter
expect 'git outside a repository says so'  'not a git repository'
keys Escape
expect 'esc again'                         '^READY>\s*$'
# a repository without a commit, and a bare one: git log --all prints nothing there, sf says why
git init -q "$W/nocommits" 2>/dev/null && git init -q --bare "$W/bare.git" 2>/dev/null
keys "cd '$W/nocommits' && '$SF' git" Enter
expect 'git: no commits yet, in the window' 'this repository has no commits yet'
shot git_nocommits
keys Escape
expect 'git: quit'                         '^READY>\s*$'
keys "clear; '$SF' git --print; echo rc-nocommits=\$?; cd '$W/bare.git' && '$SF' git --print" Enter
expect 'git: --print says it on stderr'    '^sf: this repository has no commits yet'
expect 'git: --print exits 1'              '^rc-nocommits=1'
expect 'git: a bare one says so'           '^sf: bare repository, no commits yet'

n_text=$(cd "$T" && ugrep -rlU --ignore-binary -s --exclude-dir=.git -e '' . | wc -l)
keys "cd '$T' && env -u TMUX '$SF' content" Enter
expect 'no tmux: starts in Search'         'Content › type a pattern'
expect "search: all $n_text text files before typing" "$n_text/$n_text"
type_ 'needle'
expect 'no tmux: results'                  '5/5'
keys C-o
expect_log 'no tmux: Ctrl-O runs nvim'     "NVIM [-c] [let @/ = \$SF_QUERY]"
expect 'no tmux: sf comes back'            'Files +Content +Deep +Folders +Git .*⚙ Config'
expect 'no tmux: results again'            '5/5'
keys C-t
expect 'no tmux: Ctrl-T inside sf'         'In out\.txt › needle .* 1/1'
keys Escape
expect 'no tmux: Esc back to Content'      'Content › needle .* 5/5'
keys Escape
expect 'no tmux: quit'                     '^READY>\s*$'

# no clipboard program for the session: OSC 52 through tmux (load-buffer -w)
keys "clear; env -u WAYLAND_DISPLAY '$SF'" Enter
expect 'osc52: sf starts'                  'Files › type to filter'
type_ 'main'
expect 'osc52: one file'                   "Files › main .* 1/$n_files"
keys C-c
expect 'osc52: status says how'            'Copied through tmux \(OSC 52\)'
[[ $(tm show-buffer) == src/main.c ]] && ok 'osc52: tmux buffer holds the path' || bad "osc52: buffer is '$(tm show-buffer)'"
keys Escape
expect 'osc52: quit'                       '^READY>\s*$'

# WSL: the same keys go through PowerShell with the Windows path in $SF_VALUE
keys "clear; WSL_DISTRO_NAME=e2e '$SF'" Enter
expect 'wsl: sf starts'                    'Files › type to filter'
type_ 'main'
expect 'wsl: one file'                     "Files › main .* 1/$n_files"
keys Enter;  expect_log 'wsl: Enter opens'           "PS {$winT\\src\\main.c} [SF_VALUE] \$ErrorActionPreference='Stop'; Invoke-Item -LiteralPath \$env:SF_VALUE"
keys C-e;    expect_log 'wsl: Ctrl-E selects in Explorer' "PS {$winT\\src\\main.c} [SF_VALUE] \$ErrorActionPreference='Stop'; Start-Process -FilePath explorer.exe"
keys C-c;    expect_log 'wsl: Ctrl-C copies the Windows path' "PS {$winT\\src\\main.c} [SF_VALUE] \$ErrorActionPreference='Stop'; Set-Clipboard -Value \$env:SF_VALUE"
expect 'wsl: copy status'                  'Copied to the clipboard'
keys Escape
expect 'wsl: quit'                         '^READY>\s*$'

# the actions as commands, for the shell functions
keys "clear; '$SF' open src/main.c; echo rc-open=\$?" Enter
expect 'cmd: open exits 0'                 '^rc-open=0'
[[ $(grep -cF "OPEN [$T/src/main.c]" "$LOG") == 2 ]] && ok 'cmd: open ran xdg-open' || bad 'cmd: open ran xdg-open'
keys "'$SF' reveal 'src/deep dir'; '$SF' copy-path ./src/../src/main.c; '$SF' copy-text 'two words'; echo rc-copy=\$?" Enter
expect_log 'cmd: reveal over D-Bus'        "[array:string:file://$uT/src/deep%20dir] [string:]"
expect_log 'cmd: copy-path as typed'       'COPY <./src/../src/main.c>'
expect_log 'cmd: copy-text'                'COPY <two words>'
expect 'cmd: copy exits 0'                 '^rc-copy=0'
keys "'$SF' open gone.txt; echo rc-gone=\$?" Enter
expect 'cmd: missing file is said'         'sf open: no such file or folder: .*/gone\.txt'
expect 'cmd: missing file exits 1'         '^rc-gone=1'
keys "WSL_DISTRO_NAME=e2e '$SF' copy-text e2e-text" Enter
expect_log 'cmd: WSL copy-text'            "PS {e2e-text} [SF_VALUE] \$ErrorActionPreference='Stop'; Set-Clipboard"
keys "clear; '$SF' doctor; echo rc-doctor=\$?" Enter
expect 'doctor: clipboard'                 "Clipboard +wl-copy $STUBS/wl-copy"
expect 'doctor: open'                      "Open +xdg-open $STUBS/xdg-open"
expect 'doctor: exits 0'                   '^rc-doctor=0'
shot doctor

# the command line: a start query, engine options after --, the list printed instead of the window
keys "clear; '$SF' content needle" Enter
expect 'start: Content with the pattern'   'Content filter › type words.* · pattern: needle .* 5/5'
type_ 'src'
expect 'start: typing filters the rows'    'Content filter › src · pattern: needle .* 3/5'
keys Escape
expect 'start: Esc back to the pattern'    'Content › needle +ignored +5/5'
keys Escape
expect 'start: quit'                       '^READY>\s*$'
keys "clear; '$SF' files -- -e md" Enter
expect 'engine options: shown, fd -e md'   'Files › .*-- -e md +1/1'
expect 'engine options: the one .md file'  'docs/readme\.md'
keys Escape
expect 'engine options: quit'              '^READY>\s*$'
keys "clear; '$SF' files main --print; echo rc-print=\$?" Enter
expect 'print: the filtered file'          '^src/main\.c\s*$'
expect 'print: exit 0'                     '^rc-print=0'
keys "clear; '$SF' content needle | wc -l; '$SF' folders deep | cat; '$SF' files zzz --print; echo rc-none=\$?" Enter
expect 'piped: content prints its files'   '^5\s*$'
expect 'piped: folders'                    '^src/deep dir\s*$'
expect 'print: nothing found exits 1'      '^rc-none=1'
# --print0: a newline in a name stays inside its record (shown here as ~, the NUL as | or #)
mkdir "$W/names" && printf 'needle\n' > "$W/names/a"$'\n'"b.txt" && printf 'needle\n' > "$W/names/c d.txt"
keys "clear; cd '$W/names'; '$SF' content needle --print0 | tr '\0\n' '|~'; echo; '$SF' files --print0 | tr '\0\n' '#~'; echo; cd '$T'" Enter
expect 'print0: content, a NUL after each path' '^a~b\.txt\|c d\.txt\|\s*$'
expect 'print0: files, a NUL after each path'   '^a~b\.txt#c d\.txt#\s*$'
# a file and a folder sf may not read: one line says so (as Bilal's user: root reads them anyway)
mkdir -p "$W/perms/locked" && printf 'needle\n' | tee "$W/perms/r.txt" "$W/perms/u.txt" "$W/perms/locked/x.txt" >/dev/null
chmod 000 "$W/perms/u.txt" "$W/perms/locked"
keys "clear; cd '$W/perms'; '$SF' content needle --print 2>&1 | cat; echo rc-perms=\${PIPESTATUS[0]}" Enter
expect 'perms: --print names the count'    '^2 paths could not be read\s*$'
expect 'perms: --print exits 2'            '^rc-perms=2'
keys "clear; '$SF' content needle" Enter
expect 'perms: the window says it once'    '2 paths could not be read'
expect 'perms: the readable file listed'   'Content filter › .* · pattern: needle .* 1/1'
[[ $(pane | grep -c 'could not be read') -eq 1 ]] && ok 'perms: one line, not one a path' || bad 'perms: more than one line'
shot perms
keys Escape
expect 'perms: Esc back to the pattern'    'Content › needle .* 1/1'
keys Escape
expect 'perms: quit'                       '^READY>\s*$'
chmod 700 "$W/perms/u.txt" "$W/perms/locked"
keys "cd '$T'" Enter
# UTF-16 with its byte order mark: ugrep reads it as text, so Content, Deep and the preview find the words
mkdir "$W/utf16" && { printf '\xff\xfe'; printf 'hello needle\n' | iconv -f UTF-8 -t UTF-16LE; } > "$W/utf16/le.txt"
keys "clear; cd '$W/utf16'; '$SF' content needle | cat; '$SF' deep needle | sed 's/^/deep:/'" Enter
expect 'utf-16: Content lists it'          '^le\.txt\s*$'
expect 'utf-16: Deep lists it'             '^deep:le\.txt\s*$'
keys "clear; '$SF' content needle" Enter
expect 'utf-16: the preview shows the line' '1:hello needle'
keys Escape
expect 'utf-16: Esc back to the pattern'   'Content › needle .* 1/1'
keys Escape
expect 'utf-16: quit'                      '^READY>\s*$'
# a name with a newline: one row in Content and in Deep, and Enter opens it; the OPEN stub's log line
# breaks at the newline, so the path's halves are two log lines
mkdir "$W/newline" && printf 'needle\n' > "$W/newline/x"$'\n'"y.txt"
# nl_opened NAME N: the path opened N times so far, each time both halves
nl_opened() {
    local i n
    for i in $(seq 50); do n=$(grep -cxF "OPEN [$W/newline/x" "$LOG"); (( n >= $2 )) && break; sleep 0.1; done
    if [[ $n -eq $2 && $(grep -A1 -xF "OPEN [$W/newline/x" "$LOG" | grep -cxF 'y.txt]') -eq $2 ]]; then
        ok "$1"
    else
        bad "$1 ($n OPEN lines for x<LF>y.txt, want $2)"; tail -4 "$LOG"
    fi
}
keys "clear; cd '$W/newline'; '$SF' content needle" Enter
expect 'newline: Content, one row'         'Content filter › .* · pattern: needle .* 1/1'
keys Enter; nl_opened 'newline: Content, Enter opens it' 1
keys Escape
expect 'newline: Esc back to the pattern'  'Content › needle .* 1/1'
keys Escape
expect 'newline: quit Content'             '^READY>\s*$'
keys "clear; '$SF' deep needle" Enter
expect 'newline: Deep, one row'            'Deep filter › .* · pattern: needle .* 1/1'
keys Enter; nl_opened 'newline: Deep, Enter opens it' 2
keys Escape
expect 'newline: Esc back to Deep'         'Deep › needle .* 1/1'
keys Escape
expect 'newline: quit Deep'                '^READY>\s*$'
keys "cd '$T'" Enter
keys "clear; '$SF' deep needle | grep -F 'arch.zip{'" Enter
expect "piped: deep prints ugrep's members" '^docs/arch\.zip\{inzip\.txt\}\s*$'
keys "clear; '$SF' files -- -l; echo rc-refused=\$?" Enter
expect 'refused: the reason'               'sf files: -l is refused'
expect 'refused: exit 2'                   '^rc-refused=2'

# a release package's layout, bin/sf beside lib/sf: sf takes its engines from lib/sf first and puts
# lib/sf in front of the PATH of what it starts, tmux windows included (here an nvim of the bundle)
PKG=$W/pkg
mkdir -p "$PKG/bin" "$PKG/lib/sf"
cp "$SF" "$PKG/bin/sf"
ln -s "$FD" "$PKG/lib/sf/fd"; ln -s "$(command -v ugrep)" "$PKG/lib/sf/ugrep"   # the pane's PATH may have neither
cat > "$PKG/lib/sf/nvim" <<EOF
#!/bin/bash
printf 'BUNDLED-NVIM %s PATH=%s\n' "\${@: -1}" "\$PATH" >> "$LOG"
sleep 1
EOF
chmod +x "$PKG/lib/sf/nvim"
keys "clear; '$PKG/bin/sf' doctor; echo rc-bundle=\$?" Enter
expect 'bundle: ugrep from lib/sf'         "  ugrep +$PKG/lib/sf/ugrep "
expect 'bundle: the editor from lib/sf'    "  editor +$PKG/lib/sf/nvim "
expect 'bundle: doctor exits 0'            '^rc-bundle=0'
keys "clear; cd '$T' && '$PKG/bin/sf'" Enter
expect 'bundle: sf starts'                 'Files › type to filter'
type_ 'main'
expect 'bundle: one file'                  "Files › main .* 1/$n_files"
keys C-o
expect_log 'bundle: Ctrl-O in a window, lib/sf first in its PATH' "BUNDLED-NVIM $T/src/main.c PATH=$PKG/lib/sf:$STUBS:"
back_to_sf
keys Escape
expect 'bundle: quit'                      '^READY>\s*$'

# the views over real git and delta: a changed file's diff (and / searching it), its history, its log,
# a commit; Esc or q goes back
printf 'changed by e2e\n' >> "$T/empty.txt"
keys "clear; '$SF' files empty" Enter
expect 'views: sf starts'                  'Files › empty .* 1/'
keys C-g
# delta's numbered line: only the loaded view has it (Files' preview shows the line too, as bat's);
# without delta the view is git's own diff
if [[ $DELTA == /* ]]; then
    expect 'views: Ctrl-G shows the change'    '│changed by e2e'
else
    expect 'views: Ctrl-G shows git'"'"'s diff'   '\+changed by e2e'
    skip "views: delta's numbered line (no delta in the pane's PATH)"
fi
shot view_diff
keys /
type_ 'changed'
expect 'views: / types the search'         '^/ changed'
keys Enter
expect 'views: the keys again'             'Esc back'
keys Escape; back_to_sf
keys C-u
expect 'views: Ctrl-U the history'         'empty\.txt · history with changes'
[[ $(tm display-message -p -t e2e '#W') == history ]] && ok 'views: Ctrl-U in a window, history' || bad 'views: no history window'
keys Escape; back_to_sf
keys M-u
expect 'views: Alt-U the log'              'first commit'
[[ $(tm display-message -p -t e2e '#W') == log ]] && ok 'views: Alt-U in a window, log' || bad 'views: no log window'
keys q; back_to_sf
expect 'views: q closes the window'        'Files › empty'
keys Escape
expect 'views: quit'                       '^READY>\s*$'
keys "clear; '$SF' git" Enter
expect 'views: the Git tab'                'second \(feat\)'
keys C-f
type_ 'feat'
expect 'git: Ctrl-F finds in the preview'  'Find in preview › feat'
expect 'git: what and how often, top right' ' feat · 1/[0-9]+ ╮'
red_on 'feat' && ok 'git: Ctrl-F marks the match red' || bad 'git: Ctrl-F marks no match red'
keys C-s
expect 'git: Ctrl-S while finding, Find'"'"'s options'  ' Find options .*'
expect 'git: Find has a regular expression' 'Regular expression'
keys Space Escape
expect 'git: Whole word on the input line' 'Find in preview › feat .*whole word'
find_saved=$(awk '/^\[/ { s = ($0 == "[find]") } s' "$XDG/sf/config.toml" 2>/dev/null)
[[ $find_saved == $'[find]\nmatch_case = false\nwhole_word = true\nregex = false'* ]] \
    && ok 'git: Find'"'"'s options saved as [find]' || bad "git: [find] not saved: ${find_saved:-none}"
keys Escape
expect 'git: Esc back to the list'         'Git › '
keys C-M-f
expect_not 'versions: Ctrl-Alt-F is not it'  'Search in versions'
keys C-M-S-f
expect 'versions: Ctrl-Shift-Alt-F, every version' 'Search in versions › type a pattern'
expect 'versions: nothing typed, every file'"'"'s history' 'Search in versions › .* 12/12'
type_ 'needle'
expect 'versions: both commits, five files each' 'Search in versions › needle .* 10/10'
expect 'versions: a row names the version'  '[0-9a-f]{7} [0-9]{4}-[0-9]{2}-[0-9]{2} src/main\.c:1:needle in main'
keys C-s
expect 'versions: its own options'          ' Search in versions options '
expect 'versions: the commits to choose'    '\[x\] [0-9a-f]{7}  [0-9-]{10}  T  [a-z]+  second \(feat\)'
keys Down Down Down Space Escape
expect 'versions: Documents, the zip member' 'docs/arch\.zip\{inzip\.txt\}:1:needle inside zip'
expect 'versions: two rows more'            'Search in versions › needle .* 12/12'
keys C-s Down Down Down Space Escape
keys Escape
expect 'versions: Esc back to the commits'  'HEAD -> master\) second \(feat\)'
keys Enter
expect 'views: Enter shows the commit'     'commit [0-9a-f]{40}'
shot view_commit
keys Escape
expect 'views: back in Git'                'Git › '
keys Escape
expect 'views: done'                       '^READY>\s*$'

# the palette from a config of its own: [theme] border = 99 draws every border in 256-colour 99
mkdir -p "$W/xdg-theme/sf" && printf '[theme]\nborder = 99\n' > "$W/xdg-theme/sf/config.toml"
keys "clear; cd '$T' && XDG_CONFIG_HOME='$W/xdg-theme' '$SF'" Enter
expect 'theme: sf starts with that config' 'Files › type to filter'
border_in 99 && ok 'theme: [theme] border = 99 gives 38;5;99' || bad 'theme: no border in 38;5;99'
tm capture-pane -e -p -t e2e | grep -qF '38;5;240m' && bad 'theme: the default 240 still drawn' || ok 'theme: no 240 left'
shot theme
keys Escape
expect 'theme: quit'                       '^READY>\s*$'
[[ $(cat "$W/xdg-theme/sf/config.toml") == $'[theme]\nborder = 99' ]] && ok 'theme: its config untouched' || bad 'theme: its config changed'

echo "pass=$PASS fail=$FAIL skip=$SKIP (screens in $SHOTS)"
tm kill-server
[[ $FAIL -eq 0 ]]
