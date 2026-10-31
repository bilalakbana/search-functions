"""Tests of packaging/release_notes.py; run: python3 -I packaging/test_release_notes.py"""

import io
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True  # no __pycache__ in the repo
sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_notes  # noqa: E402

CHANGELOG = """# Changelog

Intro.

## [0.13.0] - 2026-10-31

### Added
- `sf --version`.

### Fixed
- A fix.

## 0.12.1

- Older.

## [0.12.0] - 2026-10-01

## Earlier releases
"""

SECTION = "### Added\n- `sf --version`.\n\n### Fixed\n- A fix."


class Section(unittest.TestCase):
    def test_the_section_without_its_heading_and_the_next_one(self):
        self.assertEqual(release_notes.section(CHANGELOG, "0.13.0"), SECTION)

    def test_a_heading_with_or_without_brackets_and_date(self):
        self.assertEqual(release_notes.section(CHANGELOG, "0.12.1"), "- Older.")

    def test_a_leading_v_is_dropped(self):
        self.assertEqual(release_notes.section(CHANGELOG, "v0.13.0"), SECTION)

    def test_a_version_is_not_a_prefix_of_another(self):
        with self.assertRaises(release_notes.NotesError):
            release_notes.section(CHANGELOG, "0.13")

    def test_a_missing_section_is_named(self):
        with self.assertRaisesRegex(
            release_notes.NotesError, r"has no '## \[0\.14\.0\]' section; add it before tagging"
        ):
            release_notes.section(CHANGELOG, "0.14.0")

    def test_an_empty_section_is_an_error(self):
        with self.assertRaisesRegex(release_notes.NotesError, r"'## \[0\.12\.0\]' section is empty"):
            release_notes.section(CHANGELOG, "0.12.0")


class Main(unittest.TestCase):
    def run_main(self, *args):
        out, err = io.StringIO(), io.StringIO()
        code = release_notes.main(list(args), out, err)
        return code, out.getvalue(), err.getvalue()

    def test_prints_the_section_and_exits_0(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d, "CHANGELOG.md")
            path.write_text(CHANGELOG)
            self.assertEqual(self.run_main("0.13.0", str(path)), (0, SECTION + "\n", ""))

    def test_a_missing_version_exits_1_with_the_message(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d, "CHANGELOG.md")
            path.write_text(CHANGELOG)
            code, out, err = self.run_main("0.14.0", str(path))
            self.assertEqual((code, out), (1, ""))
            self.assertEqual(
                err,
                f"release_notes.py: {path} has no '## [0.14.0]' section; add it before tagging\n",
            )

    def test_an_empty_section_exits_1(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d, "CHANGELOG.md")
            path.write_text(CHANGELOG)
            code, out, err = self.run_main("0.12.0", str(path))
            self.assertEqual((code, out), (1, ""))
            self.assertIn("is empty", err)


if __name__ == "__main__":
    unittest.main()
