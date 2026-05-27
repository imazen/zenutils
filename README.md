# zenutils

Small, focused utility crates shared across the zen image-processing codebase.
A Cargo workspace; new shared helpers are added as `zenutils-*` members so every
consumer pulls them from one versioned source rather than ad-hoc path deps.

| Crate | Description |
|-------|-------------|
| [`zenutils-fuzz`](zenutils-fuzz/) | Fuzz-regression seed-corpus runner (`RegressionSuite`). |

## License

MIT OR Apache-2.0.
