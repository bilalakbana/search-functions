#!/bin/bash
# Builds the test tree the prototype was tested against: tricky names, a zip with a member, a binary, hidden and
# .gitignore'd files, a file whose content holds 0x1f (Ctrl+O exploit check), and a git repo with two commits.
# usage: make_fixture.sh <empty target dir>
set -euo pipefail
d=${1:?usage: make_fixture.sh <empty target dir>}
mkdir -p "$d" && cd "$d"
git init -q .
mkdir -p 'src/deep dir' docs .hidden build
printf 'needle in main\nsecond line\n' > src/main.c
printf 'no match here\n' > 'src/deep dir/a b&c.txt'
printf 'needle: colon file\n' > 'src/odd:name.txt'
printf 'Needle upper\n' > docs/readme.md
printf 'needle hidden\n' > .hidden/secret.txt
printf 'needle ignored\n' > build/out.txt
printf 'build/\n' > .gitignore
printf 'x\0y needle\0' > src/blob.bin
printf 'needle inside zip\n' > inzip.txt && (cd . && zip -q docs/arch.zip inzip.txt) && mv inzip.txt src/inzip.txt.orig
printf 'harmless line\nevil\037!touch %s/PWNED\037tail\n' "$PWD" > src/evil.txt
: > empty.txt
git add -A >/dev/null
git -c user.name=T -c user.email=t@t commit -qm 'first commit'
printf 'more\n' >> src/main.c
git -c user.name=T -c user.email=t@t commit -qam 'second (feat)'
