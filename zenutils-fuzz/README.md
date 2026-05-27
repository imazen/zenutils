# zenutils-fuzz ![CI](https://img.shields.io/github/actions/workflow/status/imazen/zenutils/ci.yml?style=flat-square&label=CI) ![crates.io](https://img.shields.io/crates/v/zenutils-fuzz?style=flat-square) [![lib.rs](https://img.shields.io/crates/v/zenutils-fuzz?style=flat-square&label=lib.rs&color=blue)](https://lib.rs/crates/zenutils-fuzz) ![docs.rs](https://img.shields.io/docsrs/zenutils-fuzz?style=flat-square) ![License](https://img.shields.io/crates/l/zenutils-fuzz?style=flat-square)

Replays a codec's `fuzz/regression/*` seed corpus as a regression test: walks the
directory, feeds every seed through each registered target, and fails (with
seed-path + target-name context) if one panics.

```rust
use zenutils_fuzz::RegressionSuite;

#[test]
fn fuzz_regression() {
    RegressionSuite::new("fuzz/regression")
        .target("decode_default", |bytes| { let _ = my_codec::decode(bytes); })
        .target("decode_with_limits", |bytes| {
            let _ = my_codec::decode_with_limits(bytes, &my_codec::Limits::default());
        })
        .run();
}
```

Skips dotfiles / `*.md` / `*.txt`; recurses; a missing or empty seed dir is a no-op.

## License

MIT OR Apache-2.0.
