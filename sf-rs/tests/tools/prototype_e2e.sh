#!/bin/bash
# End-to-end test of sf in a private tmux server (socket "sfe2e"): real fzf ($1), test copy of the repo,
# stubbed xdg-open / wl-copy / nvim / open_in_file_manager.
S=/tmp/claude-1000/-home-bilal-Downloads/530c2bed-c831-4fb3-acf7-ab6e4708ef18/scratchpad
FZFDIR=$(dirname "$1"); LOG=$S/sf_e2e.log; T=$S/sft
tm() { tmux -L sfe2e -f /dev/null "$@"; }
pane() { tm capture-pane -p -t e2e | sed -E 's/\x1b\[[0-9;]*m//g'; }
PASS=0; FAIL=0
ok() { echo "PASS  $*"; PASS=$((PASS+1)); }
bad() { echo "FAIL  $*"; FAIL=$((FAIL+1)); pane | grep -v '^\s*$' | tail -12; }
wait_pane() { local i; for i in $(seq 80); do pane | grep -qE -- "$1" && return 0; sleep 0.1; done; return 1; }
expect() { if wait_pane "$2"; then ok "$1"; else bad "$1 (no /$2/ on screen)"; fi; }
expect_log() { local i; for i in $(seq 50); do grep -qF -- "$2" "$LOG" && { ok "$1"; return; }; sleep 0.1; done; bad "$1 (log lacks: $2)"; }
keys() { tm send-keys -t e2e "$@"; }
type_() { tm send-keys -t e2e -l "$1"; }

printf '#!/bin/sh\nprintf "NVIM [%%s]\\n" "$@" >> %s\n' "$LOG" > $S/sfstubs/nvim; chmod +x $S/sfstubs/nvim
: > "$LOG"
tm kill-server 2>/dev/null
tm new-session -d -s e2e -x 200 -y 45 "env -i HOME=$HOME PATH=$S/sfstubs:$FZFDIR:/usr/local/bin:/usr/bin:/bin TERM=xterm-256color LANG=C.UTF-8 SF_TEST_LOG=$LOG LESS=-R bash --norc --noprofile"
keys "PS1='READY> '; cd $T" Enter
wait_pane '^READY>' || { echo "shell did not start"; exit 1; }

keys "$S/sfcopy/sf" Enter
expect 'start: Files prompt'            '│ Files> '
expect 'start: header shows modes'      'F2 Files .* F3 Content .* F6 Git .*F1 help'
expect 'start: header shows toggles'    'F7 archives & PDFs +F8 hidden & ignored'
expect 'start: 9 files listed'          '9/9'
type_ 'main'
expect 'files: query filters'           '1/9'
keys Enter;  expect_log 'files: Enter opens'           "OPEN [$T/src/main.c]"
keys C-c;    expect_log 'files: Ctrl-c copies'         'COPY [src/main.c]'
expect 'files: copy shows a label'      'Copied to the clipboard'
keys C-o;    expect_log 'files: Ctrl-o runs nvim'      "NVIM [$T/src/main.c]"

keys Tab
expect 'tab: Content prompt'            '│ Content> main'
expect 'content: query kept, live search' '1/1'
keys BSpace BSpace BSpace BSpace; type_ 'needle'
expect 'content: live search needle'    '4/4'
keys BSpace BSpace BSpace BSpace BSpace BSpace; type_ 'zzznotthere'
expect 'content: no-match hint'         'No match\. F7 also searches inside archives'
keys C-M-h; type_ 'needle'
expect 'content: back to needle'        '4/4'
keys F7
expect 'alt-d: +deep in prompt'         '│ Content \+archives> needle'
expect 'content +deep: archive + binary' '6/6'
keys BSpace BSpace BSpace BSpace BSpace BSpace; type_ 'foo('
expect 'content: broken pattern shown'  'mismatched'
keys BSpace BSpace BSpace BSpace

keys Tab
expect 'tab: Lines +deep'               '│ Lines \+archives> '
type_ 'zip'
expect 'lines +deep: zip member line'   'arch.zip\{inzip.txt\}:1:needle inside zip'
keys C-o;    expect_log 'lines: Ctrl-o opens at line'  "NVIM [+1]"
keys F7
expect 'alt-d off'                      '│ Lines> zip'
keys BSpace BSpace BSpace

keys F1
expect 'F1: help in preview'            'all search functions in one window'
keys F5
expect 'F5: Folders'                    '│ Folders> '
type_ 'deep'
expect 'folders: filter'                'src/deep dir/'
keys Enter
expect 'folders: Enter re-roots (become)' 'a b&c.txt'
expect 'folders: new root in label'     'deep dir'
keys F6
expect 'F6: Git in subfolder'           'second \(feat\)'
keys C-c;    expect_log 'git: Ctrl-c copies hash'      "COPY [$(git -C $T rev-parse HEAD)]"
keys C-e
expect 'git: Ctrl-e not available'      'Not available in Git mode'
keys BTab
expect 'shift-tab: back to Folders'     '│ Folders> '
keys Escape
expect 'esc: back to the shell'         '^READY>\s*$'

echo "pass=$PASS fail=$FAIL ($("$1" --version))"
tm kill-server
