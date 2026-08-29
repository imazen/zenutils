# Changelog

All notable changes to crates in this workspace are documented here, following
[Keep a Changelog](https://keepachangelog.com/).

## Workspace

### [Unreleased]

#### Fixed
- **Pushes to `main` now cancel their superseded CI runs.** `ci.yml` keyed its
  concurrency group on `${{ github.head_ref || github.run_id }}`.
  `github.head_ref` is populated only for `pull_request` events, so on a push it
  was empty and the group fell through to `github.run_id` — unique per run, so no
  two pushes ever shared a group and `cancel-in-progress` could never fire. Every
  push started a full matrix that ran to completion even when several commits
  landed seconds apart. Now keyed on `${{ github.ref }}`, which is set for both
  event types (`refs/heads/main` on push, `refs/pull/N/merge` on a PR), so PR
  cancellation is unchanged and consecutive pushes supersede each other.
- CI on `main` had been red since 2026-06-25 on three jobs, all pre-existing and
  unrelated to each other: **Format** (`zenutils-apidoc/src/lib.rs` was not
  `cargo fmt`-clean in two test bodies), **Clippy** (`snapshot_one` took 8
  arguments against clippy's limit of 7 — fixed by bundling the five
  per-package inputs into a private `PackageSnapshotSpec`, not by an `allow`),
  and **Public API snapshots** (`docs/public-api/zenutils-apidoc.txt` never got
  regenerated after `no_file_meta_header` / `no_autotraits_summary` landed in
  8a5a290). (a632731)

#### Added
- `docs/readme-conventions.md` — single source of truth for zen* README/onboarding
  conventions: split README.md (GitHub, full badges) + generated README.crates.md
  (crates.io, CI badge only), the crosslink-footer standard, the one-shot
  onboarding-function convention, and the fair-benchmark repro/chart standard.
  (f1114bd, 6350138, 82a050b, 1921bc2)
- `docs/zen-crates.tsv` — canonical zen* family registry (name/group/repo/one-liner)
  driving the crosslink footer. (f1114bd)
- `scripts/mutate-fuzz-guards.py` — mutation-verifies the `zenutils-fuzz` seed
  expectation guards. Reverts each guard, one at a time, to the permissive
  behaviour it replaced, runs the suite, and reports which tests noticed; a
  mutation nothing catches is an unprotected guard. `RegressionSuite` exists to
  refuse vacuous passes, so its own guards silently rotting into no-ops is the
  exact failure it is supposed to prevent. Currently 9 mutations, all caught.
  (5f1933f)
- `scripts/render-crosslink-footer.sh`, `scripts/gen-readme-crates.sh`,
  `scripts/splice-footer.sh` — render the footer from the registry, generate the
  trimmed crates.io README from README.md, and splice footers in place. The footer
  is the rich "Image tech I maintain" table (grouped image crates + Imageflow /
  ImageResizer products + a tools line + profile links), self-aware via `--self`.
  (f1114bd, 6350138, 1921bc2)

## zenutils-apidoc

### [Unreleased]

#### Added
- `ApiDoc::no_file_meta_header()` and `ApiDoc::no_autotraits_summary()` —
  two opt-in builder gates that suppress lines in the rendered snapshots
  which churn on every regen without carrying semver signal. The first
  drops the `# files: <crate>.txt N lines | <crate>.features.txt N added
  | <crate>.internal.txt N lines (X hidden + Y excluded-feature)` header
  block (its line-count counters shift every time the API surface grows
  by a few lines). The second drops the `X types implement all of:
  Freeze, RefUnwindSafe, Send, Sync, Unpin, UnwindSafe` summary line at
  the top of the `## auto traits` block (its counter changes whenever any
  type is added or removed); explicit `Type: !Trait …` exception lines
  stay — those are the actual semver signal. Both default OFF, so
  existing consumers see byte-identical snapshots; use is opt-in via
  `ApiDoc::new().no_file_meta_header().no_autotraits_summary().run()`.

#### Docs
- README now states three previously-implied, load-bearing behaviors found in
  an insulated external-developer usability test: (1) snapshots are written to
  `docs/public-api/` under the *target workspace's* root (cargo metadata
  `workspace_root`), not relative to the runner package's cwd; (2) `.run()`
  without `.crates([...])` auto-discovers every publishable library member
  (the minimal example is complete, not a stub); (3) the CI gate needs
  `ZEN_API_DOC=check` exported explicitly — unset under `GITHUB_ACTIONS` is a
  silent skip, so a check job that forgets the env var passes without checking.

### [0.1.1] - 2026-06-11

#### Added
- `ApiDoc::base_features(crate, csv)` — baseline features for the
  supported-surface build, for crates whose plain default features do not
  compile (e.g. heic's backend-selection `compile_error!` gate). The
  snapshot header records the baseline; the features file stays a delta
  vs it.

#### Fixed
- All cargo invocations now pass `name@version` package-id specs (resolved
  from the workspace metadata), so crates whose own registry-published
  version is also in the resolve graph — e.g. a dev-dependency depending on
  the published release of the crate being documented — no longer fail with
  "specification is ambiguous" (zenquant via zengif was the motivating
  case). rustdoc JSON filenames now come from the lib target name, honoring
  `[lib] name` overrides, and the target dir from cargo metadata.

#### Changed
- Dropped the `rustdoc-json` dependency: every build in the matrix now goes
  through the same directly-spawned `cargo rustdoc` path the hidden-items
  build already used (verified byte-identical snapshots on this workspace).

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

Nothing yet — 0.2.0 below is prepared but not published.

### [0.2.0] - unreleased

Breaking release. `0.1` -> `0.2` is the semver-incompatible component for a 0.x
crate, approved 2026-08-29 for the mandatory-seed-expectation change below.

Publishing this cannot break anyone. The crate is a `[dev-dependency]`
everywhere it is used, so it reaches no published artifact and no runtime path;
and a `zenutils-fuzz = "0.1.0"` requirement does not resolve to 0.2.0, because
Cargo treats a 0.x minor bump as incompatible. Each repo moves only when it
deliberately edits its manifest — and must add a `.min_seeds(n)` or
`.allow_empty_corpus()` call in the same commit, or its regression suite panics.
Measured on the 2026-08-29 checkout: 9 manifests across 8 repos declare it, 8 of
the 9 declare no expectation yet, and 3 more crates (zenavif, heic,
zenextras/zentiff) are carrying hand-written stand-ins waiting on this release.
Release checklist and the per-consumer table: `docs/RELEASE_0.2.0.md`.

#### Breaking
- `RegressionSuite::run` no longer treats an undeclared seed expectation as a
  silent pass. Every suite must now declare either `.min_seeds(n)` or
  `.allow_empty_corpus()`; a suite that declares neither panics with a message
  naming both. The old behaviour — replay whatever is there, no-op if there is
  nothing — is still available, as `.allow_empty_corpus()`; it is simply no
  longer what you get by saying nothing. Rationale: a missing corpus and a
  clean run were indistinguishable, and every observed instance of the former
  reported green (zenjxl-decoder's wasm legs replayed zero seeds for the whole
  life of its harness, because WASI refuses to traverse the literal `..` in
  its seed path). (`RegressionReport` docs carry the full case.) (038c201)
- `RegressionSuite::run` returns `RegressionReport` instead of `()`.
  Deliberately NOT `#[must_use]`, so existing `suite.run();` statements keep
  compiling unchanged under `-D warnings`. A tail-position call in a `-> ()`
  function (`fn f() { suite.run() }`, no semicolon) does break, with E0308.
  (038c201)
- A seed directory that exists but cannot be scanned (unreadable, or a path
  that is not a directory) is now a panic under every expectation, including
  `.allow_empty_corpus()`. It was previously indistinguishable from an empty
  corpus. This is what makes the WASI `..` case fail loudly instead of green.
  (038c201)

#### Semver-checks result

`cargo semver-checks --package zenutils-fuzz --baseline-root <extracted
zenutils-fuzz-0.1.0.crate> --release-type patch` (v0.49.0) reports **223 checks,
223 pass, 30 skip — "no semver update required"**. That is a false negative, and
the bump is justified anyway. Two reasons, both verified rather than argued:

- **The behavioural breaks are invisible to the tool.** A call that used to
  return now panics; cargo-semver-checks compares type signatures, not
  behaviour, so nothing in its lint set can see the mandatory-expectation
  change or the unreadable-directory change. These are the *reason* for the
  release and they will never show up in a semver-checks run.
- **The return-type break falls in a genuine lint-coverage gap.**
  cargo-semver-checks 0.49.0 ships `exported_function_return_value_added`
  (free functions), `trait_method_return_value_added` (unsealed trait methods)
  and `pub_api_sealed_trait_method_return_value_added` — all major — but has no
  `inherent_method_return_value_added`. `RegressionSuite::run` is an inherent
  method going `()` -> `RegressionReport`, which is exactly the uncovered case.
  (The reverse direction, `inherent_method_now_returns_unit`, *is* covered.)
  Confirmed by compiling the same two-function consumer against the published
  0.1.0 and against this tree: `suite.run();` in statement position builds on
  both, and `fn f(..) { suite.run() }` in tail position builds on 0.1.0 and
  fails on 0.2.0 with `error[E0308]: expected (), found RegressionReport`.

So the honest summary is: **zero tool-detected breaks, three real ones.** Do not
read the green semver-checks run as a licence to ship this as 0.1.1.

#### Added
- `RegressionSuite::min_seeds(n)` — the seed directory must exist, be readable,
  and yield at least `n` replayed seeds; a missing, unreadable, empty, or short
  corpus fails with a message saying **which** of those it was, since the four
  have different causes and different fixes. `n` counts what the suite actually
  replays (dotfiles, `*.md` and `*.txt` are already filtered out), so a
  `README.md` in the corpus directory cannot inflate it — the mistake most
  hand-rolled seed counters in the workspace make. (038c201)
- `RegressionSuite::allow_empty_corpus()` — explicit opt-in to the permissive
  behaviour, for a crate that genuinely has no corpus yet. Tolerates an absent
  or empty directory; still fails on an unreadable one. (038c201)
- `RegressionSuite::min_seeds(0)` — distinct from `allow_empty_corpus()`: the
  directory must exist, but may hold no seeds. For a deliberately-empty corpus
  (zenflate's one open repro needs a ~19 MB input, gated by a unit test
  instead) whose directory must not silently disappear. (038c201)
- `RegressionReport`, returned by `run()`: `seeds_replayed()`, `targets()`,
  `invocations()`, `seed_paths()`, `seed_dir()`, plus `Display` and `Debug`.
  Callers no longer have to re-implement the seed filter to count the corpus —
  twelve repos had grown a hand-rolled counter, eight of them a byte-identical
  copy of the same 25-line helper. (038c201)
- 9 further unit tests (15 total) covering the undeclared/absent/unreadable/
  empty/short cases, report counts, expectation ordering, and the
  `README.md`-counted-as-a-seed trap. (038c201)

#### Docs
- README gained the seed-expectation table (five corpus states x four
  expectations), the zenjxl-decoder WASI case that motivated the change, and
  the `min_seeds(0)` vs `allow_empty_corpus()` distinction. The old one-line
  "a missing or empty seed dir is a no-op" is gone — it described behaviour
  that no longer exists. (038c201)

### [0.1.0] - 2026-05-27

#### Added
- Initial `zenutils-fuzz` crate: a fuzz-regression seed-corpus runner
  (`RegressionSuite`) moved from the un-versioned `zen-fuzz-regress` helper
  (2712fe2). Walks `fuzz/regression/*` and feeds every seed through every
  registered fuzz-target entry point; a panic surfaces seed path + target name.
  Ships with 6 unit tests covering no-op/empty/recursion/meta-skip/panic paths.
