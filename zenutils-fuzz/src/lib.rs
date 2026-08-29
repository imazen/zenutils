//! Fuzz-regression seed-corpus runner shared across zen codec crates.
//!
//! Each zen codec keeps a small set of hand-minimized seed inputs under
//! `fuzz/regression/*` that reproduce historical fuzzer-found bugs. The
//! codec's `tests/fuzz_regression.rs` integration test walks that
//! directory and feeds every seed through every fuzz-target entry point.
//! A seed that used to panic must now decode without panicking.
//!
//! Before this crate, every codec carried ~50 lines of near-identical
//! scaffolding (walk dir, skip dotfiles/README, read bytes, call each
//! target, surface a useful failure message). This crate centralises
//! that scaffolding so each codec's harness shrinks to ~3-8 lines.
//!
//! # Usage
//!
//! ```no_run
//! use zenutils_fuzz::RegressionSuite;
//!
//! #[test]
//! fn fuzz_regression() {
//!     let report = RegressionSuite::new("fuzz/regression")
//!         .min_seeds(7) // this corpus has 7 seeds; fewer means it went missing
//!         .target("decode_default", |bytes| {
//!             let _ = my_codec::decode(bytes);
//!         })
//!         .target("decode_with_limits", |bytes| {
//!             let _ = my_codec::decode_with_limits(bytes, &my_codec::Limits::default());
//!         })
//!         .run();
//!     println!("{report}");
//! }
//! ```
//!
//! # Why a seed expectation is mandatory
//!
//! A regression suite that replays *zero* seeds passes. It passes loudly,
//! quickly, and green — while testing nothing at all. Every way a corpus can
//! go missing (a directory renamed, a corpus moved to block storage, a
//! `.gitignore` that swallowed the seeds, a path the target platform refuses
//! to open) lands on exactly that outcome, and nothing in the test output
//! distinguishes it from a corpus that ran clean.
//!
//! This is not hypothetical. Surveyed across the zen workspace in August 2026:
//!
//! * **zenjxl-decoder's wasm CI legs replayed zero seeds and reported green
//!   for the entire life of the harness.** Its corpus sits one directory above
//!   the crate, and the path was built with `.join("..")`. WASI's preopen
//!   resolution refuses to traverse a literal `..` component even when the
//!   target is inside a preopened directory, so the directory scan failed, the
//!   failure was swallowed, and the suite no-opped. Every wasm run was a
//!   vacuous pass. It surfaced only when a hand-rolled seed-count assertion
//!   was added for unrelated reasons and immediately turned both wasm legs
//!   red — which is to say it was caught by luck, after the fact, by the very
//!   check this API now provides.
//! * **zenflate replays zero seeds** — legitimately: its one open repro needs
//!   a ~19 MB input, four orders of magnitude past the per-seed size ceiling,
//!   so it is gated by a unit test instead. That is a fine decision, but
//!   nothing in the suite distinguished it from an accident, so zenflate had
//!   to pin the zero by hand to make it a visible choice.
//! * **zencodec's harness had to spell out that a directory it cannot read is
//!   a broken gate rather than "nothing to regress"** — the early-return on an
//!   unreadable directory left it counting seeds it never replayed.
//!
//! Twelve repos independently grew a hand-rolled seed-count guard to
//! compensate — seven of them consumers of this crate. Eight carry a
//! byte-identical 25-line copy of a helper that re-implements this crate's own
//! filter; the guards written from scratch instead mostly count `README.md`
//! as a seed, and so guard a number one or more higher than what actually
//! replays. That is the second reason [`RegressionReport::seeds_replayed`]
//! exists: the only counter that cannot drift from the filter is the one
//! inside it.
//!
//! So [`RegressionSuite::run`] refuses to run until the caller declares what
//! it expects, via exactly one of:
//!
//! * [`RegressionSuite::min_seeds`] — the seed directory must exist, be
//!   readable, and yield at least `n` seeds.
//! * [`RegressionSuite::allow_empty_corpus`] — this crate genuinely has no
//!   corpus yet (or a deliberately empty one); replaying zero seeds is fine.
//!   An *unreadable* directory still fails, because that is always a bug.
//!
//! The permissive behaviour is still available; it is just no longer what you
//! get by saying nothing.
//!
//! # Behaviour
//!
//! * Walks the seed directory recursively. Skips dotfiles (`.gitkeep`,
//!   `.DS_Store`), `*.md`, and `*.txt`.
//! * Reads every remaining file as raw bytes and calls every registered
//!   target with those bytes.
//! * If a target panics, the panic propagates with seed-path + target-name
//!   context. A panic IS the failure signal we want — recovery is not
//!   silenced.
//! * The seed expectation is checked *before* any target runs, so a corpus
//!   that went missing is reported as a missing corpus rather than as
//!   whatever the surviving seeds happen to do.
//! * [`RegressionSuite::run`] returns a [`RegressionReport`] carrying the
//!   number of seeds actually replayed, so callers never have to count files
//!   themselves. The report counts what the suite *replayed* — meta files
//!   are already filtered out, which is the count hand-rolled guards
//!   routinely got wrong.
//!
//! ## What each expectation does
//!
//! | seed directory state          | (nothing declared) | `allow_empty_corpus()` | `min_seeds(0)` | `min_seeds(n >= 1)` |
//! |-------------------------------|--------------------|------------------------|----------------|---------------------|
//! | absent                        | panic: undeclared  | pass, 0 replayed       | panic: absent  | panic: absent       |
//! | unreadable / not a directory  | panic: undeclared  | panic: I/O             | panic: I/O     | panic: I/O          |
//! | present, 0 seeds              | panic: undeclared  | pass, 0 replayed       | pass           | panic: empty        |
//! | present, `k < n` seeds        | panic: undeclared  | pass, `k` replayed     | pass           | panic: short        |
//! | present, `k >= n` seeds       | panic: undeclared  | pass, `k` replayed     | pass           | pass                |
//!
//! `min_seeds(0)` is the setting for a corpus that is *deliberately* empty
//! but whose directory must still be there — e.g. a repro too large to
//! commit, documented by a `README.md` in the otherwise-empty directory.
//! It is stricter than [`RegressionSuite::allow_empty_corpus`] (which also
//! tolerates the directory being gone) and weaker than `min_seeds(1)`.

use std::fmt;
use std::fs;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

type TargetFn = Box<dyn Fn(&[u8]) + Send + Sync>;

/// What the caller requires of the seed corpus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeedExpectation {
    /// Nothing declared. `run()` refuses to proceed.
    Undeclared,
    /// Zero seeds — and even an absent directory — are acceptable.
    AllowEmpty,
    /// The directory must exist and be readable, and yield at least `n` seeds.
    AtLeast(usize),
}

/// Why scanning the seed directory did not produce a seed list.
#[derive(Debug)]
enum ScanError {
    /// The seed directory does not exist.
    Absent,
    /// The seed directory (or something inside it) could not be read, or the
    /// seed path is not a directory at all.
    Io { path: PathBuf, err: io::Error },
}

/// What a completed [`RegressionSuite::run`] actually did.
///
/// The counts describe the seeds that were *replayed*: dotfiles, `*.md` and
/// `*.txt` were filtered out before this was built, so `seeds_replayed()` is
/// the number a coverage gate should care about — not the number of files in
/// the directory.
#[derive(Clone, Debug)]
pub struct RegressionReport {
    seed_dir: PathBuf,
    seed_paths: Vec<PathBuf>,
    target_count: usize,
}

impl RegressionReport {
    /// Number of seed files replayed through every target.
    pub fn seeds_replayed(&self) -> usize {
        self.seed_paths.len()
    }

    /// Number of registered targets.
    pub fn targets(&self) -> usize {
        self.target_count
    }

    /// `seeds_replayed() * targets()` — total target invocations.
    pub fn invocations(&self) -> usize {
        self.seed_paths.len() * self.target_count
    }

    /// The replayed seed paths, sorted.
    pub fn seed_paths(&self) -> &[PathBuf] {
        &self.seed_paths
    }

    /// The seed directory that was scanned.
    pub fn seed_dir(&self) -> &Path {
        &self.seed_dir
    }
}

impl fmt::Display for RegressionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "fuzz regression: replayed {} seed(s) from {:?} through {} target(s) = {} invocation(s)",
            self.seeds_replayed(),
            self.seed_dir,
            self.target_count,
            self.invocations()
        )
    }
}

/// Builder + runner for a fuzz-regression seed corpus.
///
/// A suite must declare a seed expectation — [`min_seeds`](Self::min_seeds)
/// or [`allow_empty_corpus`](Self::allow_empty_corpus) — before
/// [`run`](Self::run) will do anything. See the [crate docs][crate] for why
/// the permissive behaviour is not the default.
pub struct RegressionSuite {
    seed_dir: PathBuf,
    targets: Vec<(String, TargetFn)>,
    expectation: SeedExpectation,
}

impl RegressionSuite {
    pub fn new<P: Into<PathBuf>>(seed_dir: P) -> Self {
        Self {
            seed_dir: seed_dir.into(),
            targets: Vec::new(),
            expectation: SeedExpectation::Undeclared,
        }
    }

    pub fn target<F>(mut self, name: &str, f: F) -> Self
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.targets.push((name.to_string(), Box::new(f)));
        self
    }

    /// Require the corpus to replay at least `n` seeds.
    ///
    /// The seed directory must exist and be readable; a missing, unreadable,
    /// empty, or short corpus fails [`run`](Self::run) with a message saying
    /// which of those it was. `n` counts *replayed* seeds — dotfiles, `*.md`
    /// and `*.txt` never count, so `README.md` in the corpus directory does
    /// not inflate the number you pass here.
    ///
    /// `min_seeds(0)` still requires the directory to exist and be readable,
    /// but accepts a corpus with no seeds in it. That is the setting for a
    /// deliberately-empty corpus (a repro too large to commit, say) whose
    /// directory must not silently disappear.
    ///
    /// Calling this replaces any previous expectation.
    pub fn min_seeds(mut self, n: usize) -> Self {
        self.expectation = SeedExpectation::AtLeast(n);
        self
    }

    /// Accept a corpus that is absent or empty.
    ///
    /// For crates that genuinely have no seeds yet. Replaying zero seeds
    /// passes, and so does a missing seed directory — but a directory that
    /// exists and cannot be read (or a seed path that is not a directory)
    /// still fails, because that is a broken harness rather than an empty
    /// corpus.
    ///
    /// Prefer [`min_seeds`](Self::min_seeds) the moment the crate has a
    /// single seed: this setting cannot tell "no corpus yet" from "the
    /// corpus vanished".
    ///
    /// Calling this replaces any previous expectation.
    pub fn allow_empty_corpus(mut self) -> Self {
        self.expectation = SeedExpectation::AllowEmpty;
        self
    }

    /// Replay every seed through every target.
    ///
    /// Panics — which is what a `#[test]` wants — if the seed expectation is
    /// undeclared, if no targets were registered, if the corpus does not meet
    /// the declared expectation, or if a target panics on a seed.
    pub fn run(self) -> RegressionReport {
        if self.expectation == SeedExpectation::Undeclared {
            panic!(
                "RegressionSuite at {:?}: no seed expectation declared, so this suite \
                 would pass without proving it replayed anything. Declare one: \
                 `.min_seeds(n)` (the corpus must exist and yield at least n seeds) or \
                 `.allow_empty_corpus()` (this crate genuinely has no seeds yet). \
                 See the zenutils-fuzz crate docs for why zero-seed green runs are the \
                 failure this guards against.",
                self.seed_dir
            );
        }

        if self.targets.is_empty() {
            panic!(
                "RegressionSuite at {:?}: no targets registered. \
                 Call `.target(name, fn)` at least once before `.run()`.",
                self.seed_dir
            );
        }

        let seeds = match collect_seeds(&self.seed_dir) {
            Ok(seeds) => seeds,
            Err(ScanError::Absent) => match self.expectation {
                SeedExpectation::AllowEmpty => Vec::new(),
                SeedExpectation::AtLeast(n) => panic!(
                    "RegressionSuite: seed directory {:?} does not exist, but at least \
                     {} seed(s) were required. The corpus was renamed, never checked out, \
                     or the path does not resolve on this target (a path component of \
                     `..` is refused by WASI preopen resolution, for example — resolve \
                     the path from CARGO_MANIFEST_DIR instead). If this crate really has \
                     no corpus, say so with `.allow_empty_corpus()`.",
                    self.seed_dir, n
                ),
                SeedExpectation::Undeclared => unreachable!("checked above"),
            },
            Err(ScanError::Io { path, err }) => panic!(
                "RegressionSuite: seed directory {:?} exists but could not be scanned \
                 ({:?}: {}). This is a broken harness, not an empty corpus: the suite \
                 would otherwise have replayed nothing and passed.",
                self.seed_dir, path, err
            ),
        };

        if let SeedExpectation::AtLeast(n) = self.expectation {
            if seeds.len() < n {
                if seeds.is_empty() {
                    panic!(
                        "RegressionSuite: seed directory {:?} exists but contains no \
                         seeds, and at least {} were required. (Dotfiles, `*.md` and \
                         `*.txt` are never counted as seeds — a directory holding only \
                         a README counts as empty.) The corpus was emptied, or the \
                         seeds are gitignored. If it is deliberately empty, declare \
                         `.min_seeds(0)`.",
                        self.seed_dir, n
                    );
                }
                panic!(
                    "RegressionSuite: seed directory {:?} yielded {} seed(s) but at \
                     least {} were required — {} seed(s) went missing. Replayed: {:?}",
                    self.seed_dir,
                    seeds.len(),
                    n,
                    n - seeds.len(),
                    seeds
                );
            }
        }

        for seed_path in &seeds {
            let bytes = match fs::read(seed_path) {
                Ok(b) => b,
                Err(e) => panic!(
                    "RegressionSuite: failed to read seed {:?}: {}",
                    seed_path, e
                ),
            };

            for (target_name, target_fn) in &self.targets {
                let res = catch_unwind(AssertUnwindSafe(|| {
                    target_fn(&bytes);
                }));
                if let Err(payload) = res {
                    let msg = panic_payload_str(&*payload);
                    panic!(
                        "RegressionSuite: target {:?} panicked on seed {:?} \
                         ({} bytes, first 32: {:?}): {}",
                        target_name,
                        seed_path,
                        bytes.len(),
                        &bytes[..bytes.len().min(32)],
                        msg
                    );
                }
            }
        }

        RegressionReport {
            seed_dir: self.seed_dir,
            seed_paths: seeds,
            target_count: self.targets.len(),
        }
    }
}

fn collect_seeds(dir: &Path) -> Result<Vec<PathBuf>, ScanError> {
    match fs::metadata(dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(ScanError::Io {
                path: dir.to_path_buf(),
                err: io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "seed path exists but is not a directory",
                ),
            });
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Err(ScanError::Absent),
        Err(err) => {
            return Err(ScanError::Io {
                path: dir.to_path_buf(),
                err,
            });
        }
    }
    let mut seeds = Vec::new();
    walk(dir, &mut seeds)?;
    seeds.sort();
    Ok(seeds)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), ScanError> {
    let entries = fs::read_dir(dir).map_err(|err| ScanError::Io {
        path: dir.to_path_buf(),
        err,
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| ScanError::Io {
            path: dir.to_path_buf(),
            err,
        })?;
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let ft = entry.file_type().map_err(|err| ScanError::Io {
            path: path.clone(),
            err,
        })?;
        if ft.is_dir() {
            walk(&path, out)?;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".txt") {
            continue;
        }
        out.push(path);
    }
    Ok(())
}

fn panic_payload_str(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_tmp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("zenutils-fuzz-test-{}-{}-{}", name, pid, ts));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// Run `f`, returning the panic message if it panicked.
    fn panic_msg<F: FnOnce()>(f: F) -> Option<String> {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let res = catch_unwind(AssertUnwindSafe(f));
        std::panic::set_hook(prev);
        match res {
            Ok(()) => None,
            Err(payload) => Some(panic_payload_str(&*payload)),
        }
    }

    // ---- (d) the permissive behaviour is opt-in, not the default ----

    #[test]
    fn undeclared_expectation_refuses_to_run() {
        let dir = make_tmp_dir("undeclared");
        fs::write(dir.join("seed"), b"x").unwrap();
        let called = Arc::new(AtomicUsize::new(0));
        let c = called.clone();
        let msg = panic_msg(|| {
            RegressionSuite::new(&dir)
                .target("t", move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
                .run();
        })
        .expect("undeclared expectation must fail");
        let _ = fs::remove_dir_all(&dir);
        assert!(msg.contains("no seed expectation declared"), "got: {msg}");
        assert!(msg.contains("min_seeds"), "got: {msg}");
        assert!(msg.contains("allow_empty_corpus"), "got: {msg}");
        // It refuses *before* running anything.
        assert_eq!(called.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn allow_empty_corpus_tolerates_absent_dir() {
        let dir = make_tmp_dir("allow-absent");
        let nonexistent = dir.join("nope");
        let report = RegressionSuite::new(&nonexistent)
            .allow_empty_corpus()
            .target("ignored", |_| panic!("should not be called"))
            .run();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(report.seeds_replayed(), 0);
        assert_eq!(report.invocations(), 0);
    }

    #[test]
    fn allow_empty_corpus_tolerates_empty_dir() {
        let dir = make_tmp_dir("allow-empty");
        let called = Arc::new(AtomicUsize::new(0));
        let c = called.clone();
        let report = RegressionSuite::new(&dir)
            .allow_empty_corpus()
            .target("t", move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .run();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(called.load(Ordering::SeqCst), 0);
        assert_eq!(report.seeds_replayed(), 0);
    }

    // ---- (a) missing / unreadable / empty is a FAILURE under a minimum ----

    #[test]
    fn min_seeds_fails_on_absent_dir() {
        let dir = make_tmp_dir("min-absent");
        let nonexistent = dir.join("nope");
        let msg = panic_msg(|| {
            RegressionSuite::new(&nonexistent)
                .min_seeds(1)
                .target("t", |_| {})
                .run();
        })
        .expect("absent dir under a minimum must fail");
        let _ = fs::remove_dir_all(&dir);
        assert!(msg.contains("does not exist"), "got: {msg}");
        assert!(msg.contains("allow_empty_corpus"), "got: {msg}");
    }

    #[test]
    fn min_seeds_zero_still_fails_on_absent_dir() {
        // A deliberately-empty corpus still has to have its directory there.
        let dir = make_tmp_dir("min0-absent");
        let nonexistent = dir.join("nope");
        let msg = panic_msg(|| {
            RegressionSuite::new(&nonexistent)
                .min_seeds(0)
                .target("t", |_| {})
                .run();
        })
        .expect("absent dir must fail even at min_seeds(0)");
        let _ = fs::remove_dir_all(&dir);
        assert!(msg.contains("does not exist"), "got: {msg}");
    }

    #[test]
    fn min_seeds_zero_accepts_present_but_empty_dir() {
        let dir = make_tmp_dir("min0-empty");
        fs::write(
            dir.join("README.md"),
            b"repro is 19 MB, gated by a unit test",
        )
        .unwrap();
        let report = RegressionSuite::new(&dir)
            .min_seeds(0)
            .target("t", |_| panic!("no seeds to run"))
            .run();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(report.seeds_replayed(), 0);
    }

    #[test]
    fn seed_path_that_is_a_file_fails_even_when_empty_is_allowed() {
        let dir = make_tmp_dir("not-a-dir");
        let file = dir.join("regression");
        fs::write(&file, b"oops").unwrap();
        let msg = panic_msg(|| {
            RegressionSuite::new(&file)
                .allow_empty_corpus()
                .target("t", |_| {})
                .run();
        })
        .expect("a non-directory seed path must fail even under allow_empty_corpus");
        let _ = fs::remove_dir_all(&dir);
        assert!(msg.contains("could not be scanned"), "got: {msg}");
        assert!(msg.contains("not a directory"), "got: {msg}");
    }

    // ---- (c) the three states are distinguishable from the message ----

    #[test]
    fn empty_dir_and_short_corpus_report_different_causes() {
        let empty = make_tmp_dir("distinguish-empty");
        let empty_msg = panic_msg(|| {
            RegressionSuite::new(&empty)
                .min_seeds(2)
                .target("t", |_| {})
                .run();
        })
        .expect("empty dir under min_seeds(2) must fail");
        let _ = fs::remove_dir_all(&empty);

        let short = make_tmp_dir("distinguish-short");
        fs::write(short.join("only_seed"), b"x").unwrap();
        let short_msg = panic_msg(|| {
            RegressionSuite::new(&short)
                .min_seeds(2)
                .target("t", |_| {})
                .run();
        })
        .expect("short corpus under min_seeds(2) must fail");
        let _ = fs::remove_dir_all(&short);

        let absent = make_tmp_dir("distinguish-absent");
        let gone = absent.join("nope");
        let absent_msg = panic_msg(|| {
            RegressionSuite::new(&gone)
                .min_seeds(2)
                .target("t", |_| {})
                .run();
        })
        .expect("absent dir under min_seeds(2) must fail");
        let _ = fs::remove_dir_all(&absent);

        assert!(empty_msg.contains("contains no seeds"), "got: {empty_msg}");
        assert!(short_msg.contains("went missing"), "got: {short_msg}");
        assert!(short_msg.contains("only_seed"), "got: {short_msg}");
        assert!(absent_msg.contains("does not exist"), "got: {absent_msg}");

        // The three causes must not be confusable with one another.
        assert!(!empty_msg.contains("does not exist"), "got: {empty_msg}");
        assert!(!short_msg.contains("contains no seeds"), "got: {short_msg}");
        assert!(
            !absent_msg.contains("contains no seeds"),
            "got: {absent_msg}"
        );
    }

    // ---- (b) the suite reports what it replayed ----

    #[test]
    fn report_counts_seeds_targets_and_invocations() {
        let dir = make_tmp_dir("report");
        fs::write(dir.join("seed_a"), b"hello").unwrap();
        fs::write(dir.join("seed_b.bin"), b"world!!").unwrap();
        fs::write(dir.join("seed_c"), b"!").unwrap();
        let report = RegressionSuite::new(&dir)
            .min_seeds(3)
            .target("t1", |_| {})
            .target("t2", |_| {})
            .run();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(report.seeds_replayed(), 3);
        assert_eq!(report.targets(), 2);
        assert_eq!(report.invocations(), 6);
        assert_eq!(report.seed_paths().len(), 3);
        let rendered = report.to_string();
        assert!(rendered.contains("replayed 3 seed(s)"), "got: {rendered}");
        assert!(rendered.contains("6 invocation(s)"), "got: {rendered}");
    }

    /// The trap several hand-rolled guards fell into: counting `README.md`
    /// (and `.gitkeep`, and `notes.txt`) as seeds, so a corpus of one real
    /// seed looked like four and the guard was set to the wrong number.
    #[test]
    fn meta_files_are_not_counted_as_seeds() {
        let dir = make_tmp_dir("readme-trap");
        fs::write(dir.join("real_seed.bin"), b"x").unwrap();
        fs::write(dir.join("README.md"), b"# how this corpus works").unwrap();
        fs::write(dir.join("notes.txt"), b"see issue 12").unwrap();
        fs::write(dir.join(".gitkeep"), b"").unwrap();
        fs::write(dir.join("PROVENANCE.MD"), b"uppercase extension too").unwrap();

        // A naive `read_dir().count()` sees 5 entries. The suite replays 1.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 5);

        let report = RegressionSuite::new(&dir)
            .min_seeds(1)
            .target("t", |_| {})
            .run();
        assert_eq!(report.seeds_replayed(), 1);
        assert_eq!(
            report.seed_paths()[0].file_name().unwrap(),
            std::ffi::OsStr::new("real_seed.bin")
        );

        // And a guard set from the naive count fails loudly rather than
        // pretending four seeds ran.
        let msg = panic_msg(|| {
            RegressionSuite::new(&dir)
                .min_seeds(4)
                .target("t", |_| {})
                .run();
        })
        .expect("min_seeds(4) against 1 real seed must fail");
        let _ = fs::remove_dir_all(&dir);
        assert!(msg.contains("yielded 1 seed(s)"), "got: {msg}");
        assert!(msg.contains("3 seed(s) went missing"), "got: {msg}");
    }

    // ---- pre-existing behaviour, still guarded ----

    #[test]
    fn runs_every_target_on_every_seed_and_skips_meta() {
        let dir = make_tmp_dir("multi");
        fs::write(dir.join("seed_a"), b"hello").unwrap();
        fs::write(dir.join("seed_b.bin"), b"world!!").unwrap();
        fs::write(dir.join(".gitkeep"), b"").unwrap();
        fs::write(dir.join("README.md"), b"# notes").unwrap();
        fs::write(dir.join("notes.txt"), b"hi").unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let total_bytes = Arc::new(AtomicUsize::new(0));
        let c1 = count.clone();
        let b1 = total_bytes.clone();
        let c2 = count.clone();
        let report = RegressionSuite::new(&dir)
            .min_seeds(2)
            .target("t1", move |bytes| {
                c1.fetch_add(1, Ordering::SeqCst);
                b1.fetch_add(bytes.len(), Ordering::SeqCst);
            })
            .target("t2", move |_| {
                c2.fetch_add(1, Ordering::SeqCst);
            })
            .run();
        // 2 real seeds × 2 targets = 4 invocations (meta files filtered out)
        assert_eq!(count.load(Ordering::SeqCst), 4);
        assert_eq!(total_bytes.load(Ordering::SeqCst), 12); // 5 + 7, t1 only
        assert_eq!(report.seeds_replayed(), 2);
        assert_eq!(report.invocations(), 4);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn panic_in_target_surfaces_seed_and_target_name() {
        let dir = make_tmp_dir("panic");
        fs::write(dir.join("bad_seed.bin"), b"trigger").unwrap();
        let msg = panic_msg(|| {
            RegressionSuite::new(&dir)
                .min_seeds(1)
                .target("panicky_target", |_| panic!("oh no"))
                .run();
        })
        .expect("suite must propagate the panic");
        let _ = fs::remove_dir_all(&dir);
        assert!(msg.contains("panicky_target"), "got: {msg}");
        assert!(msg.contains("bad_seed.bin"), "got: {msg}");
        assert!(msg.contains("oh no"), "got: {msg}");
    }

    #[test]
    fn empty_targets_with_seeds_panics() {
        let dir = make_tmp_dir("empty-targets");
        fs::write(dir.join("seed"), b"x").unwrap();
        let msg = panic_msg(|| {
            RegressionSuite::new(&dir).min_seeds(1).run();
        })
        .expect("no targets must fail");
        let _ = fs::remove_dir_all(&dir);
        assert!(msg.contains("no targets registered"), "got: {msg}");
    }

    #[test]
    fn recurses_into_subdirs() {
        let dir = make_tmp_dir("recurse");
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(dir.join("top"), b"a").unwrap();
        fs::write(sub.join("nested"), b"bb").unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let report = RegressionSuite::new(&dir)
            .min_seeds(2)
            .target("t", move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .run();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(report.seeds_replayed(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn last_expectation_wins() {
        let dir = make_tmp_dir("last-wins");
        // allow_empty_corpus() then min_seeds(1) => the minimum applies.
        let msg = panic_msg(|| {
            RegressionSuite::new(&dir)
                .allow_empty_corpus()
                .min_seeds(1)
                .target("t", |_| {})
                .run();
        })
        .expect("min_seeds after allow_empty_corpus must apply");
        assert!(msg.contains("contains no seeds"), "got: {msg}");

        // min_seeds(1) then allow_empty_corpus() => permissive applies.
        let report = RegressionSuite::new(&dir)
            .min_seeds(1)
            .allow_empty_corpus()
            .target("t", |_| {})
            .run();
        assert_eq!(report.seeds_replayed(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
