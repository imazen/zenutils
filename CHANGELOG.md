# Changelog

All notable changes to crates in this workspace are documented here, following
[Keep a Changelog](https://keepachangelog.com/).

## zenutils-apidoc

### [Unreleased]

### [0.1.1] - 2026-06-11

#### Added
- `ApiDoc::base_features(crate, csv)` — baseline features for the
  supported-surface build, for crates whose plain default features do not
  compile (e.g. heic's backend-selection `compile_error!` gate). The
  snapshot header records the baseline; the features file stays a delta
  vs it.

### [0.1.0] - 2026-06-11

#### Added
- Initial `zenutils-apidoc` crate: workspace-wide public-API snapshot tests —
  the shared implementation of the `public_api_doc.rs` test that previously
  lived as 41 drifting per-repo copies. One `cargo test` regenerates committed
  `docs/public-api/` docs. Auto-discovers publishable library members;
  `ApiDoc` builder covers pinned feature combos (`pinned_features`),
  default-only crates (`no_extra_section`), and feature exclusion without
  renaming (`exclude_features`). Built on `public-api` + `rustdoc-json` +
  `rustup-toolchain` (no `cargo-public-api` binary needed);
  `ZEN_API_DOC=off|check|regen` protocol kept byte-compatible with existing
  CI, plus unset-under-`GITHUB_ACTIONS` → skip. Toolchain defaults to
  tracking `nightly` (`ZEN_API_DOC_TOOLCHAIN` overrides) because
  `public_api::MINIMUM_NIGHTLY_RUST_VERSION` 0.52.1 lags its own
  `rustdoc-types` 0.57.3 requirement (emits unparsable format-55 JSON).
  (0589e923)
- Format v3 — three disjoint files per crate: `<crate>.txt` supported
  surface (default features, hidden excluded), `<crate>.features.txt`
  non-excluded feature additions, `<crate>.internal.txt` `doc(hidden)` +
  excluded-feature surface. Trait impls collapse to one roster line per
  type (method bodies dropped — signatures live at the trait definition);
  auto traits collapse to a complete-types count + explicit `!Trait`
  exceptions (conditional impls verbatim); blanket impls omitted;
  re-export duplicates annotated `[also: path]`; crate-name prefix
  stripped. Hidden items come from a directly-spawned `cargo rustdoc
  --document-hidden-items`; unbuildable excluded/hidden builds degrade to
  a NOTE line. First catch: zensim-regress's `doc(hidden) pub mod layout`,
  1,276 raw lines of previously-invisible hidden API. (094f6cd0)
- `ApiDoc::workspace_dir` — targets a parent workspace, enabling the
  recommended **CI-free runner package** integration: a workspace-excluded
  `apidoc/` package holds the only dependency on this crate, so consumer
  CI (including `--all-features` jobs) never compiles the apidoc tree and
  never runs rustdoc; regeneration is `cargo test --manifest-path
  apidoc/Cargo.toml` from a justfile.
- Packaging-invariant check (on by default): every snapshotted crate's
  `cargo package --list` must be free of snapshot docs/tests and
  repo-local session files (CLAUDE.md, .workongoing, …) — the org's
  packaging audits, made self-enforcing. `skip_packaging_check(crate)`
  opts out; `forbid_in_package([...])` extends the pattern set.
- `ApiDoc::attribute_features(crate)` — opt-in per-feature attribution:
  one `## added by feature: X` section per feature (one extra rustdoc
  build each) plus a `feature interactions` section for lines that only
  appear when features combine.

## zenutils-fuzz

### [Unreleased]

#### Added
- Initial `zenutils-fuzz` crate: a fuzz-regression seed-corpus runner
  (`RegressionSuite`) moved from the un-versioned `zen-fuzz-regress` helper.
  Walks `fuzz/regression/*` and feeds every seed through every registered
  fuzz-target entry point; a panic surfaces seed path + target name. Ships
  with 6 unit tests covering no-op/empty/recursion/meta-skip/panic paths.
