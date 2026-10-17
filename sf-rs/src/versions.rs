//! Git, Ctrl-Alt-F: the files of every version of the repository (or of the versions chosen) searched.
//! `sf grep-versions` does it as the window's list: `git grep` over the versions, then with Documents
//! every document's and archive's content (each blob once) through ugrep, read from git, never copied
//! to disk. Every row goes out as `git grep -z` writes one: `REV:PATH\0LINE\0TEXT\n`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use crate::model::VersionsOpts;

/// Versions given to one `git grep` (a command line holds only so many).
const REVS_PER_GREP: usize = 200;

/// What to search for and where: no versions given means every one (`git rev-list --all`).
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub pattern: String,
    pub opts: VersionsOpts,
    pub revs: Vec<String>,
}

/// A version to choose in the popup, newest first: what Bilal asked to see of each.
#[derive(Clone, Debug, PartialEq)]
pub struct VersionRow {
    pub hash: String,
    pub date: String,
    pub author: String,
    /// The branch (or tag) it was reached from (`git log --source`), without `refs/…/`.
    pub branch: String,
    pub subject: String,
}

/// `git log`'s arguments for the versions to choose from: every branch, the one each came from.
pub const LOG_ARGS: [&str; 5] = [
    "log",
    "--all",
    "--source",
    "--date=short",
    "--format=%H%x1f%ad%x1f%an%x1f%S%x1f%s",
];

/// `git log LOG_ARGS`' output read; a line not in its form is left out.
pub fn parse_log(out: &[u8]) -> Vec<VersionRow> {
    let text = String::from_utf8_lossy(out);
    text.lines()
        .filter_map(|line| {
            let mut f = line.splitn(5, '\x1f');
            let (hash, date, author, source, subject) =
                (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
            let branch = ["refs/heads/", "refs/remotes/", "refs/tags/", "refs/"]
                .iter()
                .find_map(|p| source.strip_prefix(p))
                .unwrap_or(source);
            let shown = |s: &str| crate::items::printable(s);
            Some(VersionRow {
                hash: hash.to_string(),
                date: shown(date),
                author: shown(author),
                branch: shown(branch),
                subject: shown(subject),
            })
        })
        .collect()
}

/// A document's or archive's kind, by its name's extension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Doc {
    /// zip, tar, gz… : ugrep -z, rows name the member.
    Archive,
    /// PDF, Office, e-books: through a filter to text.
    Filtered,
}

/// `sf grep-versions …` as the window runs it (after `sf`).
pub fn args(q: &Query) -> Vec<OsString> {
    let o = &q.opts;
    let mut a: Vec<OsString> = vec!["grep-versions".into()];
    let flags = [
        (o.whole_word, "--word"),
        (!o.regex, "--fixed"),
        (o.binary, "--binary"),
        (o.documents, "--documents"),
        (o.smart_case, "--smart"),
        (o.match_case, "--case"),
    ];
    a.extend(
        flags
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, f)| OsString::from(f)),
    );
    a.extend(["-e".into(), q.pattern.clone().into(), "--".into()]);
    a.extend(q.revs.iter().map(OsString::from));
    a
}

/// The arguments after `sf grep-versions`.
pub fn parse_args(rest: &[OsString]) -> Result<Query, String> {
    let text = |a: &OsString| {
        a.to_str()
            .map(str::to_string)
            .ok_or("arguments must be text")
    };
    let mut opts = VersionsOpts::default();
    let mut pattern = None;
    let mut revs = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match text(&rest[i])?.as_str() {
            "--word" => opts.whole_word = true,
            "--fixed" => opts.regex = false,
            "--binary" => opts.binary = true,
            "--documents" => opts.documents = true,
            "--smart" => opts.smart_case = true,
            "--case" => opts.match_case = true,
            "-e" => {
                i += 1;
                pattern = Some(text(rest.get(i).ok_or("-e needs a pattern")?)?);
            }
            "--" => {
                revs = rest[i + 1..].iter().map(text).collect::<Result<_, _>>()?;
                break;
            }
            other => return Err(format!("sf grep-versions: unknown option {other}")),
        }
        i += 1;
    }
    let pattern = pattern.ok_or("sf grep-versions: -e PATTERN is needed")?;
    Ok(Query {
        pattern,
        opts,
        revs,
    })
}

/// Case aside unless the case counts or (smart case, as ugrep's) the pattern has an upper-case letter.
fn ignore_case(q: &Query) -> bool {
    let o = &q.opts;
    !(o.match_case || (o.smart_case && q.pattern.chars().any(char::is_uppercase)))
}

/// `git grep`'s arguments for `revs`.
fn git_grep_args(q: &Query, revs: &[String]) -> Vec<String> {
    let o = &q.opts;
    let mut a: Vec<String> = ["grep", "-n", "-z"].map(String::from).to_vec();
    if ignore_case(q) {
        a.push("-i".into());
    }
    if o.whole_word {
        a.push("-w".into());
    }
    a.push(if o.regex { "-P" } else { "-F" }.into());
    a.push(if o.binary { "-a" } else { "-I" }.into());
    a.extend(["-e".to_string(), q.pattern.clone()]);
    a.extend(revs.iter().cloned());
    a.push("--".into());
    a
}

pub fn doc_kind(path: &str) -> Option<Doc> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.')?.1.to_lowercase();
    match ext.as_str() {
        "pdf" | "doc" | "docx" | "odt" | "epub" | "rtf" => Some(Doc::Filtered),
        "zip" | "jar" | "war" | "apk" | "tar" | "tgz" | "gz" | "bz2" | "xz" | "zst" | "lz4"
        | "lzma" | "7z" | "cpio" | "pax" => Some(Doc::Archive),
        _ => None,
    }
}

/// The documents and archives of `git ls-tree -r -z`'s output: (path, blob).
fn docs_in_tree(ls_tree: &[u8]) -> Vec<(String, String)> {
    ls_tree
        .split(|&b| b == 0)
        .filter_map(|record| {
            let record = std::str::from_utf8(record).ok()?;
            let (meta, path) = record.split_once('\t')?;
            let mut meta = meta.split(' ');
            let (_mode, kind, blob) = (meta.next()?, meta.next()?, meta.next()?);
            (kind == "blob" && doc_kind(path).is_some())
                .then(|| (path.to_string(), blob.to_string()))
        })
        .collect()
}

/// ugrep's line for a version's document (`LINE\x1fTEXT`, an archive's `{MEMBER}\x1fLINE\x1fTEXT`) as
/// a row of `rev`'s `path`.
fn doc_row(rev: &str, path: &str, line: &[u8]) -> Option<Vec<u8>> {
    let (member, rest) = match line.strip_prefix(b"{") {
        Some(_) => {
            let end = line.windows(2).position(|w| w == b"}\x1f")?;
            (&line[..=end], &line[end + 2..])
        }
        None => (&b""[..], line),
    };
    let sep = rest.iter().position(|&b| b == 0x1f)?;
    let (number, text) = (&rest[..sep], &rest[sep + 1..]);
    if number.is_empty() || !number.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut row = format!("{rev}:{path}").into_bytes();
    row.extend_from_slice(member);
    row.push(0);
    row.extend_from_slice(number);
    row.push(0);
    row.extend_from_slice(text);
    row.push(b'\n');
    Some(row)
}

/// ugrep's arguments for one version's document `path` read from its stdin.
fn ugrep_args(q: &Query, path: &str, doc: Doc, filters: &str) -> Vec<String> {
    let o = &q.opts;
    let mut a: Vec<String> = vec![
        "-n".into(),
        "--separator=\x1f".into(),
        format!("--label={path}"),
    ];
    match doc {
        Doc::Archive => a.push("-z".into()),
        Doc::Filtered => {
            a.push("-h".into());
            if !filters.is_empty() {
                a.push(format!("--filter={filters}"));
            }
        }
    }
    if ignore_case(q) {
        a.push("-i".into());
    }
    if o.whole_word {
        a.push("-w".into());
    }
    a.push(if o.regex { "-P" } else { "-F" }.into());
    a.extend(["-e".to_string(), q.pattern.clone()]);
    a
}

/// ugrep's `--filter` for documents read from stdin, for the converters found in `path_var`: each is
/// told its input's format (pandoc cannot tell it from stdin, so ugrep+'s own filter does not do).
pub fn filters(path_var: &std::ffi::OsStr) -> String {
    let has = |name: &str| crate::engines::find(path_var, &[name]).is_some();
    let mut f = Vec::new();
    if has("pdftotext") {
        f.push("pdf:pdftotext -q -layout - -".to_string());
    }
    if has("antiword") {
        f.push("doc:antiword -".to_string());
    }
    if has("pandoc") {
        for ext in ["docx", "odt", "epub", "rtf"] {
            f.push(format!(
                "{ext}:pandoc -f {ext} -t plain --wrap=preserve -o -"
            ));
        }
    }
    f.join(",")
}

/// `git ARGS` in `root`: its output, or what went wrong.
fn git_output(git: &Path, root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new(git)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", git.display()))?;
    if !out.status.success() {
        let message = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("git {}: {message}", args.join(" ")));
    }
    Ok(out.stdout)
}

/// Reads `r` to the end in a thread of its own (a child's stderr, while its stdout is read).
fn collect(mut r: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut s = Vec::new();
        let _ = r.read_to_end(&mut s);
        String::from_utf8_lossy(&s).trim().to_string()
    })
}

/// Searches in the repository at `root`; rows to `out`, messages to `err`. Exit status as grep's: 0
/// something found, 1 nothing, 2 an error.
pub fn search(
    q: &Query,
    git: &Path,
    ugrep: Option<&Path>,
    filters: &str,
    root: &Path,
    out: &mut impl Write,
    err: &mut impl Write,
) -> u8 {
    let revs: Vec<String> = if q.revs.is_empty() {
        match git_output(git, root, &["rev-list", "--all"]) {
            Ok(list) => String::from_utf8_lossy(&list)
                .lines()
                .map(str::to_string)
                .collect(),
            Err(e) => {
                let _ = writeln!(err, "{e}");
                return 2;
            }
        }
    } else {
        q.revs.clone()
    };
    let (mut found, mut failed) = (false, false);
    for chunk in revs.chunks(REVS_PER_GREP) {
        match grep_chunk(q, git, root, chunk, out, err) {
            Ok(some) => found |= some,
            Err(e) => {
                failed = true;
                let _ = writeln!(err, "{e}");
            }
        }
    }
    if q.opts.documents {
        match ugrep {
            Some(ugrep) => match documents(q, git, ugrep, filters, root, &revs, out, err) {
                Ok(some) => found |= some,
                Err(e) => {
                    failed = true;
                    let _ = writeln!(err, "{e}");
                }
            },
            None => {
                failed = true;
                let _ = writeln!(err, "ugrep is not installed (needed for Documents)");
            }
        }
    }
    match (found, failed) {
        (true, _) => 0,
        (false, true) => 2,
        (false, false) => 1,
    }
}

/// One `git grep` over `revs`, its rows copied to `out` as they come; whether there were any.
fn grep_chunk(
    q: &Query,
    git: &Path,
    root: &Path,
    revs: &[String],
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<bool, String> {
    let mut child = Command::new(git)
        .args(git_grep_args(q, revs))
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", git.display()))?;
    let messages = collect(child.stderr.take().expect("stderr is piped"));
    let copied = io::copy(&mut child.stdout.take().expect("stdout is piped"), out)
        .map_err(|e| format!("git grep: {e}"))?;
    let status = child.wait().map_err(|e| format!("git grep: {e}"))?;
    let messages = messages.join().unwrap_or_default();
    if !messages.is_empty() {
        let _ = writeln!(err, "{messages}");
    }
    match status.code() {
        Some(0 | 1) => Ok(copied > 0),
        _ => Err(format!("git grep failed ({status})")),
    }
}

/// Every document and archive of `revs` through ugrep, each blob once, its rows given to every version
/// (and path) that has it; whether there were any.
#[allow(clippy::too_many_arguments)]
fn documents(
    q: &Query,
    git: &Path,
    ugrep: &Path,
    filters: &str,
    root: &Path,
    revs: &[String],
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<bool, String> {
    let mut order: Vec<String> = Vec::new();
    let mut holders: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for rev in revs {
        for (path, blob) in docs_in_tree(&git_output(git, root, &["ls-tree", "-r", "-z", rev])?) {
            let list = holders.entry(blob.clone()).or_default();
            if list.is_empty() {
                order.push(blob);
            }
            list.push((rev.clone(), path));
        }
    }
    let mut found = false;
    for blob in &order {
        let held = &holders[blob];
        let (_, path) = &held[0];
        let doc = doc_kind(path).expect("only documents are held");
        let mut cat = Command::new(git)
            .args(["cat-file", "blob", blob])
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", git.display()))?;
        let mut ug = Command::new(ugrep)
            .args(ugrep_args(q, path, doc, filters))
            .current_dir(root)
            .stdin(cat.stdout.take().expect("stdout is piped"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", ugrep.display()))?;
        let messages = collect(ug.stderr.take().expect("stderr is piped"));
        let reader = BufReader::new(ug.stdout.take().expect("stdout is piped"));
        for line in reader.split(b'\n') {
            let line = line.map_err(|e| format!("ugrep: {e}"))?;
            for (rev, path) in held {
                if let Some(row) = doc_row(rev, path, &line) {
                    out.write_all(&row).map_err(|e| format!("writing: {e}"))?;
                    found = true;
                }
            }
        }
        let _ = ug.wait();
        let _ = cat.wait();
        let messages = messages.join().unwrap_or_default();
        if !messages.is_empty() {
            let _ = writeln!(err, "{path}: {messages}");
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{calls, stub, temp_dir};
    use std::path::PathBuf;
    use std::process::Command;

    fn query(pattern: &str) -> Query {
        Query {
            pattern: pattern.into(),
            opts: VersionsOpts::default(),
            revs: Vec::new(),
        }
    }

    #[test]
    fn the_versions_to_choose_from_come_from_git_log() {
        assert_eq!(
            LOG_ARGS,
            [
                "log",
                "--all",
                "--source",
                "--date=short",
                "--format=%H%x1f%ad%x1f%an%x1f%S%x1f%s"
            ]
        );
        let out = "aaa\x1f2026-10-17\x1fBilal Akbana\x1frefs/heads/main\x1fsf 0.9.1\n\
                   bbb\x1f2026-10-16\x1fT\x1frefs/remotes/origin/dev\x1fwip: a\x1fb\n\
                   broken line\n";
        assert_eq!(
            parse_log(out.as_bytes()),
            [
                VersionRow {
                    hash: "aaa".into(),
                    date: "2026-10-17".into(),
                    author: "Bilal Akbana".into(),
                    branch: "main".into(),
                    subject: "sf 0.9.1".into(),
                },
                VersionRow {
                    hash: "bbb".into(),
                    date: "2026-10-16".into(),
                    author: "T".into(),
                    branch: "origin/dev".into(),
                    subject: "wip: a\u{fffd}b".into(),
                },
            ],
            "the branch without refs/…; a control character in a subject is not drawn"
        );
    }

    #[test]
    fn the_arguments_go_and_come_back() {
        let mut q = query("-dash (x)");
        q.opts.whole_word = true;
        q.opts.regex = false;
        q.opts.documents = true;
        q.opts.smart_case = true;
        q.revs = vec!["abc".into(), "def".into()];
        let a = args(&q);
        assert_eq!(a[0], "grep-versions");
        assert_eq!(parse_args(&a[1..]), Ok(q.clone()));
        assert_eq!(parse_args(&args(&query("x"))[1..]), Ok(query("x")));
        assert!(parse_args(&["--bogus".into()]).is_err());
        assert!(parse_args(&[]).is_err(), "a pattern is needed");
    }

    #[test]
    fn git_grep_reads_the_options() {
        let revs = vec!["r1".to_string()];
        let a = git_grep_args(&query("needle"), &revs);
        assert_eq!(
            a,
            [
                "grep", "-n", "-z", "-i", "-P", "-I", "-e", "needle", "r1", "--"
            ],
            "case aside; Perl's syntax; binaries left out"
        );
        let a = git_grep_args(&query("Needle"), &revs);
        assert!(
            a.contains(&"-i".to_string()),
            "case aside, an upper-case letter too"
        );
        let mut smart = query("Needle");
        smart.opts.smart_case = true;
        assert!(
            !git_grep_args(&smart, &revs).contains(&"-i".to_string()),
            "smart case"
        );
        smart.pattern = "needle".into();
        assert!(git_grep_args(&smart, &revs).contains(&"-i".to_string()));
        let mut q = query("a.b");
        q.opts.regex = false;
        q.opts.whole_word = true;
        q.opts.binary = true;
        q.opts.match_case = true;
        let a = git_grep_args(&q, &revs);
        assert_eq!(
            a,
            [
                "grep", "-n", "-z", "-w", "-F", "-a", "-e", "a.b", "r1", "--"
            ]
        );
    }

    #[test]
    fn documents_and_archives_are_known_by_their_extension() {
        assert_eq!(doc_kind("a/b.PDF"), Some(Doc::Filtered));
        assert_eq!(doc_kind("x.docx"), Some(Doc::Filtered));
        assert_eq!(doc_kind("x.zip"), Some(Doc::Archive));
        assert_eq!(doc_kind("x.tar.gz"), Some(Doc::Archive));
        assert_eq!(doc_kind("x.txt"), None);
        assert_eq!(doc_kind("Makefile"), None);
    }

    #[test]
    fn the_tree_names_its_documents_with_their_blobs() {
        let out = b"100644 blob aaa\ta.txt\x00100644 blob bbb\tdir/r e.pdf\x00040000 tree ccc\tsub\x00100644 blob ddd\tz.zip\x00";
        assert_eq!(
            docs_in_tree(out),
            [
                ("dir/r e.pdf".to_string(), "bbb".to_string()),
                ("z.zip".to_string(), "ddd".to_string())
            ]
        );
    }

    #[test]
    fn a_documents_line_becomes_a_row_of_its_version() {
        assert_eq!(
            doc_row("r1", "d.pdf", b"3\x1fthe needle"),
            Some(b"r1:d.pdf\x003\x00the needle\n".to_vec())
        );
        assert_eq!(
            doc_row("r1", "a.zip", b"{in/x.txt}\x1f1\x1fzipped needle"),
            Some(b"r1:a.zip{in/x.txt}\x001\x00zipped needle\n".to_vec()),
            "an archive's member"
        );
        assert_eq!(doc_row("r1", "d.pdf", b"no separator"), None);
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn every_version_is_searched_and_each_document_once() {
        let dir = temp_dir("versions_search");
        git(&dir, &["init", "-q", "."]);
        std::fs::write(dir.join("a.txt"), "plain needle\n").unwrap();
        std::fs::write(dir.join("arch.zip"), b"PK\x03\x04 zip bytes").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-qm", "one"]);
        let first = git(&dir, &["rev-parse", "HEAD"]);
        std::fs::write(dir.join("a.txt"), "plain needle\nsecond\n").unwrap();
        git(&dir, &["commit", "-qam", "two"]);
        let second = git(&dir, &["rev-parse", "HEAD"]);
        let stubs = temp_dir("versions_stubs");
        stub(&stubs, "ugrep");
        std::fs::write(
            stubs.join("ugrep.out"),
            "{inner.txt}\x1f1\x1fzipped needle\n",
        )
        .unwrap();
        let mut q = query("needle");
        q.opts.documents = true;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ugrep = stubs.join("ugrep");
        let status = search(
            &q,
            Path::new("git"),
            Some(&ugrep),
            "",
            &dir,
            &mut out,
            &mut err,
        );
        assert_eq!(status, 0, "{}", String::from_utf8_lossy(&err));
        let rows = String::from_utf8(out).unwrap();
        for rev in [&first, &second] {
            assert!(
                rows.contains(&format!("{rev}:a.txt\x001\x00plain needle\n")),
                "{rows:?}"
            );
            assert!(
                rows.contains(&format!(
                    "{rev}:arch.zip{{inner.txt}}\x001\x00zipped needle\n"
                )),
                "{rows:?}"
            );
        }
        let ugrep_calls = calls(&stubs, "ugrep");
        assert_eq!(ugrep_calls.len(), 1, "the same blob once: {ugrep_calls:?}");
        assert!(
            ugrep_calls[0].contains(&"-z".to_string())
                && ugrep_calls[0].contains(&"--label=arch.zip".to_string())
        );
        let stdin = std::fs::read(stubs.join("ugrep.stdin")).unwrap();
        assert_eq!(
            stdin, b"PK\x03\x04 zip bytes",
            "the blob from git, no copy on disk"
        );
        q.revs = vec![first.clone()];
        q.opts.documents = false;
        let mut out = Vec::new();
        search(
            &q,
            Path::new("git"),
            None,
            "",
            &dir,
            &mut out,
            &mut Vec::new(),
        );
        let rows = String::from_utf8(out).unwrap();
        assert!(
            rows.contains(&first) && !rows.contains(&second),
            "only the version chosen: {rows:?}"
        );
        let mut out = Vec::new();
        let none = search(
            &query("zzz"),
            Path::new("git"),
            None,
            "",
            &dir,
            &mut out,
            &mut Vec::new(),
        );
        assert_eq!((none, out.is_empty()), (1, true), "nothing found");
        let _ = PathBuf::new();
    }
}
