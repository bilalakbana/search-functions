#!/bin/bash
# End-to-end test of the sf binary in a private tmux server (socket "sfrs-e2e", never Bilal's own):
# the fixture tree of tests/tools/make_fixture.sh, the real engines (fd, ugrep, ugrep+, bat, git, delta),
# and stubs for everything that would reach the desktop (xdg-open, wl-copy, dbus-send, nvim, the
# sub-functions, and WSL's powershell.exe and wslpath).
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
PASS=0 FAIL=0
ok() { echo "PASS  $*"; PASS=$((PASS + 1)); }
bad() { echo "FAIL  $*"; FAIL=$((FAIL + 1)); pane | grep -v '^\s*$' | tail -15; }
wait_pane() { local i; for i in $(seq 60); do pane | grep -qE -- "$1" && return 0; sleep 0.1; done; return 1; }
expect() { if wait_pane "$2"; then ok "$1"; else bad "$1 (no /$2/ on screen)"; fi; }
expect_not() { sleep 0.4; if pane | grep -qE -- "$2"; then bad "$1 (/$2/ still on screen)"; else ok "$1"; fi; }
expect_log() { local i; for i in $(seq 50); do grep -qF -- "$2" "$LOG" && { ok "$1"; return; }; sleep 0.1; done; bad "$1 (log lacks: $2)"; tail -3 "$LOG"; }
keys() { tm send-keys -t e2e "$@"; }
type_() { tm send-keys -t e2e -l "$1"; }
shot() { tm capture-pane -e -p -t e2e > "$SHOTS/$1.ansi"; }
# red_on TEXT: TEXT is drawn in Catppuccin red somewhere on the screen (a highlighted match)
red_on() { local i; for i in $(seq 30); do tm capture-pane -e -p -t e2e | grep -qF -- "237;135;150m$1" && return 0; sleep 0.1; done; return 1; }
clear_query() { keys C-M-h; }
# back_to_sf: a terminal action opened a tmux window (it has the focus); wait until its stub ends
back_to_sf() {
    local i
    for i in $(seq 60); do [[ $(tm list-windows -t e2e | wc -l) -eq 1 ]] && break; sleep 0.1; done
    tm select-window -t e2e:0
}

tm kill-server 2>/dev/null
# \$TMUX and \$TMUX_PANE are the private server's (expanded inside the pane), never the caller's own tmux
tm new-session -d -s e2e -x 160 -y 40 "env -i HOME=$HOME PATH=$STUBS:/usr/local/bin:/usr/bin:/bin TERM=xterm-256color \
    COLORTERM=truecolor LANG=C.UTF-8 XDG_CONFIG_HOME=$XDG LESS=-R WAYLAND_DISPLAY=wayland-e2e \
    TMUX=\$TMUX TMUX_PANE=\$TMUX_PANE bash --norc --noprofile"
keys "PS1='READY> '; cd '$T'" Enter
wait_pane '^READY>' || { echo "e2e: shell did not start" >&2; exit 2; }
tm rename-window -t e2e shell

# the defaults: hidden files off, ignored files on
n_files=$(cd "$T" && fd --type f --no-ignore --exclude .git . | wc -l)
keys "'$SF'" Enter
expect 'start: tab bar with Config'        'Files +Content +Deep +Folders +Git .*⚙ Config'
expect "start: $n_files files listed"      "$n_files/$n_files"
expect_not 'start: Files without a preview' '│ *1 needle ignored'
keys C-l
expect 'files: Ctrl-L shows bat with numbers' '│ *1 needle ignored'
expect 'start: footer'                     'Enter open · \^S options · \^O nvim'
[[ $(tm display-message -p -t e2e '#W') == search ]] && ok 'tmux: window named search' || bad 'tmux: window not renamed'
shot files
keys C-s
expect 'types: under the options'          '── File types ──'
keys Down Down Down Down Space
expect 'types: .bin turned off'            '\[ \] \.bin'
keys Escape
expect 'types: the list without it'        "Files › .*1 type off +$((n_files - 1))/$n_files"
keys C-s a Escape
expect 'types: a turns all on again'       "Files › .*ignored +$n_files/$n_files"
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
expect 'files: an unchanged file says so'  'No local changes in .*main\.c'
keys Escape
expect 'files: Esc leaves the view'        "Files › main .* 1/$n_files"
keys C-t
expect "files: Ctrl-T the file's lines"    'In main\.c › .* 3/3'
type_ 'second more'
expect 'files: Ctrl-T any of the words'    'In main\.c › second more .* 2/2'
keys Escape
expect 'files: Esc back to Files'          "Files › main .* 1/$n_files"

keys Tab
expect 'tab: Search keeps the query'       'Content › main'
expect 'search: one file'                  'src/main\.c'
clear_query; type_ 'needle'
expect 'search: files with needle'         '5/5'
expect 'search: preview numbers the matches' 'Match #1'
expect 'search: preview of the first file' '1: needle ignored'
expect 'search: match count in the preview' 'Matches +1'
expect 'search: file in the preview header' 'File +build/out\.txt'
shot search
clear_query; type_ 'zzznotthere'
expect 'search: no-match hint'             'No match\. Archives & documents'
clear_query; type_ 'foo('
expect 'search: broken pattern shown'      'mismatched'
clear_query; type_ 'needle'
expect 'search: back to needle'            '5/5'
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
keys C-f; type_ 'blob'
expect 'deep: Ctrl-F filters the rows'     'Deep › needle · filter: blob .* 1/7'
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
keys C-e
expect 'git: other keys not available'     'Not available in Git'
keys Escape
expect 'esc: back to the shell'            '^READY>\s*$'
[[ $(tm display-message -p -t e2e '#W') == shell ]] && ok 'tmux: window name restored' || bad 'tmux: window name not restored'

keys "cd '$W/nogit' && '$SF' git" Enter
expect 'git outside a repository says so'  'not a git repository'
keys Escape
expect 'esc again'                         '^READY>\s*$'

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
expect 'start: Content with the pattern'   'Content › needle · filter: .* 5/5'
type_ 'src'
expect 'start: typing filters the rows'    'Content › needle · filter: src .* 3/5'
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
for e in fd ugrep; do ln -s "$(command -v "$e")" "$PKG/lib/sf/$e"; done   # the pane's PATH may have neither
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
expect 'views: Ctrl-G shows the change'    'changed by e2e'
shot view_diff
keys /
type_ 'changed'
expect 'views: / types the search'         '^/ changed'
keys Enter
expect 'views: the keys again'             'Esc back'
keys Escape
keys C-u
expect 'views: Ctrl-U the history'         'empty\.txt · history with changes'
keys Escape
keys M-u
expect 'views: Alt-U the log'              'first commit'
keys q
expect 'views: q goes back'                'Files › empty'
keys Escape
expect 'views: quit'                       '^READY>\s*$'
keys "clear; '$SF' git" Enter
expect 'views: the Git tab'                'second \(feat\)'
keys Enter
expect 'views: Enter shows the commit'     'commit [0-9a-f]{40}'
shot view_commit
keys Escape
expect 'views: back in Git'                'Git › '
keys Escape
expect 'views: done'                       '^READY>\s*$'

echo "pass=$PASS fail=$FAIL (screens in $SHOTS)"
tm kill-server
[[ $FAIL -eq 0 ]]
