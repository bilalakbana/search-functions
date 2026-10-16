//! Helpers shared by the unit tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A new, empty folder under `target/test-tmp/` for one test. Nothing is ever deleted: the folders
/// go away with `cargo clean`.
pub fn temp_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-tmp")
        .join(format!("{name}-{}-{nanos}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes an executable script (mode 755) at `path`. A child process writes it: were the file open for
/// writing in this process while another test thread forks, the fork would hold that descriptor until
/// its exec, and running the script meanwhile fails with "Text file busy" (ETXTBSY).
pub fn executable(path: &Path, script: &str) {
    let status = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"",
            "sh",
            script,
        ])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success(), "writing {}", path.display());
}

/// A stub program NAME in `dir`. Every call appends its arguments (each ended by NUL, the call by a
/// newline) to NAME.args and the values of `$SF_VALUE` and `$WSLENV` the same way to NAME.env, and
/// writes its stdin to NAME.stdin. It prints NAME.out to stdout and NAME.err to stderr and exits
/// with the code in NAME.exit, each when that file is there.
pub fn stub(dir: &Path, name: &str) {
    let script = format!(
        "#!/bin/sh\nd='{d}'\nfor a in \"$@\"; do printf '%s\\0' \"$a\" >> \"$d/{name}.args\"; done\n\
         printf '\\n' >> \"$d/{name}.args\"\n\
         printf 'SF_VALUE=%s\\0WSLENV=%s\\0\\n' \"$SF_VALUE\" \"$WSLENV\" >> \"$d/{name}.env\"\n\
         /bin/cat > \"$d/{name}.stdin\"\n\
         [ -f \"$d/{name}.out\" ] && /bin/cat \"$d/{name}.out\"\n\
         [ -f \"$d/{name}.err\" ] && /bin/cat \"$d/{name}.err\" >&2\n\
         [ -f \"$d/{name}.exit\" ] && exit \"$(/bin/cat \"$d/{name}.exit\")\"\nexit 0\n",
        d = dir.display()
    );
    executable(&dir.join(name), &script);
}

/// The argument lists of every call of the stub NAME in `dir`.
pub fn calls(dir: &Path, name: &str) -> Vec<Vec<String>> {
    records(&dir.join(format!("{name}.args")))
}

/// `SF_VALUE=…` and `WSLENV=…` as the stub NAME saw them, per call.
pub fn envs(dir: &Path, name: &str) -> Vec<Vec<String>> {
    records(&dir.join(format!("{name}.env")))
}

fn records(file: &Path) -> Vec<Vec<String>> {
    let Ok(log) = std::fs::read(file) else {
        return Vec::new();
    };
    let calls = log.strip_suffix(b"\n").unwrap_or(&log);
    calls
        .split(|&b| b == b'\n')
        .map(|l| {
            l.split(|&b| b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect()
        })
        .collect()
}

/// The test binary's allocator: the system's, counting the live heap bytes of each thread, so a test
/// can measure what its own work allocates while other tests run in parallel.
pub struct CountingAlloc;

thread_local! {
    static LIVE: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn count(delta: isize) {
    let _ = LIVE.try_with(|l| l.set(l.get() + delta));
}

// SAFETY: forwards to the system allocator unchanged; only counts.
unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count(layout.size() as isize);
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        count(-(layout.size() as isize));
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        count(new_size as isize - layout.size() as isize);
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// The heap bytes this thread holds right now.
pub fn live_heap() -> isize {
    LIVE.with(|l| l.get())
}
