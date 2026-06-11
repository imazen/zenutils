# Changelog

All notable changes to crates in this workspace are documented here, following
[Keep a Changelog](https://keepachangelog.com/).

## zenutils-apidoc

### [Unreleased]

#### Added
- Initial `zenutils-apidoc` crate: workspace-wide public-API snapshot tests —
  the shared implementation of the `public_api_doc.rs` test that previously
  lived as 41 drifting per-repo copies. One `cargo test` regenerates committed
  `docs/public-api/<crate>.txt` docs with a generated taxonomy `## summary`
  (free fns vs methods vs impl plumbing, per-module table) and a delta-only
  non-default-features section. Auto-discovers publishable library members;
  `ApiDoc` builder covers pinned feature combos (`pinned_features`) and
  default-only crates (`no_extra_section`). Built on `public-api` +
  `rustdoc-json` + `rustup-toolchain` (no `cargo-public-api` binary needed);
  `ZEN_API_DOC=off|check|regen` protocol kept byte-compatible with existing
  CI, plus unset-under-`GITHUB_ACTIONS` → skip. Toolchain defaults to
  tracking `nightly` (`ZEN_API_DOC_TOOLCHAIN` overrides) because
  `public_api::MINIMUM_NIGHTLY_RUST_VERSION` 0.52.1 lags its own
  `rustdoc-types` 0.57.3 requirement (emits unparsable format-55 JSON).

## zenutils-fuzz

### [Unreleased]

#### Added
- Initial `zenutils-fuzz` crate: a fuzz-regression seed-corpus runner
  (`RegressionSuite`) moved from the un-versioned `zen-fuzz-regress` helper.
  Walks `fuzz/regression/*` and feeds every seed through every registered
  fuzz-target entry point; a panic surfaces seed path + target name. Ships
  with 6 unit tests covering no-op/empty/recursion/meta-skip/panic paths.
