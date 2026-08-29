# zenutils-fuzz ![CI](https://img.shields.io/github/actions/workflow/status/imazen/zenutils/ci.yml?style=flat-square&label=CI) ![crates.io](https://img.shields.io/crates/v/zenutils-fuzz?style=flat-square) [![lib.rs](https://img.shields.io/crates/v/zenutils-fuzz?style=flat-square&label=lib.rs&color=blue)](https://lib.rs/crates/zenutils-fuzz) ![docs.rs](https://img.shields.io/docsrs/zenutils-fuzz?style=flat-square) ![License](https://img.shields.io/crates/l/zenutils-fuzz?style=flat-square)

Replays a codec's `fuzz/regression/*` seed corpus as a regression test: walks the
directory, feeds every seed through each registered target, and fails (with
seed-path + target-name context) if one panics.

```rust
use zenutils_fuzz::RegressionSuite;

#[test]
fn fuzz_regression() {
    RegressionSuite::new("fuzz/regression")
        .min_seeds(7) // this corpus has 7 seeds; fewer means it went missing
        .target("decode_default", |bytes| { let _ = my_codec::decode(bytes); })
        .target("decode_with_limits", |bytes| {
            let _ = my_codec::decode_with_limits(bytes, &my_codec::Limits::default());
        })
        .run();
}
```

Skips dotfiles / `*.md` / `*.txt`; recurses.

## The seed expectation is mandatory

A regression suite that replays *zero* seeds passes — green, fast, and testing
nothing. A renamed directory, a corpus moved to block storage, a `.gitignore`
that swallowed the seeds, a path the target platform refuses to open: every one
of those lands on that same outcome, and nothing in the test output tells it
apart from a clean run.

zenjxl-decoder's wasm CI legs replayed zero seeds and reported green for the
entire life of its harness. The corpus lives one directory above the crate and
the path was built with `.join("..")`; WASI's preopen resolution refuses to
traverse a literal `..`, the scan failed, the failure was swallowed, and the
suite no-opped on every wasm run.

So `run()` will not do anything until you declare what you expect:

| | seed dir absent | unreadable / not a dir | present, 0 seeds | present, `k < n` | present, `k >= n` |
|---|---|---|---|---|---|
| *(nothing declared)* | panic | panic | panic | panic | panic |
| `.allow_empty_corpus()` | pass | **panic** | pass | pass | pass |
| `.min_seeds(0)` | **panic** | **panic** | pass | pass | pass |
| `.min_seeds(n >= 1)` | **panic** | **panic** | **panic** | **panic** | pass |

The three failures say which one happened — "does not exist", "contains no
seeds", and "yielded k seed(s) … n seed(s) went missing" have different causes
and different fixes.

`run()` returns a `RegressionReport` with `seeds_replayed()`, `targets()`,
`invocations()` and `seed_paths()`, so a harness never has to count corpus
files itself. That count is taken *after* the dotfile/`*.md`/`*.txt` filter —
hand-rolled counters routinely include `README.md` and guard a number higher
than what actually replays.

`min_seeds(0)` is for a corpus that is deliberately empty (a repro too large to
commit, say) but whose directory must still exist. `allow_empty_corpus()` is
for a crate that has no corpus yet; prefer `min_seeds` the moment it has one,
because `allow_empty_corpus()` cannot tell "no corpus yet" from "the corpus
vanished".

## License

MIT OR Apache-2.0.
