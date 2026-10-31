#!/bin/bash
# Speed of sf as a ratio, not a stopwatch (needs tmux): the time until the window draws its first row of a tree
# of 100,000 files, over the time of `sf files --print` on the same tree. Each of 5 release runs measures both
# back to back (same load) and gives its ratio; the median of the 5 ratios fails (exit 1) above 1.5. One
# warm-up run first (page cache, tmux server). Not part of cargo test.
# The window runs in a private tmux server (socket "sf-speed-ratio", never the user's own), 140x40, in a waiting
# bash made before the timer starts: the TUI time runs from Enter on the typed command to the first file name in
# a capture of the pane; it includes that send-keys call and the poll step (one capture-pane every ~5 ms), not
# the pane's creation. The --print time is the wall clock of the whole command. Each run gets a fresh
# XDG_CONFIG_HOME, so no user config changes the list.
#
# usage: bash tests/speed_ratio.sh [TREE]   (from sf-rs/; TREE default target/speed-ratio-tree, made when
#        missing: 100,000 empty files f000000 … f099999 in one folder)
#        SF_BIN=path/to/sf (default target/release/sf); SF_SPEED_LIMIT=1.5 (the ratio's limit)
set -u
die() { echo "speed_ratio.sh: $*" >&2; exit 2; }
crate=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SF=${SF_BIN:-$crate/target/release/sf}
[[ -f $SF && -x $SF ]] || die "no sf at $SF: build it first: cargo build --release"
SF=$(realpath -- "$SF")
limit=${SF_SPEED_LIMIT:-1.5}
[[ $limit =~ ^[0-9]+(\.[0-9]+)?$ ]] || die "SF_SPEED_LIMIT is not a number: $limit"
command -v tmux >/dev/null || die "tmux not found"

TREE=${1:-$crate/target/speed-ratio-tree}
if [[ ! -d $TREE ]]; then
    (( $# == 0 )) || die "no folder $TREE"
    echo "making $TREE: 100,000 empty files"
    # built aside and renamed when complete, so a stopped run never leaves a short tree behind
    mkdir -p "$TREE.part" && (cd "$TREE.part" && seq -f 'f%06.0f' 0 99999 | xargs touch) \
        && mv -T "$TREE.part" "$TREE" || die "could not make $TREE"
fi
TREE=$(realpath -- "$TREE")
mkdir -p "$crate/target/speed-ratio-work" && work=$(mktemp -d "$crate/target/speed-ratio-work/run.XXXXXX") \
    || die "no work folder"

tm() { tmux -L sf-speed-ratio -f /dev/null "$@"; }
tm kill-server 2>/dev/null
trap 'tm kill-server 2>/dev/null' EXIT
# the server stays up between the runs' sessions, so no run pays for its start
tm start-server \; set -s exit-empty off || die "tmux server did not start"

# ms from Enter on `cd "$T" && XDG_CONFIG_HOME=$X "$S"` in a waiting shell to the first file name of the tree on
# the screen; the pane and its shell are made before the timer starts (the paths are in its environment, so the
# typed line holds no file name)
tui_ms() {
    local t0 t1 i
    tm new-session -d -s run -x 140 -y 40 \
        "exec env PS1='ready\$ ' T=${TREE@Q} X=${1@Q} S=${SF@Q} bash --noprofile --norc"
    for (( i = 0; i < 400; i++ )); do
        tm capture-pane -p -t run 2>/dev/null | grep -q '^ready\$' && break
        sleep 0.005
    done
    tm send-keys -t run -l 'cd "$T" && XDG_CONFIG_HOME=$X "$S"'
    t0=${EPOCHREALTIME/./}
    tm send-keys -t run Enter
    for (( i = 0; i < 2000; i++ )); do
        if tm capture-pane -p -t run 2>/dev/null | grep -qE 'f0[0-9]{5}'; then
            t1=${EPOCHREALTIME/./}
            tm kill-session -t run
            echo $(( (t1 - t0) / 1000 ))
            return 0
        fi
        sleep 0.005
    done
    tm kill-session -t run 2>/dev/null
    return 1
}

# ms of `sf files --print` in the tree
print_ms() {
    local t0 t1
    t0=${EPOCHREALTIME/./}
    (cd "$TREE" && XDG_CONFIG_HOME=$1 "$SF" files --print >/dev/null) || return 1
    t1=${EPOCHREALTIME/./}
    echo $(( (t1 - t0) / 1000 ))
}

echo "sf: $SF ($("$SF" --version)); tree: $TREE"
echo "TUI first row: Enter in a waiting shell -> first file name on the screen; --print: the whole command"
tui=() print=() ratios=()
for run in warm-up 1 2 3 4 5; do
    mkdir "$work/$run-tui" "$work/$run-print" || die "no config folder in $work"
    # a pair, back to back: both see the same load
    t=$(tui_ms "$work/$run-tui") || die "run $run: no file name of the tree on the screen"
    p=$(print_ms "$work/$run-print") || die "run $run: sf files --print failed"
    read -r shown r < <(LC_ALL=C awk -v t="$t" -v p="$p" 'BEGIN { printf "%.2f %.6f\n", t / p, t / p }')
    if [[ $run == warm-up ]]; then
        echo "warm-up (not counted): TUI first row $t ms, --print $p ms, ratio $shown"
    else
        echo "run $run: TUI first row $t ms, --print $p ms, ratio $shown"
        tui+=("$t") print+=("$p") ratios+=("$r")
    fi
done
median() { printf '%s\n' "$@" | LC_ALL=C sort -n | sed -n 3p; } # the 3rd of 5
LC_ALL=C awk -v t="$(median "${tui[@]}")" -v p="$(median "${print[@]}")" -v r="$(median "${ratios[@]}")" \
    -v l="$limit" 'BEGIN {
    over = (r > l) # in printf or exit, a bare > would be an output redirect
    printf "median: TUI %d ms, --print %d ms; median of the 5 run ratios %.2f (limit %s) %s\n", t, p, r, l,
        (over ? "FAIL" : "PASS")
    exit over
}'
