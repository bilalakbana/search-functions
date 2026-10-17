#!/bin/bash
# Test of sub-functions/platform_action, the open/reveal/copy wrapper of the fzf functions: with sf-rs in PATH it
# hands over to sf, without it uses xdg-open / open_in_file_manager (beside it) / wl-copy. The script runs as a
# copy in WORKDIR, so its open_in_file_manager is a stub; every program is a stub that logs its arguments,
# HOME is a fake one, and tmux is a stub (never a real server).
#
# usage: tests/platform_action.sh WORKDIR   (WORKDIR: a new, empty folder; nothing in it is ever deleted)
#        SF_BIN=path/to/sf tests/platform_action.sh WORKDIR   (default: target/debug/sf)
set -u
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
W=${1:?usage: platform_action.sh WORKDIR}
mkdir -p "$W" && W=$(realpath -- "$W")
[[ -z $(ls -A "$W") ]] || { echo "platform_action.sh: $W is not empty" >&2; exit 2; }
LOG=$W/log WITH=$W/with-sf OLD=$W/old-sf PLAIN=$W/plain FAKE_HOME=$W/home SUB=$W/sub
mkdir -p "$WITH" "$OLD" "$PLAIN" "$FAKE_HOME" "$SUB"
cp "$here/../../sub-functions/platform_action" "$SUB/" || exit 2
ACTION=$SUB/platform_action

# stub DIR NAME [HELP]: logs "NAME|arg|arg..." and "<stdin>" when there is any; prints $DIR/NAME.err to stderr
# and exits with the code in $DIR/NAME.exit when those files are there. With HELP, --help prints it (unlogged).
stub() {
    local help=${3:-}
    cat > "$1/$2" <<EOF
#!/bin/bash
[[ \$1 == --help && -n "$help" ]] && { printf '%s\n' "$help"; exit 0; }
line="$2"; for a in "\$@"; do line+="|\$a"; done
[[ -t 0 ]] || { input=\$(cat); [[ -n \$input ]] && line+=" <\$input>"; }
printf '%s\n' "\$line" >> "$LOG"
[[ -f "$1/$2.err" ]] && cat "$1/$2.err" >&2
[[ -f "$1/$2.exit" ]] && exit "\$(cat "$1/$2.exit")"
exit 0
EOF
    chmod +x "$1/$2"
}
for d in "$WITH" "$OLD" "$PLAIN"; do for p in xdg-open wl-copy tmux; do stub "$d" "$p"; done; done
stub "$WITH" sf "usage: sf [files|content|folders|git]
       sf open|reveal|copy-path PATH"
# an sf from before the actions (or another program called sf): its usage does not name them
stub "$OLD" sf "usage: sf [files|content|folders|git]"
stub "$SUB" open_in_file_manager

PASS=0 FAIL=0
ok() { echo "PASS  $*"; PASS=$((PASS + 1)); }
bad() { echo "FAIL  $*"; FAIL=$((FAIL + 1)); }
# act PATHDIR TMUXVALUE ARGS...: runs platform_action in a clean environment; sets $code and $err
act() {
    local dir=$1 tmux=$2; shift 2
    : > "$LOG"
    err=$(env -i HOME="$FAKE_HOME" PATH="$dir:/usr/bin:/bin" ${tmux:+TMUX=$tmux} "$ACTION" "$@" 2>&1 </dev/null)
    code=$?
}
logged() { [[ $(cat "$LOG") == "$1" ]]; }
check() { if eval "$2"; then ok "$1"; else bad "$1 (code=$code err=$err log=$(tr '\n' '/' < "$LOG"))"; fi; }

# the contract with the real sf: its usage holds the line platform_action looks for
SF_BIN=${SF_BIN:-$here/../target/debug/sf}
if "$SF_BIN" --help | grep -qF 'sf open|reveal|copy-path PATH'; then
    ok 'sf: its usage names the actions'
else
    bad "sf: $SF_BIN --help lacks 'sf open|reveal|copy-path PATH'"
fi

act "$WITH" "" open './-a b#(touch x)'
check 'sf: open is handed over'            'logged "sf|open|./-a b#(touch x)" && [[ $code == 0 ]]'
act "$WITH" "" reveal /tmp
check 'sf: reveal'                         'logged "sf|reveal|/tmp"'
act "$WITH" "" copy-path src/a.c
check 'sf: copy-path'                      'logged "sf|copy-path|src/a.c"'
act "$WITH" "" copy-text 'two words'
check 'sf: copy-text'                      'logged "sf|copy-text|two words"'

echo 1 > "$WITH/sf.exit"; echo 'sf open: no such file or folder: /x' > "$WITH/sf.err"
act "$WITH" "" open /x
check 'sf fails: exit 1 and its message'   '[[ $code == 1 && $err == "platform_action: sf open: no such file or folder: /x" ]]'
check 'sf fails: no tmux outside it'       'logged "sf|open|/x"'
act "$WITH" /tmp/fake-tmux,1,0 open /x
check 'sf fails in tmux: status line'      'logged "sf|open|/x
tmux|display-message|-l|--|platform_action: sf open: no such file or folder: /x"'
reset_sf() { printf 0 > "$WITH/sf.exit"; : > "$WITH/sf.err"; }
reset_sf

act "$OLD" "" open './a b'
check 'old sf: open with xdg-open'         'logged "xdg-open|./a b" && [[ $code == 0 ]]'
act "$OLD" "" copy-text abc
check 'old sf: copy with wl-copy'          'logged "wl-copy <abc>"'

act "$PLAIN" "" open './a b'
check 'no sf: open with xdg-open'          'logged "xdg-open|./a b" && [[ $code == 0 ]]'
act "$PLAIN" "" reveal '/r/a b'
check 'no sf: reveal with open_in_file_manager' 'logged "open_in_file_manager|/r/a b"'
act "$PLAIN" "" copy-path 'src/a b.c'
check 'no sf: copy-path with wl-copy'      'logged "wl-copy <src/a b.c>"'
act "$PLAIN" "" copy-text abc
check 'no sf: copy-text with wl-copy'      'logged "wl-copy <abc>"'
echo 4 > "$PLAIN/xdg-open.exit"
act "$PLAIN" "" open ./a
check 'no sf: xdg-open failure is said'    '[[ $code == 1 && $err == "platform_action: xdg-open failed (exit 4): ./a" ]]'

act "$WITH" "" open
check 'one argument: usage, nothing run'   '[[ $code == 1 && $err == *usage* ]] && logged ""'
act "$WITH" "" delete /x
check 'unknown action: nothing run'        '[[ $code == 1 && $err == *"unknown action: delete"* ]] && logged ""'

# The binds of a real fzf function (sub-functions/file_list) reach platform_action and so the stub sf; in a
# private tmux server (socket "sfrs-platform-action", never Bilal's own).
T=$W/tree
mkdir -p "$T/d" && : > "$T/d/x y.txt"
tm() { tmux -L sfrs-platform-action -f /dev/null "$@"; }
tm kill-server 2>/dev/null
fzf_dir=$(dirname -- "$(command -v fzf)")   # e.g. ~/.local/bin; the stub sf stays first
tm new-session -d -s pa -x 120 -y 30 "env -i HOME=$HOME PATH=$WITH:$fzf_dir:/usr/local/bin:/usr/bin:/bin TERM=xterm-256color \
    LANG=C.UTF-8 bash --norc --noprofile"
pane() { tm capture-pane -p -t pa; }
wait_for() { local i; for i in $(seq 50); do eval "$1" && return 0; sleep 0.1; done; return 1; }
: > "$LOG"
tm send-keys -t pa "cd '$T' && ~/.SearchFunctions/sub-functions/file_list" Enter
if wait_for 'pane | grep -q "d/x y.txt"'; then
    tm send-keys -t pa Enter
    wait_for 'grep -qxF "sf|open|./d/x y.txt" "$LOG"' && ok 'fzf: Enter opens through sf' || bad 'fzf: Enter'
    tm send-keys -t pa C-e
    wait_for 'grep -qxF "sf|reveal|$T/d/x y.txt" "$LOG"' && ok 'fzf: Ctrl-E reveals through sf' || bad 'fzf: Ctrl-E'
    tm send-keys -t pa C-c
    wait_for 'grep -qxF "sf|copy-path|d/x y.txt" "$LOG"' && ok 'fzf: Ctrl-C copies through sf' || bad 'fzf: Ctrl-C'
else
    bad "fzf: file_list did not show the file ($(pane | grep -v '^\s*$' | tail -3 | tr '\n' '/'))"
fi
tm kill-server

echo "pass=$PASS fail=$FAIL"
[[ $FAIL -eq 0 ]]
