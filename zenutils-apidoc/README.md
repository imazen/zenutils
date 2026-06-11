# zenutils-apidoc

Public-API snapshot tests for whole workspaces: one `cargo test` regenerates
committed `docs/public-api/` surface docs for every publishable library
crate, so API changes always show up as a git diff next to the code change
that caused them — and the surface size stays one glance away.

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
    .exclude_features("zenfilters", ["experimental"]) // documented, not headlined
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

## Snapshot format: three disjoint files per crate

- **`<crate>.txt`** — the supported surface: default features, hidden items
  excluded. What a consumer who types `cargo add <crate>` gets.
- **`<crate>.features.txt`** — additions from non-excluded, non-`_*`
  features (delta vs the default surface).
- **`<crate>.internal.txt`** — `#[doc(hidden)]` items plus the surface of
  EXCLUDED features (`_*`-prefixed, or named via `exclude_features` —
  exclusion without the semver break of renaming a feature).

No line appears in more than one file. Within each file:

- a generated `## summary` taxonomy (free functions vs inherent methods vs
  fields/variants, plus a per-module table) keeps the headline honest;
- the crate-name path prefix is stripped from every line;
- **trait impls collapse to one roster line per type**
  (`Type: Clone, Debug, Display, Error`) — method signatures live at the
  trait definition, so per-impl bodies are dropped; inherent methods stay;
- **auto traits** collapse to a count of fully-`Send`/`Sync`/… types plus
  explicit `Type: !Send !Sync` exception lines — a type losing `Send` moves
  into the exceptions list, so the semver diff guard survives with ~95%
  fewer lines (conditional `where`-bearing impls are preserved verbatim);
- blanket impls are omitted (compiler-guaranteed, zero semver signal);
- re-export duplicates are annotated `[also: other::path]` instead of
  listed twice.

## License

MIT OR Apache-2.0.
