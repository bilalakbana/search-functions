#!/bin/bash
# End-to-end test of the sf binary in a private tmux server (socket "sfrs-e2e", never Bilal's own):
# the fixture tree of tests/tools/make_fixture.sh, the real engines (fd, ugrep, ugrep+, bat, git, delta),
# and stubs for everything that would reach the desktop (xdg-open, wl-copy, nvim, the sub-functions).
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
T=$W/tree LOG=$W/actions.log STUBS=$W/stubs SUB=$W/sub XDG=$W/xdg SHOTS=$W/screens
mkdir -p "$STUBS" "$SUB" "$XDG" "$SHOTS" "$W/nogit"
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
stub "$STUBS" nvim NVIM 1
stub "$SUB" open_in_file_manager REVEAL
stub "$SUB" search_in_single_file INFILE 3
for s in show_diffs git_diff git_diff_short show_logs; do stub "$SUB" "$s" "$s" 1; done

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
    COLORTERM=truecolor LANG=C.UTF-8 XDG_CONFIG_HOME=$XDG SF_SUBFUNCTIONS=$SUB LESS=-R \
    TMUX=\$TMUX TMUX_PANE=\$TMUX_PANE bash --norc --noprofile"
keys "PS1='READY> '; cd '$T'" Enter
wait_pane '^READY>' || { echo "e2e: shell did not start" >&2; exit 2; }
tm rename-window -t e2e shell

n_files=$(cd "$T" && fd --type f --hidden --no-ignore --exclude .git . | wc -l)
keys "'$SF'" Enter
expect 'start: tab bar with Config'        'Files +Content +Folders +Git .*⚙ Config'
expect "start: $n_files files listed"      "$n_files/$n_files"
expect 'start: bat preview with numbers'   '│ *1 build/'
expect 'start: footer'                     'Enter open · \^O nvim'
[[ $(tm display-message -p -t e2e '#W') == search ]] && ok 'tmux: window named search' || bad 'tmux: window not renamed'
shot files
type_ 'main'
expect 'files: query filters'              "Files › main .* 1/$n_files"
red_on 'main' && ok 'files: typed text highlighted' || bad 'files: typed text not highlighted'
keys Enter;  expect_log 'files: Enter opens'           "OPEN [$T/src/main.c]"
keys C-c;    expect_log 'files: Ctrl-C copies'         'COPY <src/main.c>'
expect 'files: copy shows a status'        'Copied to the clipboard'
keys C-o;    expect_log 'files: Ctrl-O nvim in tmux'   "NVIM [--] [$T/src/main.c]"; back_to_sf
keys C-e;    expect_log 'files: Ctrl-E reveals'        "REVEAL [$T/src/main.c]"
keys C-g;    expect_log 'files: Ctrl-G git diff'       "git_diff [$T/src/main.c]"; back_to_sf

keys Tab
expect 'tab: Search keeps the query'       'Content › main'
expect 'search: one file'                  'src/main\.c'
clear_query; type_ 'needle'
expect 'search: files with needle'         '5/5'
expect 'search: preview of the first file' '1:needle hidden'
expect 'search: match count in the preview' 'Matches +1'
expect 'search: file in the preview header' 'File +\.hidden/secret\.txt'
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
expect_log 'lines: Ctrl-T passes the pattern' "INFILE [$T/"
grep -q "INFILE .*\[needle\]" "$LOG" && ok 'lines: Ctrl-T got the pattern' || bad 'lines: Ctrl-T pattern'
tm list-windows -t e2e -F '#W' | grep -qx infile && ok 'tmux: infile window' || bad 'tmux: no infile window'
back_to_sf
keys C-s Down Space Escape
expect 'archives: zip member line'         'docs/arch\.zip\{inzip\.txt\}:1:needle inside zip'
keys C-s Space Down Space Escape
expect 'archives off, lines off'           '5/5'

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

n_text=$(cd "$T" && ugrep -rlU --ignore-binary -s --hidden --ignore-files --exclude-dir=.git -e '' . | wc -l)
keys "cd '$T' && env -u TMUX '$SF' content" Enter
expect 'no tmux: starts in Search'         'Content › type a pattern'
expect "search: all $n_text text files before typing" "$n_text/$n_text"
type_ 'needle'
expect 'no tmux: results'                  '5/5'
keys C-o
expect_log 'no tmux: Ctrl-O runs nvim'     "NVIM [-c] [let @/ = \$SF_QUERY]"
expect 'no tmux: sf comes back'            'Files +Content +Folders +Git .*⚙ Config'
expect 'no tmux: results again'            '5/5'
keys Escape
expect 'no tmux: quit'                     '^READY>\s*$'

echo "pass=$PASS fail=$FAIL (screens in $SHOTS)"
tm kill-server
[[ $FAIL -eq 0 ]]
