#!/bin/bash
# Test of the sub-functions a release package ships beside sf (lib/sf/sub-functions): they find each other in
# their own folder, never at ~/.SearchFunctions (HOME is a fake one here), and hand their PATH to the tmux
# windows they open; show_diffs works without the npm tool git-split-diffs (delta side by side). The scripts
# under test are copies in WORKDIR; their neighbours, delta and less are stubs that log. tmux runs as a
# private server (socket "sfrs-sub-functions", never Bilal's own).
#
# usage: tests/sub_functions.sh WORKDIR   (WORKDIR: a new, empty folder; nothing in it is ever deleted)
#        SUB_SRC=DIR tests/sub_functions.sh WORKDIR   (the scripts from DIR, e.g. a release package's
#        lib/sf/sub-functions; default: the repo's sub-functions)
set -u
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_SUB=$(realpath -- "${SUB_SRC:-$here/../../sub-functions}") || exit 2
W=${1:?usage: sub_functions.sh WORKDIR}
mkdir -p "$W" && W=$(realpath -- "$W")
[[ -z $(ls -A "$W") ]] || { echo "sub_functions.sh: $W is not empty" >&2; exit 2; }
LOG=$W/log SUB=$W/pkg/lib/sf/sub-functions STUBS=$W/stubs FAKE_HOME=$W/home T=$W/tree
mkdir -p "$SUB" "$STUBS" "$FAKE_HOME"
: > "$LOG"
bash "$here/tools/make_fixture.sh" "$T" >/dev/null || { echo "sub_functions.sh: fixture failed" >&2; exit 2; }

PASS=0 FAIL=0
ok() { echo "PASS  $*"; PASS=$((PASS + 1)); }
bad() { echo "FAIL  $*"; FAIL=$((FAIL + 1)); }
expect_log() { local i; for i in $(seq 50); do grep -qF -- "$2" "$LOG" && { ok "$1"; return; }; sleep 0.1; done; bad "$1 (log lacks: $2)"; tail -3 "$LOG"; }

# stub DIR NAME: logs "NAME [arg]... PATH=<its PATH>"; with stdin, also how many lines start with "diff --git"
stub() {
    cat > "$1/$2" <<EOF
#!/bin/bash
line="$2"; for a in "\$@"; do line+=" [\$a]"; done
[[ -t 0 ]] || line+=" diffs=\$(grep -c '^diff --git')"
printf '%s PATH=%s\n' "\$line" "\$PATH" >> "$LOG"
EOF
    chmod +x "$1/$2"
}

# --- search_in_single_file: its keys call the scripts beside it, its tmux windows get its PATH
cp "$REPO_SUB/search_in_single_file" "$SUB/"
for s in platform_action show_diffs git_diff git_diff_short show_logs; do stub "$SUB" "$s"; done
fzf_dir=$(dirname -- "$(command -v fzf)")
tm() { tmux -L sfrs-sub-functions -f /dev/null "$@"; }
tm kill-server 2>/dev/null
tm new-session -d -s sub -x 120 -y 30 "env -i HOME=$FAKE_HOME PATH=$STUBS:$fzf_dir:/usr/local/bin:/usr/bin:/bin \
    TERM=xterm-256color LANG=C.UTF-8 TMUX=\$TMUX bash --norc --noprofile"
pane() { tm capture-pane -p -t sub; }
wait_pane() { local i; for i in $(seq 60); do pane | grep -qE -- "$1" && return 0; sleep 0.1; done; return 1; }
tm send-keys -t sub "'$SUB/search_in_single_file' '$T/src/main.c' needle" Enter
if wait_pane 'needle in main'; then
    ok 'search_in_single_file starts with HOME elsewhere'
    tm send-keys -t sub Enter
    expect_log 'Enter: platform_action beside it' "platform_action [open] [$T/src/main.c]"
    tm send-keys -t sub C-c
    expect_log 'Ctrl-C: platform_action beside it' "platform_action [copy-path] [$T/src/main.c]"
    tm send-keys -t sub C-u
    expect_log 'Ctrl-U: show_diffs beside it, in a window with its PATH' "show_diffs [$T/src/main.c] PATH=$STUBS:"
    tm send-keys -t sub C-g
    expect_log 'Ctrl-G: git_diff beside it, with its PATH' "git_diff [$T/src/main.c] PATH=$STUBS:"
else
    bad "search_in_single_file did not start ($(pane | grep -v '^\s*$' | tail -3 | tr '\n' '/'))"
fi
tm kill-server

# --- show_diffs: git-split-diffs when installed, else delta side by side
cp "$REPO_SUB/show_diffs" "$SUB/show_diffs"
stub "$STUBS" delta
stub "$STUBS" less
run_show_diffs() {
    : > "$LOG"
    (cd "$T" && env -i HOME="$FAKE_HOME" PATH="$1:/usr/bin:/bin" TERM=xterm-256color "$SUB/show_diffs" src/main.c \
        </dev/null >/dev/null 2>"$W/show_diffs.err")
}
run_show_diffs "$STUBS"
if grep -q '^delta \[--side-by-side\].* diffs=2 ' "$LOG" && grep -q '^less ' "$LOG"; then
    ok 'show_diffs without git-split-diffs: delta side by side, both commits'
else
    bad "show_diffs without git-split-diffs (log: $(tr '\n' '/' < "$LOG") err: $(head -c 300 "$W/show_diffs.err"))"
fi
SPLIT=$W/with-split
mkdir -p "$SPLIT"
stub "$SPLIT" git-split-diffs
run_show_diffs "$SPLIT:$STUBS"
if grep -q '^git-split-diffs ' "$LOG" && ! grep -q '^delta ' "$LOG"; then
    ok 'show_diffs with git-split-diffs: uses it'
else
    bad "show_diffs with git-split-diffs (log: $(tr '\n' '/' < "$LOG"))"
fi

echo "pass=$PASS fail=$FAIL"
[[ $FAIL -eq 0 ]]
