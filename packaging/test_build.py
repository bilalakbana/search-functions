"""Tests of packaging/build.py's checks; run: python3 -I packaging/test_build.py"""

import hashlib
import io
import os
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True  # no __pycache__ in the repo
sys.path.insert(0, str(Path(__file__).resolve().parent))
import build  # noqa: E402


class Fetch(unittest.TestCase):
    def test_a_wrong_sha256_stops_the_build(self):
        with tempfile.TemporaryDirectory() as d:
            src = Path(d, "x.tar.gz")
            src.write_bytes(b"payload")
            good = hashlib.sha256(b"payload").hexdigest()
            got = build.fetch(src.as_uri(), good, Path(d, "out"))
            self.assertEqual(got.read_bytes(), b"payload")
            with self.assertRaisesRegex(build.BuildError, "sha256"):
                build.fetch(src.as_uri(), "0" * 64, Path(d, "out2"))

    def test_the_file_name_drops_the_query(self):
        self.assertEqual(
            build.url_name("https://git.musl-libc.org/cgit/musl/plain/COPYRIGHT?h=v1.2.6"), "COPYRIGHT"
        )


class AlpineVersions(unittest.TestCase):
    LIBS = [
        {"name": "zlib", "apk": "zlib", "version": "1.3.2"},
        {"name": "bzip2", "apk": "libbz2", "version": "1.0.8"},
        {"name": "gcc-runtime", "apk": "libstdc++", "version": "15.2.0"},
    ]

    def test_same_upstream_versions_pass(self):
        installed = (
            "zlib-1.3.2-r1 x86_64 {zlib} (Zlib) [installed]\n"
            "libbz2-1.0.8-r6 x86_64 {bzip2} (bzip2-1.0.6) [installed]\n"
            "libstdc++-15.2.0-r5 x86_64 {gcc} (GPL-3.0-or-later AND LGPL-2.1-or-later) [installed]\n"
        )
        self.assertEqual(build.version_errors(installed, self.LIBS), [])

    def test_another_version_or_a_missing_package_is_named(self):
        installed = "zlib-1.3.3-r0 x86_64 {zlib} (Zlib) [installed]\n"
        errors = build.version_errors(installed, self.LIBS)
        self.assertEqual(len(errors), 3, errors)
        self.assertIn("zlib: Alpine has 1.3.3, bundle.toml says 1.3.2", errors)
        self.assertTrue(any("libbz2" in e and "not installed" in e for e in errors), errors)


class Static(unittest.TestCase):
    def test_only_static_binaries_pass(self):
        # `file -b` as it describes the programs of the package and a usual dynamic one
        for desc, static in [
            ("ELF 64-bit LSB executable, x86-64, version 1 (SYSV), statically linked, Go BuildID=x", True),
            ("ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, stripped", True),
            (
                "ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), dynamically linked, "
                "interpreter /lib64/ld-linux-x86-64.so.2, stripped",
                False,
            ),
        ]:
            self.assertEqual(build.is_static(desc), static, desc)


class Tarball(unittest.TestCase):
    def test_same_tree_same_bytes_with_modes_and_no_owner(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d, "root")
            (root / "bin").mkdir(parents=True)
            (root / "bin/sf").write_text("#!/bin/sh\n")
            os.chmod(root / "bin/sf", 0o755)
            (root / "share/doc").mkdir(parents=True)
            (root / "share/doc/a.txt").write_text("a\n")
            one, two = Path(d, "1.tar.gz"), Path(d, "2.tar.gz")
            build.write_tar(root, one, mtime=1_700_000_000)
            os.utime(root / "share/doc/a.txt", (1, 1))  # a later build on other file times
            build.write_tar(root, two, mtime=1_700_000_000)
            self.assertEqual(one.read_bytes(), two.read_bytes())
            with tarfile.open(one) as t:
                members = {m.name: m for m in t.getmembers()}
            self.assertEqual(sorted(members), ["bin", "bin/sf", "share", "share/doc", "share/doc/a.txt"])
            self.assertEqual(members["bin/sf"].mode, 0o755)
            self.assertEqual(members["share/doc/a.txt"].mode, 0o644)
            self.assertTrue(all(m.uid == 0 and m.uname == "" for m in members.values()))


class Crates(unittest.TestCase):
    def test_reachable_normal_dependencies_only(self):
        meta = {
            "packages": [
                {"id": "sf", "name": "sf", "version": "0.1.0", "manifest_path": "/s/Cargo.toml"},
                {"id": "a", "name": "a", "version": "1.0.0", "manifest_path": "/a/Cargo.toml"},
                {"id": "b", "name": "b", "version": "2.0.0", "manifest_path": "/b/Cargo.toml"},
                {"id": "t", "name": "t", "version": "3.0.0", "manifest_path": "/t/Cargo.toml"},
            ],
            "resolve": {
                "root": "sf",
                "nodes": [
                    {"id": "sf", "deps": [
                        {"pkg": "a", "dep_kinds": [{"kind": None}]},
                        {"pkg": "t", "dep_kinds": [{"kind": "dev"}]},
                    ]},
                    {"id": "a", "deps": [{"pkg": "b", "dep_kinds": [{"kind": None}]}]},
                    {"id": "b", "deps": []},
                    {"id": "t", "deps": []},
                ],
            },
        }
        names = [p["name"] for p in build.linked_crates(meta)]
        self.assertEqual(names, ["a", "b"])


if __name__ == "__main__":
    unittest.main()
