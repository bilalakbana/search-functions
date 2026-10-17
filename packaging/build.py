"""Builds sf's release package, sf-linux-x86_64.tar.gz, and its .sha256 into OUTDIR.

From packaging/bundle.toml: downloads the pinned programs and license texts (sha256 checked), builds ugrep
statically in an Alpine container, builds sf for x86_64-unknown-linux-musl, collects every license, checks
that each binary is statically linked, and packs the tree for `tar -xz -C ~/.local`:

    bin/sf
    lib/sf/{fd,bat,delta,ugrep,ug,ugrep+}
    share/doc/sf/{README.md,THIRD-PARTY.md,licenses/}

Needs: python3 (3.11+), docker, cargo and rustc with the x86_64-unknown-linux-musl target, git, file, strip.

usage: python3 -I packaging/build.py OUTDIR     (OUTDIR: a new, empty folder; nothing in it is ever deleted)
"""

import gzip
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TARGET = "x86_64-unknown-linux-musl"
PACKAGE = "sf-linux-x86_64.tar.gz"
# what ugrep is built with in Alpine: the libraries' static archives
ALPINE_PACKAGES = (
    "build-base pcre2-dev pcre2-static zlib-dev zlib-static bzip2-dev bzip2-static xz-dev xz-static "
    "lz4-dev lz4-static zstd-dev zstd-static brotli-dev brotli-static"
)
# runs in the container as root; the source archive is /src.tar.gz, results and logs go to /out, which is
# given to the caller's user ($OWNER) on the way out, failed or not. brotlicommon goes last, after brotlidec
# and brotlienc that need it: in that order the static link succeeds (ugrep's README suggests
# --without-brotli instead).
UGREP_BUILD = f"""
set -eu
trap 'chown -R "$OWNER" /out' EXIT
apk add -q {ALPINE_PACKAGES} >/out/apk.log 2>&1
apk list -I >/out/apk-installed.txt 2>/dev/null
mkdir /build && cd /build && tar -xzf /src.tar.gz && cd ugrep-*
./configure --enable-static --disable-dependency-tracking LIBS=-lbrotlicommon >/out/configure.log 2>&1
make -j"$(nproc)" >/out/make.log 2>&1
strip bin/ugrep bin/ug
cp bin/ugrep bin/ug bin/ugrep+ /out/
"""


class BuildError(Exception):
    """A step failed; the message says which and why."""


def log(message: str) -> None:
    print(f"build: {message}", file=sys.stderr, flush=True)


def run(cmd: list, **kwargs) -> subprocess.CompletedProcess:
    """Runs cmd (a list, no shell); a failure is a BuildError with its stderr's end."""
    result = subprocess.run(cmd, capture_output=True, text=True, **kwargs)
    if result.returncode != 0:
        tail = "\n".join((result.stderr or result.stdout).strip().splitlines()[-15:])
        raise BuildError(f"{' '.join(map(str, cmd[:4]))} … failed (exit {result.returncode}):\n{tail}")
    return result


def url_name(url: str) -> str:
    """The file name of a URL: its last path part, without a query."""
    return url.split("?", 1)[0].rstrip("/").rsplit("/", 1)[-1]


def fetch(url: str, sha256: str, dest_dir: Path) -> Path:
    """Downloads url into dest_dir and checks its sha256."""
    dest_dir.mkdir(parents=True, exist_ok=True)
    dest = dest_dir / url_name(url)
    with urllib.request.urlopen(url, timeout=120) as response:
        data = response.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        raise BuildError(f"{url}: sha256 is {got}, bundle.toml says {sha256}")
    dest.write_bytes(data)
    return dest


def version_errors(installed: str, libraries: list[dict]) -> list[str]:
    """What differs between `apk list -I` and the libraries' versions in bundle.toml."""
    versions = {}
    for line in installed.splitlines():
        m = re.match(r"(?P<name>\S+)-(?P<version>[0-9][^-\s]*)-r\d+\s", line)
        if m:
            versions[m["name"]] = m["version"]
    errors = []
    for lib in libraries:
        have = versions.get(lib["apk"])
        if have is None:
            errors.append(f"{lib['name']}: Alpine package {lib['apk']} is not installed")
        elif have != lib["version"]:
            errors.append(f"{lib['name']}: Alpine has {have}, bundle.toml says {lib['version']}")
    return errors


def is_static(file_description: str) -> bool:
    """Whether `file -b`'s description is that of a statically linked program."""
    return "statically linked" in file_description or "static-pie linked" in file_description


def write_tar(root: Path, out: Path, mtime: int) -> None:
    """Packs root's contents (no top folder) the same way every time: sorted, owner root, the given mtime,
    modes 755 for folders and executables, else 644."""

    def info(path: Path) -> tarfile.TarInfo:
        ti = tarfile.TarInfo(path.relative_to(root).as_posix())
        ti.uid = ti.gid = 0
        ti.uname = ti.gname = ""
        ti.mtime = mtime
        executable = path.is_dir() or os.access(path, os.X_OK)
        ti.mode = 0o755 if executable else 0o644
        if path.is_dir():
            ti.type = tarfile.DIRTYPE
        else:
            ti.size = path.stat().st_size
        return ti

    with open(out, "wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.PAX_FORMAT) as tar:
            for path in sorted(root.rglob("*"), key=lambda p: p.relative_to(root).as_posix()):
                if path.is_symlink():
                    raise BuildError(f"{path}: symbolic links are not packed")
                ti = info(path)
                if path.is_dir():
                    tar.addfile(ti)
                else:
                    with open(path, "rb") as f:
                        tar.addfile(ti, f)


def linked_crates(metadata: dict) -> list[dict]:
    """The packages `cargo metadata` says the root links: reachable through normal dependencies."""
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    seen, stack = set(), [root]
    while stack:
        node = stack.pop()
        if node in seen:
            continue
        seen.add(node)
        for dep in nodes[node]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    seen.discard(root)
    return sorted((packages[i] for i in seen), key=lambda p: (p["name"], p["version"]))


def extract(archive: Path, dest: Path) -> None:
    with tarfile.open(archive) as tar:
        tar.extractall(dest, filter="data")


def build_ugrep(archive: Path, out: Path, image: str, libraries: list[dict]) -> None:
    out.mkdir(parents=True)
    owner = f"{os.getuid()}:{os.getgid()}"
    cmd = ["docker", "run", "--rm", "-e", f"OWNER={owner}", "-v", f"{archive}:/src.tar.gz:ro",
           "-v", f"{out}:/out", image, "sh", "-c", UGREP_BUILD]
    try:
        run(cmd)
    except BuildError as e:
        logs = [p for p in ("make.log", "configure.log", "apk.log") if (out / p).exists()]
        tail = "\n".join((out / logs[0]).read_text().splitlines()[-20:]) if logs else ""
        raise BuildError(f"ugrep's build failed; {out / logs[0] if logs else 'no log'}:\n{tail}") from e
    errors = version_errors((out / "apk-installed.txt").read_text(), libraries)
    if errors:
        raise BuildError("Alpine's libraries are not the ones bundle.toml names (update the versions "
                         "and license URLs there):\n  " + "\n  ".join(errors))


def build_sf() -> tuple[Path, dict]:
    manifest = REPO / "sf-rs/Cargo.toml"
    run(["cargo", "build", "--release", "--locked", "--target", TARGET, "--manifest-path", str(manifest)])
    meta = json.loads(run(["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform",
                           TARGET, "--manifest-path", str(manifest)]).stdout)
    return Path(meta["target_directory"]) / TARGET / "release/sf", meta


def copy(src: Path, dest: Path, mode: int) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dest)
    os.chmod(dest, mode)


def crate_licenses(meta: dict, dest: Path) -> int:
    """Copies the license files of every crate sf links; a crate without one gets a NOTICE.txt naming its
    license and authors. Returns the number of crates."""
    crates = linked_crates(meta)
    for p in crates:
        folder = Path(p["manifest_path"]).parent
        files = sorted(f for f in folder.iterdir() if f.is_file() and f.name.upper().startswith(
            ("LICEN", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT")))
        if p.get("license_file"):
            files.append(folder / p["license_file"])
        target = dest / f"{p['name']}-{p['version']}"
        target.mkdir(parents=True)
        for f in dict.fromkeys(files):
            copy(f, target / f.name, 0o644)
        if not files:
            if not p.get("license"):
                raise BuildError(f"crate {p['name']} {p['version']} has neither a license nor a license file")
            authors = ", ".join(p.get("authors") or []) or "(none given)"
            (target / "NOTICE.txt").write_text(
                f"{p['name']} {p['version']} ships no license file.\nLicense: {p['license']} (texts in "
                f"../../rust/licenses/)\nAuthors: {authors}\nSource: {p.get('repository') or 'crates.io'}\n")
    return len(crates)


def third_party(bundle: dict, crates: int, rustc: str) -> str:
    rows = ["| Component | Version | License | Source |", "|---|---|---|---|"]
    for p in bundle["program"]:
        rows.append(f"| {p['name']} | {p['version']} | {p['license']} | {p['source']} |")
    for lib in bundle["library"]:
        rows.append(f"| {lib['name']} (in ugrep) | {lib['version']} | {lib['license']} | {lib['source']} |")
    return (
        "# Third-party software in this package\n\n"
        "Each component's license texts are in `licenses/<component>/`.\n\n" + "\n".join(rows) + "\n\n"
        f"sf itself links {crates} Rust crates (`licenses/rust-crates/`), Rust's standard library "
        f"({rustc}; `licenses/rust/`) and musl (`licenses/musl/`). bat's own list of the syntaxes and "
        "themes it carries is `licenses/bat/acknowledgements.txt`.\n"
    )


def readme(version: str, commit: str) -> str:
    return f"""# sf {version}

The search functions in one terminal window: Files, Content, Deep, Folders, Git and Help, with previews. Built from
https://github.com/bilalakbana/search-functions at commit {commit} (the source of sf, GPL-3.0: see
`licenses/sf/LICENSE`).

- `bin/sf` is the program; `sf doctor` shows what it finds and how open, reveal and copy work here.
- `lib/sf/` holds the engines it runs (fd, bat, ugrep, ug, ugrep+, delta). sf uses these before any in
  `$PATH`; they are not put in `$PATH` otherwise.
- Not included, installed from the system when wanted: git (Git tab, diffs), tmux (actions in windows of
  their own), nvim (Ctrl-O), less; for PDF and Office files in Content: poppler-utils, antiword, pandoc.
- Third-party software and licenses: `THIRD-PARTY.md`, `licenses/`.
"""


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    out = Path(argv[1]).resolve()
    if out.exists() and any(out.iterdir()):
        raise BuildError(f"{out} is not empty")
    bundle = tomllib.loads((REPO / "packaging/bundle.toml").read_text())
    downloads, unpacked, root = out / "downloads", out / "unpacked", out / "root"
    lib, doc = root / "lib/sf", root / "share/doc/sf"
    licenses = doc / "licenses"

    for p in bundle["program"]:
        log(f"{p['name']} {p['version']}")
        archive = fetch(p["url"], p["sha256"], downloads / p["name"])
        src = unpacked / p["name"]
        extract(archive, src)
        if p.get("build") == "ugrep":
            built = out / "ugrep-build"
            build_ugrep(archive, built, bundle["alpine"], bundle["library"])
            for name in ("ugrep", "ug", "ugrep+"):
                copy(built / name, lib / name, 0o755)
        for f in p["files"]:
            copy(src / f, lib / Path(f).name, 0o755)
        for f in p["licenses"]:
            copy(src / f, licenses / p["name"] / Path(f).name, 0o644)
        for lic in p["license_urls"]:
            copy(fetch(lic["url"], lic["sha256"], downloads / f"{p['name']}-license"),
                 licenses / p["name"] / url_name(lic["url"]), 0o644)
    for library in bundle["library"]:
        for lic in library["license_urls"]:
            copy(fetch(lic["url"], lic["sha256"], downloads / f"{library['name']}-license"),
                 licenses / library["name"] / url_name(lic["url"]), 0o644)

    log("sf")
    sf, meta = build_sf()
    copy(sf, root / "bin/sf", 0o755)
    run(["strip", str(root / "bin/sf")])

    log("licenses")
    copy(REPO / "LICENSE", licenses / "sf/LICENSE", 0o644)
    sysroot = Path(run(["rustc", "--print", "sysroot"]).stdout.strip())
    rust_doc = sysroot / "share/doc/rust"
    copy(rust_doc / "COPYRIGHT-library.html", licenses / "rust/COPYRIGHT-library.html", 0o644)
    for f in sorted((rust_doc / "licenses").iterdir()):
        copy(f, licenses / "rust/licenses" / f.name, 0o644)
    crates = crate_licenses(meta, licenses / "rust-crates")
    acknowledgements = run([str(lib / "bat"), "--acknowledgements"]).stdout
    (licenses / "bat/acknowledgements.txt").write_text(acknowledgements)
    rustc = run(["rustc", "--version"]).stdout.strip()
    (doc / "THIRD-PARTY.md").write_text(third_party(bundle, crates, rustc))
    version = next(p["version"] for p in meta["packages"] if p["name"] == "sf")
    commit = run(["git", "-C", str(REPO), "rev-parse", "HEAD"]).stdout.strip()
    dirty = run(["git", "-C", str(REPO), "status", "--porcelain"]).stdout.strip()
    (doc / "README.md").write_text(readme(version, commit + (" (with uncommitted changes)" if dirty else "")))

    log("checks")
    for path in sorted([root / "bin/sf", *lib.iterdir()]):
        if path.is_file() and path.read_bytes()[:4] == b"\x7fELF":
            desc = run(["file", "-b", str(path)]).stdout.strip()
            if not is_static(desc):
                raise BuildError(f"{path.relative_to(root)} is not statically linked: {desc}")
    mtime = int(os.environ.get("SOURCE_DATE_EPOCH")
                or run(["git", "-C", str(REPO), "log", "-1", "--format=%ct"]).stdout.strip())
    package = out / PACKAGE
    write_tar(root, package, mtime)
    digest = hashlib.sha256(package.read_bytes()).hexdigest()
    (out / f"{PACKAGE}.sha256").write_text(f"{digest}  {PACKAGE}\n")
    log(f"{package} ({package.stat().st_size // 1024} KiB) sha256 {digest}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv))
    except BuildError as e:
        log(f"FAILED: {e}")
        sys.exit(1)
