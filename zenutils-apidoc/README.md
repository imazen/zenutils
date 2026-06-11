# zenutils-apidoc

Public-API snapshot tests for whole workspaces: one `cargo test` regenerates
committed `docs/public-api/<crate>.txt` surface docs for every publishable
library crate, so API changes always show up as a git diff next to the code
change that caused them.

Built on [`public-api`](https://lib.rs/crates/public-api) and
[`rustdoc-json`](https://lib.rs/crates/rustdoc-json) — the well-maintained
libraries behind `cargo public-api` — plus
[`rustup-toolchain`](https://lib.rs/crates/rustup-toolchain) for automatic
nightly install (rustdoc JSON requires nightly; the tracking `nightly`
toolchain is used by default, `ZEN_API_DOC_TOOLCHAIN` pins a specific one).
This crate adds the workspace-wide orchestration and the snapshot format
shared across zen repos.

## Usage

```toml
[dev-dependencies]
zenutils-apidoc = "0.1.0"
```

```rust
// tests/public_api_doc.rs — the whole file, for most workspaces:
#[test]
fn public_api_surface_docs_are_current() {
    zenutils_apidoc::run();
}
```

Workspaces that need control:

```rust
zenutils_apidoc::ApiDoc::new()
    .crates(["zenpipe", "zencodecs", "zenfilters"])
    .no_extra_section("zenpipe")                 // --all-features doesn't build
    .pinned_features("zencodecs", "jxl-encode,cms")
    .run();
```

## Modes (`ZEN_API_DOC` env var)

| value | behavior |
|---|---|
| unset (local) | regenerate in place; commit the diff |
| unset (under `GITHUB_ACTIONS`) | skip — reusable CI workflows can't always pass env vars |
| `check` | regenerate to memory, FAIL if a committed file is stale (the CI gate job) |
| `regen` | force regenerate |
| `off` | skip |

## Snapshot format

Each `<crate>.txt` carries a generated `## summary` taxonomy (free functions
vs methods vs fields/variants vs auto-trait/derived impl lines, plus a
per-module table), the full default-features surface, and a **delta-only**
non-default-features section. Auto-trait impl lines are kept on purpose:
losing `Send`/`Sync` on a public type is a semver break and must show in the
diff. Blanket impls are omitted.

## License

MIT OR Apache-2.0.
