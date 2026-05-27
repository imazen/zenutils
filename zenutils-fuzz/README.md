# zenutils-fuzz ![CI](https://img.shields.io/github/actions/workflow/status/imazen/zenutils/ci.yml?style=flat-square&label=CI) ![crates.io](https://img.shields.io/crates/v/zenutils-fuzz?style=flat-square) [![lib.rs](https://img.shields.io/crates/v/zenutils-fuzz?style=flat-square&label=lib.rs&color=blue)](https://lib.rs/crates/zenutils-fuzz) ![docs.rs](https://img.shields.io/docsrs/zenutils-fuzz?style=flat-square) ![License](https://img.shields.io/crates/l/zenutils-fuzz?style=flat-square)

Shared fuzz-regression seed-corpus runner for the zen codec crates
(`zenwebp`, `zenavif`, `zengif`, `zenjxl-decoder`, `zenbitmaps`,
`image-tiff`, `zenraw`, `zenavif-parse`, `zenflate`, ...).

Each codec ships seed inputs under `fuzz/regression/*` that reproduce
historical fuzzer-found bugs. The codec's `tests/fuzz_regression.rs`
walks that directory and feeds every seed through every fuzz-target
entry point. A seed that used to panic must not panic again.

`RegressionSuite` centralises the walk-dir / skip-meta / read-bytes /
call-each-target / surface-a-useful-failure scaffolding so each codec's
harness shrinks to ~3-8 lines.

## Usage

```rust
use zenutils_fuzz::RegressionSuite;

#[test]
fn fuzz_regression() {
    RegressionSuite::new("fuzz/regression")
        .target("decode_default", |bytes| {
            let _ = my_codec::decode(bytes);
        })
        .target("decode_with_limits", |bytes| {
            let _ = my_codec::decode_with_limits(bytes, &my_codec::Limits::default());
        })
        .run();
}
```

Or one `#[test]` per target (the pattern `image-tiff` and `zenavif`
use, for finer-grained `cargo test <target_name>` selection):

```rust
#[test]
fn fuzz_decode_default() {
    RegressionSuite::new("fuzz/regression")
        .target("decode_default", |bytes| { let _ = my_codec::decode(bytes); })
        .run();
}
```

## Behaviour

* Walks the seed directory recursively. Skips dotfiles, `*.md`, `*.txt`.
* Reads every remaining file as raw bytes and calls every registered
  target with those bytes.
* A panicking target propagates with seed-path + target-name context.
  Panic recovery is NOT silenced — a panic IS the failure signal.
* Missing or empty seed directory is a clean no-op.

## License

AGPL-3.0-only OR LicenseRef-Imazen-Commercial.
