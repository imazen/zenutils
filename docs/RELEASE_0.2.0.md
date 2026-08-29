# zenutils-fuzz 0.2.0 — release checklist

Status: **prepared, not released.** The version bump, CHANGELOG, snapshots and
dry-run are done and pushed. Tagging, the GitHub release, and `cargo publish`
are the owner's to run — nothing in this document has been executed against
crates.io or the GitHub releases API.

Scope: **`zenutils-fuzz` only.** `zenutils-apidoc` sits at 0.1.2 in-tree with
only 0.1.1 published; that is a separate pending release and must not be swept
into this one. The two crates have no dependency on each other, so
`cargo publish -p zenutils-fuzz` is a clean single-crate publish.

## What is in 0.2.0

A seed expectation is now mandatory. `RegressionSuite::run` refuses to proceed
until the caller declares `.min_seeds(n)` or `.allow_empty_corpus()`, an
unreadable seed directory panics under every expectation, and `run()` returns a
`RegressionReport` carrying the replayed count. Full entries, with commit
references, in `CHANGELOG.md` under `## zenutils-fuzz` -> `### [0.2.0]`.

The API change is one commit, `038c201`. `5f1933f` added the mutation harness
(`scripts/mutate-fuzz-guards.py`) that verifies the new guards are not no-ops;
it is workspace tooling and ships in no crate.

## Semver: the tool says nothing, the break is real

```
cargo semver-checks --package zenutils-fuzz \
  --baseline-root <extracted zenutils-fuzz-0.1.0.crate> --release-type patch
```

cargo-semver-checks 0.49.0: **223 checks, 223 pass, 30 skip — "no semver update
required".** Exit 0. That is a false negative in both of its halves:

1. **The behavioural breaks are outside what the tool models.** A call that used
   to return now panics. cargo-semver-checks compares type signatures, not
   behaviour, so the mandatory-expectation change and the unreadable-directory
   change are structurally invisible to it. These are the reason for the
   release and no version of the tool will ever flag them.

2. **The return-type break falls in a real lint-coverage gap.** 0.49.0 ships
   `exported_function_return_value_added` (free functions),
   `trait_method_return_value_added` (unsealed trait methods) and
   `pub_api_sealed_trait_method_return_value_added` — all classified major — but
   there is no `inherent_method_return_value_added`. `RegressionSuite::run` is
   an inherent method going `()` -> `RegressionReport`: exactly the uncovered
   case. (The opposite direction, `inherent_method_now_returns_unit`, *is*
   covered, which is what makes this look like an oversight rather than a
   judgement that the change is compatible.)

Verified by compiling one two-function consumer against the published 0.1.0
`.crate` and against this tree:

| consumer form | against 0.1.0 | against 0.2.0 |
|---|---|---|
| `suite.run();` (statement) | compiles | compiles |
| `fn f(..) { suite.run() }` (tail expr, `-> ()`) | compiles | `error[E0308]: expected (), found RegressionReport` |

So the statement form — which is what every consumer in the workspace actually
writes — survives, and the `#[must_use]`-was-deliberately-omitted claim holds.
The tail-expression form breaks. Three real breaking changes, zero detected.

**Do not read the green semver-checks run as licence to ship this as 0.1.1.**

### Reproducing the semver check

The obvious invocation gives a misleading answer twice over, so use the one
above rather than improvising:

- `--baseline-version 0.1.0` **while the crate is still at 0.1.0** resolves the
  baseline to the *local* source, not the registry, and reports "no change".
  The poisoned artifact is cached under
  `target/semver-checks/local-zenutils_fuzz-0_1_0-*` and is reused on later
  runs, so delete it if you have ever run it that way.
- `--baseline-version 0.1.0` **after** the bump reports
  `0.1.0 -> 0.2.0 (major change)` and then skips all 253 lints, because nothing
  can require more than a major bump. `--release-type patch` is what forces
  them to run.
- `--baseline-root <extracted .crate>` removes the last doubt about which
  bytes the baseline actually is.

## Gates — all green on this tree

| gate | command | result |
|---|---|---|
| fmt | `cargo fmt --all -- --check` | pass |
| tests | `cargo test --workspace --all-targets` | pass (15 zenutils-fuzz + 12 + 1 apidoc) |
| doctests | `cargo test --workspace --doc` | pass (1 + 3) |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| API snapshots | `ZEN_API_DOC=regen cargo test -p zenutils-apidoc --test public_api_doc` | pass, no diff (already current as of 038c201) |
| packaging | `cargo publish --dry-run -p zenutils-fuzz` | pass — 6 files, 36.5 KiB (10.0 KiB compressed) |
| guard mutation | `python3 scripts/mutate-fuzz-guards.py` | pass — 9/9 mutations CAUGHT, baseline green before and after, source restored |

The mutation gate is the one that matters most for this particular release. The
whole point of 0.2.0 is refusing to pass vacuously, so a guard that rotted into
a no-op would be the exact failure the release exists to prevent. Reverting each
guard to its 0.1.0 permissive behaviour one at a time turns at least one test
red every time (1 to 8 tests per mutation), so every new guard is genuinely
pinned rather than merely present.

Packaged file list is exactly `.cargo_vcs_info.json`, `Cargo.lock`,
`Cargo.toml`, `Cargo.toml.orig`, `README.md`, `src/lib.rs`. No session files, no
snapshot docs, no tests. Re-verified on a pristine `git archive origin/main`
export so the result does not depend on `--allow-dirty`.

**CI is green on the release commit.** Run
[33258142056](https://github.com/imazen/zenutils/actions/runs/33258142056) on
`27d5d21`: 12/12 jobs success — ubuntu-latest, ubuntu-24.04-arm, macos-latest,
macos-26-intel, windows-latest, windows-11-arm, i686 (`cross`), WASM check,
Clippy, Format, MSRV, Public API snapshots. Step 2 of the sequence below is
therefore already satisfied for this commit; re-check if anything lands on
`main` before you tag.

## Release sequence

Run in this order. **If any step fails, stop — do not proceed to the next.**

1. **Confirm the gates locally.** `cargo test --workspace --all-targets` and
   `cargo test --workspace --doc` must pass on the commit you are about to
   release. A publish against failing tests is never acceptable.

2. **Push the release commit and wait for CI to go green on every platform.**
   `gh run list --repo imazen/zenutils`. The matrix is ubuntu-latest,
   ubuntu-24.04-arm, macos-latest, macos-26-intel, windows-latest,
   windows-11-arm, plus the i686 (`cross`), wasm32-wasip1, clippy, fmt, MSRV and
   public-API-snapshot jobs. All of them, not just the fast ones.

   CI on `main` was red from 2026-06-25 until `a632731` fixed it earlier today.
   Do not tag against a red run and do not tag ahead of CI — a tag pointing at a
   broken commit has to be force-deleted.

3. **Tag.** `git tag zenutils-fuzz-v0.2.0 && git push origin zenutils-fuzz-v0.2.0`

   Note the tag name. This workspace already uses the crate-prefixed form for
   `zenutils-apidoc-v0.1.0` / `zenutils-apidoc-v0.1.1`; the bare `v0.1.0` tag is
   a legacy from when zenutils-fuzz was the only crate in the repo. Prefixing
   keeps the two crates' release histories separable from here on. If you would
   rather stay bit-compatible with the old scheme, `v0.2.0` also works — but
   pick one deliberately, because the next apidoc release will sit next to it.

4. **GitHub release.**
   `gh release create zenutils-fuzz-v0.2.0 --repo imazen/zenutils --title "zenutils-fuzz v0.2.0" --generate-notes`

   A tag alone is not sufficient — every published version needs a release page.
   Consider replacing the generated notes with the `### [0.2.0]` CHANGELOG body,
   since `--generate-notes` will pull in the apidoc commits too.

5. **Publish.** `cargo publish -p zenutils-fuzz`

6. **After publishing**, change `### [0.2.0] - unreleased` in `CHANGELOG.md` to
   the real release date and commit that.

## Consumers — nothing auto-upgrades, and nothing breaks until it is bumped

`zenutils-fuzz` is a **`[dev-dependency]` everywhere it is used** — it reaches
no published artifact and no runtime code path. And `0.1.0 -> 0.2.0` is
semver-incompatible to Cargo for a 0.x crate: a `zenutils-fuzz = "0.1.0"`
requirement will **not** resolve to 0.2.0. So publishing cannot break anyone.
Each repo moves when it deliberately edits its manifest.

Verified on this checkout (2026-08-29) — 9 manifests across 8 repos, every one
`[dev-dependencies]`, every one pinned `"0.1.0"`:

| repo | manifest:line | seeds in corpus | needs on bump |
|---|---|---|---|
| image-tiff | `Cargo.toml:62` | 5 (`fuzz/regression`) | `.min_seeds(5)` |
| ultrahdr | `ultrahdr-rs/Cargo.toml:62` | 2 (`../fuzz/regression`) | `.min_seeds(2)` |
| zenavif | `Cargo.toml:245` | 1 (`fuzz/regression`) | already on the new shape via a local stand-in; the declared dep is currently unused (`use regress::RegressionSuite;`) |
| zenavif | `zenavif-parse/Cargo.toml:39` | 1 (`../fuzz/regression`) | `.min_seeds(1)` |
| zenbitmaps | `Cargo.toml:41` | 5 | `.min_seeds(5)` |
| zenflate | `Cargo.toml:62` | **0** | `.min_seeds(0)` — deliberately empty, ~19 MB repro gated by a unit test |
| zengif | `Cargo.toml:101` | 2 | `.min_seeds(2)` |
| zenjxl-decoder | `zenjxl-decoder/Cargo.toml:38` | 21 (`../fuzz/regression`) | `.min_seeds(21)` — this is the WASI `..` case the release exists for |
| zenraw | `Cargo.toml:38` | 1 | `.min_seeds(1)` |

Seed counts are measured on this checkout with the crate's own filter
(recursive, dotfiles and `*.md` / `*.txt` excluded). Re-check in each repo
before pinning a number — a corpus can be gitignored or live in block storage.

**Eight of the nine currently call `.run()` with no expectation declared** — all
but zenavif, which is the exception only because it is already driving its local
stand-in. That compiles fine after the bump; it panics at test time with a
message naming both `.min_seeds` and `.allow_empty_corpus`. That is the intended
migration prompt — loud, immediate, and impossible to mistake for a passing run
— but it does mean a bump without the added line turns the consumer's suite red.
Bump and declare in the same commit.

`zen/zenavif--encode-rd` carries two more declarations
(`Cargo.toml:81`, `zenavif-parse/Cargo.toml:39`) but it is a stale sibling
workspace of zenavif, not a separate repo. Not a migration target.

### Three crates are already waiting on this release

`zenavif`, `heic` and `zenextras/zentiff` each carry a hand-written local
`regress` module that reimplements the 0.2.0 builder shape, with a module
comment saying the copy exists only because the API is unpublished and that
migration is "delete the module, add the dependency, `use
zenutils_fuzz::RegressionSuite;`, leave the chain untouched":

- `zen/zenavif/tests/fuzz_regression.rs:19-24` (already calls
  `.min_seeds(TRACKED_SEEDS)` against the local stand-in)
- `zen/heic/tests/fuzz_regression.rs:18-22` — 35 seeds, no dependency declared yet
- `zen/zenextras/zentiff/tests/fuzz_regression.rs:27-31` — 4 seeds, no dependency declared yet

Publishing 0.2.0 lets all three delete their copies. Counting those, ten repos
consume or are waiting to consume this crate.
