#!/bin/bash
# Test of a release package in clean containers, by default Ubuntu 22.04, Debian 12 and Ubuntu 24.04. Only
# what the tests themselves need comes from apt (git, tmux, zip, python3, file, less), none of sf's engines.
# The package is unpacked into ~/.local as the README says; then `sf doctor` must name the bundled engines,
# and sf-rs/tests/e2e.sh must pass against the installed sf. Logs and screens of each image go to
# WORKDIR/<image>/.
#
# usage: packaging/test_package.sh PACKAGE WORKDIR [IMAGE...]
#        (WORKDIR: a new, empty folder; nothing in it is ever deleted)
set -u
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PKG=$(realpath -- "${1:?usage: test_package.sh PACKAGE WORKDIR [IMAGE...]}") || exit 2
W=${2:?usage: test_package.sh PACKAGE WORKDIR [IMAGE...]}
mkdir -p "$W" && W=$(realpath -- "$W")
[[ -z $(ls -A "$W") ]] || { echo "test_package.sh: $W is not empty" >&2; exit 2; }
images=("${@:3}")
(( ${#images[@]} )) || images=(ubuntu:22.04 debian:12 ubuntu:24.04)

# runs in the container as root; /work is given back to the caller's user ($OWNER) on the way out
inside='
set -u
trap '\''chown -R "$OWNER" /work'\'' EXIT
export DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 LC_ALL=C.UTF-8
apt-get update -qq >/dev/null && apt-get install -y -qq git tmux zip python3 file less >/dev/null \
    || { echo "SETUP apt-get failed"; exit 3; }
for e in fd fdfind bat batcat ugrep ug delta; do
    command -v "$e" >/dev/null && { echo "SETUP the image already has $e"; exit 3; }
done
mkdir -p ~/.local && tar -xzf /pkg.tar.gz -C ~/.local || { echo "SETUP unpacking failed"; exit 3; }
fail=0
~/.local/bin/sf doctor > /work/doctor.txt; rc=$?
cat /work/doctor.txt
[[ $rc == 0 ]] || { echo "FAIL  sf doctor exited $rc"; fail=1; }
for e in fd bat ugrep ugrep+ delta; do
    if grep -qE "^  $e +$HOME/.local/lib/sf/$e " /work/doctor.txt; then echo "PASS  doctor: $e from the package"
    else echo "FAIL  doctor: $e is not the packaged one"; fail=1; fi
done
export PATH=$HOME/.local/lib/sf:$PATH   # for the tests'"'"' own fd / ugrep calls
SF_BIN=$HOME/.local/bin/sf /tests/e2e.sh /work/e2e || fail=1
exit $fail
'

failed=()
for img in "${images[@]}"; do
    out=$W/${img//[:\/]/-}
    mkdir -p "$out"
    echo "== $img"
    docker run --rm -e OWNER="$(id -u):$(id -g)" -v "$PKG:/pkg.tar.gz:ro" -v "$here/../sf-rs/tests:/tests:ro" \
        -v "$out:/work" "$img" bash -c "$inside" > "$out/log.txt" 2>&1
    rc=$?
    grep -E '^(FAIL|SETUP)|^pass=' "$out/log.txt"
    (( rc == 0 )) || failed+=("$img (exit $rc, log $out/log.txt)")
done
if (( ${#failed[@]} )); then
    printf 'test_package.sh: failed on %s\n' "${failed[@]}" >&2
    exit 1
fi
echo "test_package.sh: all passed on ${images[*]}"
