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
//!     RegressionSuite::new("fuzz/regression")
//!         .target("decode_default", |bytes| {
//!             let _ = my_codec::decode(bytes);
//!         })
//!         .target("decode_with_limits", |bytes| {
//!             let _ = my_codec::decode_with_limits(bytes, &my_codec::Limits::default());
//!         })
//!         .run();
//! }
//! ```
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
//! * Missing or empty seed directory is a clean no-op (matches the
//!   historical behaviour every codec hand-rolled).

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

type TargetFn = Box<dyn Fn(&[u8]) + Send + Sync>;

/// Builder + runner for a fuzz-regression seed corpus.
pub struct RegressionSuite {
    seed_dir: PathBuf,
    targets: Vec<(String, TargetFn)>,
}

impl RegressionSuite {
    pub fn new<P: Into<PathBuf>>(seed_dir: P) -> Self {
        Self {
            seed_dir: seed_dir.into(),
            targets: Vec::new(),
        }
    }

    pub fn target<F>(mut self, name: &str, f: F) -> Self
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.targets.push((name.to_string(), Box::new(f)));
        self
    }

    pub fn run(self) {
        let seeds = match collect_seeds(&self.seed_dir) {
            Some(s) => s,
            None => return,
        };

        if self.targets.is_empty() {
            panic!(
                "RegressionSuite at {:?}: no targets registered. \
                 Call `.target(name, fn)` at least once before `.run()`.",
                self.seed_dir
            );
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
    }
}

fn collect_seeds(dir: &Path) -> Option<Vec<PathBuf>> {
    if !dir.exists() {
        return None;
    }
    let mut seeds = Vec::new();
    walk(dir, &mut seeds);
    seeds.sort();
    Some(seeds)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            walk(&path, out);
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

    #[test]
    fn no_seed_dir_is_noop() {
        let dir = make_tmp_dir("no-seed");
        let nonexistent = dir.join("nope");
        RegressionSuite::new(&nonexistent)
            .target("ignored", |_| panic!("should not be called"))
            .run();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_seed_dir_is_noop() {
        let dir = make_tmp_dir("empty-seed");
        let called = Arc::new(AtomicUsize::new(0));
        let c = called.clone();
        RegressionSuite::new(&dir)
            .target("t", move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .run();
        assert_eq!(called.load(Ordering::SeqCst), 0);
        let _ = fs::remove_dir_all(&dir);
    }

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
        RegressionSuite::new(&dir)
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
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn panic_in_target_surfaces_seed_and_target_name() {
        let dir = make_tmp_dir("panic");
        fs::write(dir.join("bad_seed.bin"), b"trigger").unwrap();
        let res = catch_unwind(AssertUnwindSafe(|| {
            RegressionSuite::new(&dir)
                .target("panicky_target", |_| panic!("oh no"))
                .run();
        }));
        let _ = fs::remove_dir_all(&dir);
        let err = res.expect_err("suite must propagate the panic");
        let msg = panic_payload_str(&*err);
        assert!(msg.contains("panicky_target"), "got: {msg}");
        assert!(msg.contains("bad_seed.bin"), "got: {msg}");
        assert!(msg.contains("oh no"), "got: {msg}");
    }

    #[test]
    fn empty_targets_with_seeds_panics() {
        let dir = make_tmp_dir("empty-targets");
        fs::write(dir.join("seed"), b"x").unwrap();
        let res = catch_unwind(AssertUnwindSafe(|| {
            RegressionSuite::new(&dir).run();
        }));
        let _ = fs::remove_dir_all(&dir);
        assert!(res.is_err());
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
        RegressionSuite::new(&dir)
            .target("t", move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .run();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
