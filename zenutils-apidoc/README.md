# zenutils-apidoc

Public-API snapshot tests for whole workspaces: one `cargo test` regenerates
committed `docs/public-api/` surface docs for every publishable library
crate, so API changes always show up as a git diff next to the code change
that caused them — and the surface size stays one glance away.

Built on [`public-api`](https://lib.rs/crates/public-api) — the
well-maintained library behind `cargo public-api` — plus
[`rustup-toolchain`](https://lib.rs/crates/rustup-toolchain) for automatic
nightly install (rustdoc JSON requires nightly; the tracking `nightly`
toolchain is used by default, `ZEN_API_DOC_TOOLCHAIN` pins a specific one).
This crate adds the rustdoc JSON builds (spawned `cargo rustdoc` with
disambiguated `name@version` package specs), the workspace-wide
orchestration, and the snapshot format shared across zen repos.

## Usage: the CI-free runner package

Hold the dependency in a tiny `publish = false` package at `apidoc/` that
your workspace `exclude`s — plain `cargo test` and every CI job (including
`--all-features` ones) then never compile this crate's dependency tree and
never run rustdoc. Regeneration is a justfile recipe.

```toml
# apidoc/Cargo.toml
[package]
name = "my-workspace-apidoc"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
zenutils-apidoc = "0.1.0"
```

```rust
// apidoc/tests/public_api_doc.rs — the whole file, for most workspaces:
#[test]
fn public_api_surface_docs_are_current() {
    zenutils_apidoc::ApiDoc::new()
        .workspace_dir("..") // the real workspace, relative to this package
        .run(); // no .crates(...) → auto-discover every publishable library member
}
```

Calling `.run()` without `.crates([...])` **auto-discovers** every workspace
member that is publishable (its `publish` field is not `false`/`[]`) and has a
library target (`lib`/`rlib`/`proc-macro` — bin-only and cdylib-only members
are skipped; list those explicitly). It does not snapshot nothing: the minimal
example above is the whole config for most workspaces. If discovery finds no
publishable library, `.run()` panics telling you to pass an explicit list.

```text
# the real workspace Cargo.toml
[workspace]
exclude = ["apidoc"]
```

```just
api-doc:
    cargo test --manifest-path apidoc/Cargo.toml

fmt:
    cargo fmt --all
    cargo test --manifest-path apidoc/Cargo.toml
```

Workspaces that need control:

```rust
zenutils_apidoc::ApiDoc::new()
    .workspace_dir("..")
    .crates(["zenpipe", "zencodecs", "zenfilters"])
    .no_extra_section("zenpipe")                 // --all-features doesn't build
    .pinned_features("zencodecs", "jxl-encode,cms")
    .exclude_features("zenfilters", ["experimental"]) // documented, not headlined
    .run();
```

## Modes (`ZEN_API_DOC` env var)

| value | behavior |
|---|---|
| unset, locally | regenerate the files in place; commit the diff |
| unset, under `GITHUB_ACTIONS` | **skip (silent no-op)** — see the CI gate below |
| `check` | regenerate to memory, FAIL if a committed file is stale |
| `regen` | regenerate in place (same as unset-locally; forces regen even under CI) |
| `off` | skip |
| anything else | panic (`unknown ZEN_API_DOC value`) |

### The CI gate: `ZEN_API_DOC=check` must be set explicitly

Detecting `GITHUB_ACTIONS` only flips the *unset* default from regenerate to
skip — because a regen on CI is write-only noise, and reusable CI workflows
can't always pass env vars to a job. It does **not** turn on checking. To make
CI actually fail on a stale snapshot you must **explicitly export
`ZEN_API_DOC=check`** in a dedicated job:

```yaml
- run: cargo test --manifest-path apidoc/Cargo.toml
  env:
    ZEN_API_DOC: check
```

The footgun: a CI job that runs the test **without** setting `ZEN_API_DOC`
silently passes (the var is unset → skip under `GITHUB_ACTIONS`) without
checking anything. If your "api-doc check" job is green but never seems to
catch drift, confirm the `check` value is being exported — an unset or empty
var is a no-op on CI, not a check. (`off` also skips; only `check` gates.)

## Snapshot location: relative to the workspace root, not the runner package

The snapshots are written to `docs/public-api/` **under the target workspace's
root** — the directory `cargo metadata --manifest-path <workspace_dir>/Cargo.toml`
reports as `workspace_root` — **not** relative to the runner package's current
directory. So with `.workspace_dir("..")` from an `apidoc/` runner package, the
files land in `<real-workspace>/docs/public-api/`, which is exactly where you
want them committed (next to the code). `.out_dir("some/dir")` overrides the
`docs/public-api` subdirectory; it is still joined onto the workspace root. Run
the regen recipe from any cwd — the output path follows the workspace, not the
shell.

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
