"""Prints one version's section of CHANGELOG.md, the body of that version's GitHub release.

usage: python3 -I packaging/release_notes.py VERSION [CHANGELOG]   (VERSION: 0.13.0 or v0.13.0)

The section is the lines after `## [VERSION] - DATE` (or `## VERSION`) up to the next `## ` heading, without
blank lines around them. A missing or empty section exits 1, so the release job never publishes without notes.
"""

import re
import sys


class NotesError(Exception):
    pass


def section(text, version, name="CHANGELOG.md"):
    version = version.removeprefix("v")
    v = re.escape(version)
    heading = re.compile(rf"## (\[{v}\]|{v})(\s|$)")
    lines = text.splitlines()
    start = next((i for i, line in enumerate(lines) if heading.match(line)), None)
    if start is None:
        raise NotesError(f"{name} has no '## [{version}]' section; add it before tagging")
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")), len(lines))
    body = "\n".join(lines[start + 1 : end]).strip("\n")
    if not body.strip():
        raise NotesError(f"{name}: the '## [{version}]' section is empty; write it before tagging")
    return body


def main(argv, out=sys.stdout, err=sys.stderr):
    if len(argv) not in (1, 2):
        print("usage: python3 -I packaging/release_notes.py VERSION [CHANGELOG]", file=err)
        return 2
    path = argv[1] if len(argv) == 2 else "CHANGELOG.md"
    try:
        with open(path, encoding="utf-8") as f:
            print(section(f.read(), argv[0], path), file=out)
    except (OSError, NotesError) as e:
        print(f"release_notes.py: {e}", file=err)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
