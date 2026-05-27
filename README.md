# zenutils

Home of the `zenutils-*` family: small, focused utility crates shared across
the zen image-processing codebase. Each member crate does one thing, carries
few or no dependencies, and is published independently.

This repository is a Cargo workspace. New shared helpers are added as new
`zenutils-*` workspace members rather than as ad-hoc path dependencies, so
every consumer pulls them from one versioned source.

## Members

| Crate | Description |
|-------|-------------|
| [`zenutils-fuzz`](zenutils-fuzz/) | Fuzz-regression seed-corpus runner (`RegressionSuite`) shared across zen codec crates. |

## zenutils-fuzz

Each zen codec ships seed inputs under `fuzz/regression/*` that reproduce
historical fuzzer-found bugs. The codec's `tests/fuzz_regression.rs` walks
that directory and feeds every seed through every fuzz-target entry point.
A seed that used to panic must not panic again. `RegressionSuite` centralises
the walk-dir / read-bytes / call-each-target scaffolding so each codec's
harness shrinks to a few lines.

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

See [`zenutils-fuzz/README.md`](zenutils-fuzz/README.md) for full usage and
behaviour notes.

## License

AGPL-3.0-only OR LicenseRef-Imazen-Commercial.
