//! Public-API snapshot tests for whole workspaces.
//!
//! Regenerates committed public-API surface snapshots (three disjoint files
//! per crate under `<workspace>/docs/public-api/`) from a single
//! `cargo test`, so API changes always show up as a git diff next to the
//! code change that caused them. This is the shared implementation of the
//! `public_api_doc.rs` test that previously lived as a drifting copy in
//! every zen repo.
//!
//! # Integration: the CI-free runner package (recommended)
//!
//! Consumer workspaces hold this dependency in a tiny `publish = false`
//! package at `apidoc/` that the real workspace `exclude`s, so plain
//! `cargo test` — and every CI job, including `--all-features` ones —
//! never compiles this crate's dependency tree and never runs rustdoc.
//! Regeneration is a justfile recipe
//! (`cargo test --manifest-path apidoc/Cargo.toml`), typically chained
//! into `just fmt`.
//!
//! ```no_run
//! // apidoc/tests/public_api_doc.rs — the whole file, for most workspaces:
//! #[test]
//! fn public_api_surface_docs_are_current() {
//!     zenutils_apidoc::ApiDoc::new()
//!         .workspace_dir("..") // the real workspace
//!         .run(); // auto-discovers its publishable library members
//! }
//! ```
//!
//! Workspaces that need control use the rest of the builder:
//!
//! ```no_run
//! #[test]
//! fn public_api_surface_docs_are_current() {
//!     zenutils_apidoc::ApiDoc::new()
//!         .workspace_dir("..")
//!         .crates(["zenpipe", "zencodecs", "zenfilters"])
//!         .no_extra_section("zenpipe") // --all-features does not build
//!         .pinned_features("zencodecs", "jxl-encode,cms")
//!         .exclude_features("zenfilters", ["experimental"])
//!         .run();
//! }
//! ```
//!
//! (An in-workspace `tests/public_api_doc.rs` calling [`run`] also works —
//! this repo dogfoods that — but it makes every `cargo test` compile this
//! crate's dependency tree, which consumer CI generally shouldn't pay.)
//!
//! # Modes (`ZEN_API_DOC` env var)
//!
//! - unset / `regen` → regenerate the files in place (local default; commit
//!   the diff). Under `GITHUB_ACTIONS` an unset var means **skip**, because
//!   reusable CI workflows can't always pass env vars and a regen on CI is
//!   write-only noise; the dedicated check job sets `ZEN_API_DOC=check`.
//! - `check` → regenerate to memory, FAIL if a committed file is stale.
//! - `off` → skipped (matrix jobs without nightly rustdoc).
//!
//! # Snapshot layout: three disjoint files per crate
//!
//! - **`<crate>.txt`** — the supported surface: default features, hidden
//!   items excluded. What a consumer who types `cargo add <crate>` gets.
//! - **`<crate>.features.txt`** — ADDITIONS from non-excluded, non-`_*`
//!   features (delta vs the default surface), hidden items excluded. A
//!   `removed by features` section appears only when enabling features
//!   removes surface (a `cfg(not(feature = ...))` gate).
//! - **`<crate>.internal.txt`** — `#[doc(hidden)]` items (from every build
//!   configuration) and the additions from EXCLUDED features (`_*`-prefixed
//!   plus any named via [`ApiDoc::exclude_features`] — exclusion without
//!   renaming, since renaming a feature is itself a semver break). Callable
//!   surface, documented here instead of cluttering the supported files.
//!
//! No line appears in more than one file.
//!
//! # Signal-over-noise encodings (within each file)
//!
//! - The crate-name path prefix is stripped from every line (it is in the
//!   header; signatures referencing the crate's own types shorten too).
//! - **Auto traits** (`Freeze`/`Send`/`Sync`/`Unpin`/`RefUnwindSafe`/
//!   `UnwindSafe`) collapse to one count line for types implementing all
//!   six, plus explicit `Type: !Send !Sync` exception lines. A type losing
//!   `Send` moves into the exceptions list — the semver diff guard survives
//!   with ~95% fewer lines. (`StructuralPartialEq` is omitted: it tracks
//!   the `PartialEq` derive, which the trait roster already records.
//!   Conditional auto impls — `where` clauses — are preserved verbatim.)
//! - **Trait impls** collapse to one roster line per type
//!   (`Type: Clone, Debug, Display, Error`); the per-impl method bodies are
//!   dropped because their signatures are fixed by the trait's own
//!   definition. `core`/`alloc`/`std` trait paths shorten to their last
//!   segment; other crates' traits keep their full path. Conditional
//!   (`where`-bearing) trait impls are preserved verbatim below the roster.
//! - Blanket impls are omitted entirely (compiler-guaranteed; zero semver
//!   signal).
//! - Re-export duplicates (the same item reachable at several paths with an
//!   identical signature) list the shortest path once with an
//!   `[also: other::path]` annotation.
//!
//! # Toolchain
//!
//! Building rustdoc JSON needs a nightly toolchain whose JSON format matches
//! the `public-api` parser this crate compiles against. The default is the
//! tracking `nightly` toolchain (auto-installed via `rustup` when missing),
//! which is also what `dtolnay/rust-toolchain@nightly` provisions on CI. Pin
//! a specific one with the `ZEN_API_DOC_TOOLCHAIN` env var if a nightly
//! regression ever requires it. (`public_api::MINIMUM_NIGHTLY_RUST_VERSION`
//! is deliberately not used: in public-api 0.52.1 it lags the crate's own
//! `rustdoc-types` requirement and produces unparsable format-55 JSON.)

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Toolchain used for rustdoc JSON: `ZEN_API_DOC_TOOLCHAIN` env override, or
/// the tracking `nightly`.
fn toolchain() -> String {
    std::env::var("ZEN_API_DOC_TOOLCHAIN").unwrap_or_else(|_| "nightly".to_owned())
}

/// How the extra (non-default-features) snapshot file is built for one
/// crate.
enum Extra {
    /// All manifest features except `default`, `_*`-prefixed, and
    /// explicitly excluded ones (default).
    PublicFeatures,
    /// No features file content — snapshot default features only.
    None,
    /// Pinned `--features` csv (for crates whose full feature powerset
    /// doesn't build).
    Pinned(String),
}

enum Mode {
    Off,
    Check,
    Regen,
}

fn mode() -> Mode {
    match std::env::var("ZEN_API_DOC").as_deref() {
        Ok("off") => Mode::Off,
        Ok("check") => Mode::Check,
        Ok("regen") => Mode::Regen,
        Ok(other) => panic!("unknown ZEN_API_DOC value {other:?} (off|check|regen)"),
        Err(_) if std::env::var_os("GITHUB_ACTIONS").is_some() => {
            eprintln!(
                "ZEN_API_DOC unset under GITHUB_ACTIONS — snapshot regen skipped \
                 (a dedicated api-doc job should set ZEN_API_DOC=check)"
            );
            Mode::Off
        }
        Err(_) => Mode::Regen,
    }
}

/// Snapshot the public API of every publishable library crate in the calling
/// workspace. Equivalent to `ApiDoc::new().run()`.
///
/// # Panics
///
/// Panics on any failure (missing tooling, rustdoc errors, or — in
/// `ZEN_API_DOC=check` mode — a stale committed snapshot). It is meant to be
/// called from a `#[test]`.
pub fn run() {
    ApiDoc::new().run();
}

/// Builder for workspaces that need more control than [`run`].
#[derive(Default)]
pub struct ApiDoc {
    crates: Option<Vec<String>>,
    overrides: Vec<(String, Extra)>,
    excluded: Vec<(String, Vec<String>)>,
    out_dir: Option<String>,
    workspace_dir: Option<String>,
    attributed: Vec<String>,
    skip_packaging: Vec<String>,
    packaging_forbid_extra: Vec<String>,
    base: Vec<(String, String)>,
}

/// Substrings that must never appear in `cargo package --list` output for a
/// publishable crate: snapshot docs and the snapshot test would force
/// nightly-rustdoc onto downstream test runs, and the rest is repo-local
/// session tooling. (The org keeps these out via `exclude`/`include` rules;
/// this check makes the invariant self-enforcing instead of audit-enforced.)
const PACKAGING_FORBIDDEN: &[&str] = &[
    "docs/public-api/",
    "public_api_doc",
    "CLAUDE.md",
    ".workongoing",
    "CONTEXT-HANDOFF.md",
    "FEEDBACK.md",
];

impl ApiDoc {
    /// Start with defaults: auto-discover publishable workspace members that
    /// have a library target, snapshot to `<workspace>/docs/public-api/`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicit crate list, replacing auto-discovery.
    pub fn crates<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.crates = Some(names.into_iter().map(Into::into).collect());
        self
    }

    /// Skip the features file content for `crate_name` (its full feature
    /// set doesn't build, or default features are the only public surface).
    pub fn no_extra_section(mut self, crate_name: &str) -> Self {
        self.overrides.push((crate_name.to_owned(), Extra::None));
        self
    }

    /// Use a pinned `--features` csv for `crate_name`'s features file
    /// instead of "all features except `default`, `_*`, and excluded".
    pub fn pinned_features(mut self, crate_name: &str, features_csv: &str) -> Self {
        self.overrides.push((
            crate_name.to_owned(),
            Extra::Pinned(features_csv.to_owned()),
        ));
        self
    }

    /// Treat the named features of `crate_name` as EXCLUDED: their surface
    /// is documented in `<crate>.internal.txt` instead of the supported
    /// files. This is how `experimental`-style gates are kept out of the
    /// headline without the semver break of renaming them to `_*`.
    pub fn exclude_features<I, S>(mut self, crate_name: &str, features: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.excluded.push((
            crate_name.to_owned(),
            features.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Output directory relative to the workspace root.
    /// Default: `docs/public-api`.
    pub fn out_dir(mut self, rel: &str) -> Self {
        self.out_dir = Some(rel.to_owned());
        self
    }

    /// Target workspace directory (where its `Cargo.toml` lives), relative
    /// to the test's working directory. Default: the calling package's own
    /// workspace.
    ///
    /// This is what makes the **CI-free runner package** pattern work: a
    /// tiny `publish = false` package in an `apidoc/` directory that the
    /// real workspace `exclude`s, holding the only dependency on this
    /// crate. Plain `cargo test` (and every CI job, including
    /// `--all-features` ones) never compiles the apidoc tree or runs
    /// rustdoc; regeneration happens via
    /// `cargo test --manifest-path apidoc/Cargo.toml` from a justfile:
    ///
    /// ```no_run
    /// // apidoc/tests/public_api_doc.rs
    /// #[test]
    /// fn public_api_surface_docs_are_current() {
    ///     zenutils_apidoc::ApiDoc::new().workspace_dir("..").run();
    /// }
    /// ```
    pub fn workspace_dir(mut self, dir: &str) -> Self {
        self.workspace_dir = Some(dir.to_owned());
        self
    }

    /// Attribute the features file of `crate_name` per feature: one
    /// `## added by feature: <name>` section per non-excluded feature (one
    /// extra rustdoc build each), plus a `feature interactions` section for
    /// lines that only appear when several features combine. Off by default
    /// because of the build cost; the unattributed combined delta is
    /// otherwise identical in content.
    pub fn attribute_features(mut self, crate_name: &str) -> Self {
        self.attributed.push(crate_name.to_owned());
        self
    }

    /// Baseline `--features` csv for `crate_name`'s SUPPORTED-surface build
    /// (the `<crate>.txt` file), for crates whose plain default features do
    /// not compile — e.g. a backend-selection `compile_error!` gate. The
    /// snapshot header records the baseline. The features file stays a
    /// delta vs this baseline.
    pub fn base_features(mut self, crate_name: &str, features_csv: &str) -> Self {
        self.base
            .push((crate_name.to_owned(), features_csv.to_owned()));
        self
    }

    /// Skip the packaging-invariant check for `crate_name` (e.g. when
    /// `cargo package` cannot run in this environment). The check otherwise
    /// asserts that no snapshot docs, snapshot tests, or repo-local session
    /// files leak into the published package.
    pub fn skip_packaging_check(mut self, crate_name: &str) -> Self {
        self.skip_packaging.push(crate_name.to_owned());
        self
    }

    /// Additional substrings to forbid in `cargo package --list` output,
    /// on top of the built-in set (snapshot docs/tests + session files).
    pub fn forbid_in_package<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.packaging_forbid_extra
            .extend(patterns.into_iter().map(Into::into));
        self
    }

    /// Regenerate or check the snapshots, per the `ZEN_API_DOC` mode.
    ///
    /// # Panics
    ///
    /// Panics on any failure; see [`run`].
    pub fn run(self) {
        let check = match mode() {
            Mode::Off => return,
            Mode::Check => true,
            Mode::Regen => false,
        };

        let toolchain = toolchain();
        rustup_toolchain::install(&toolchain).unwrap_or_else(|e| {
            panic!(
                "failed to install {toolchain} via rustup ({e}); \
                 set ZEN_API_DOC=off to skip the public-API snapshot test"
            )
        });

        let meta = workspace_metadata(self.workspace_dir.as_deref());
        let workspace_root = PathBuf::from(
            meta["workspace_root"]
                .as_str()
                .expect("workspace_root in cargo metadata"),
        );
        let out_dir = workspace_root.join(self.out_dir.as_deref().unwrap_or("docs/public-api"));

        let crates = match &self.crates {
            Some(list) => list.clone(),
            None => discover_publishable_libs(&meta),
        };
        assert!(
            !crates.is_empty(),
            "no publishable library crates found in this workspace; pass an \
             explicit list via ApiDoc::crates()"
        );

        for package in &crates {
            let extra = self
                .overrides
                .iter()
                .find(|(name, _)| name == package)
                .map(|(_, e)| e)
                .unwrap_or(&Extra::PublicFeatures);
            let excluded_cfg: Vec<String> = self
                .excluded
                .iter()
                .filter(|(name, _)| name == package)
                .flat_map(|(_, f)| f.iter().cloned())
                .collect();
            if !self.skip_packaging.iter().any(|c| c == package) {
                packaging_check(
                    &workspace_root,
                    package,
                    &pkg_ref(&meta, package).spec,
                    &self.packaging_forbid_extra,
                );
            }
            let attribute = self.attributed.iter().any(|c| c == package);
            let base_feats: Vec<String> = self
                .base
                .iter()
                .find(|(name, _)| name == package)
                .map(|(_, csv)| csv.split(',').map(str::to_owned).collect())
                .unwrap_or_default();
            let files = snapshot_one(
                &workspace_root,
                &meta,
                package,
                extra,
                &excluded_cfg,
                attribute,
                &base_feats,
            );
            for (suffix, doc) in files {
                let path = out_dir.join(format!("{package}{suffix}"));
                let existing = std::fs::read_to_string(&path).ok();
                if check {
                    assert_eq!(
                        existing.as_deref(),
                        Some(doc.as_str()),
                        "committed public-API snapshot for {package} is stale: run \
                         `cargo test` locally and commit the regenerated {}",
                        path.display()
                    );
                } else if existing.as_deref() != Some(doc.as_str()) {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, &doc).unwrap();
                    eprintln!(
                        "regenerated {} — review and commit the diff",
                        path.display()
                    );
                }
            }
        }
    }
}

/// Assert the packaging invariant: no snapshot docs/tests or repo-local
/// session files in the crate's published package. Runs `cargo package
/// --list` (fast, no compile). A crate that cannot run `cargo package` at
/// all fails loudly — use [`ApiDoc::skip_packaging_check`] to opt out.
fn packaging_check(workspace_root: &Path, package: &str, spec: &str, extra_forbid: &[String]) {
    let out = Command::new("cargo")
        .arg("package")
        .arg("--list")
        .arg("--allow-dirty")
        .args(["--package", spec])
        .arg("--manifest-path")
        .arg(workspace_root.join("Cargo.toml"))
        .output()
        .unwrap_or_else(|e| panic!("failed to run cargo package --list: {e}"));
    assert!(
        out.status.success(),
        "cargo package --list failed for {package} (skip with \
         ApiDoc::skip_packaging_check(\"{package}\")):\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let listing = String::from_utf8_lossy(&out.stdout);
    let mut violations: Vec<&str> = Vec::new();
    for line in listing.lines() {
        if PACKAGING_FORBIDDEN.iter().any(|p| line.contains(p))
            || extra_forbid.iter().any(|p| line.contains(p.as_str()))
        {
            violations.push(line);
        }
    }
    assert!(
        violations.is_empty(),
        "{package}'s published package would ship repo-local files: \
         {violations:?} — fix the manifest's include/exclude (whitelist \
         crates: move the offending file out of the whitelisted globs; see \
         the tests-dev/ pattern)"
    );
}

fn workspace_metadata(workspace_dir: Option<&str>) -> serde_json::Value {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--no-deps", "--format-version", "1"]);
    if let Some(dir) = workspace_dir {
        cmd.arg("--manifest-path")
            .arg(Path::new(dir).join("Cargo.toml"));
    }
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to run cargo metadata: {e}"));
    assert!(
        out.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("cargo metadata JSON")
}

/// Workspace members that are publishable (`publish` is not `[]`/false) and
/// have a library-ish target (`lib` or `proc-macro`; bin-only and
/// cdylib-only members are skipped — list those explicitly if needed).
fn discover_publishable_libs(meta: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = meta["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .filter(|p| !matches!(p["publish"].as_array(), Some(a) if a.is_empty()))
        .filter(|p| {
            p["targets"].as_array().is_some_and(|targets| {
                targets.iter().any(|t| {
                    t["kind"].as_array().is_some_and(|kinds| {
                        kinds
                            .iter()
                            .any(|k| matches!(k.as_str(), Some("lib" | "rlib" | "proc-macro")))
                    })
                })
            })
        })
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// Manifest features of `package` split into (included, excluded): excluded
/// = `_*`-prefixed plus the configured exclusions; `default` is neither.
fn split_features(
    meta: &serde_json::Value,
    package: &str,
    excluded_cfg: &[String],
) -> (Vec<String>, Vec<String>) {
    let pkg = meta["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|p| p["name"] == package)
        .unwrap_or_else(|| panic!("{package} not in workspace metadata"));
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    for k in pkg["features"].as_object().expect("features map").keys() {
        if k == "default" {
            continue;
        }
        if k.starts_with('_') || excluded_cfg.iter().any(|e| e == k) {
            excluded.push(k.clone());
        } else {
            included.push(k.clone());
        }
    }
    included.sort();
    excluded.sort();
    (included, excluded)
}

/// Cargo-facing identity for one snapshotted crate, resolved once from the
/// `--no-deps` workspace metadata.
///
/// `spec` is the `name@version` package-id spec used for every cargo
/// invocation: a bare name is ambiguous whenever the crate's own
/// registry-published version is also in the resolve graph (e.g. a
/// dev-dependency that depends on the published release of the very crate
/// being documented — zenquant via zengif was the motivating case).
/// `json_name` is rustdoc's output filename stem: the lib/proc-macro target
/// name with `-` mapped to `_`, which honors `[lib] name` overrides instead
/// of assuming it matches the package name.
struct PkgRef {
    spec: String,
    json_name: String,
    target_dir: PathBuf,
}

fn pkg_ref(meta: &serde_json::Value, package: &str) -> PkgRef {
    let pkg = meta["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|p| p["name"] == package)
        .unwrap_or_else(|| panic!("{package} not in workspace metadata"));
    let version = pkg["version"].as_str().expect("package version");
    let lib_target = pkg["targets"]
        .as_array()
        .expect("targets array")
        .iter()
        .find(|t| {
            t["kind"].as_array().is_some_and(|kinds| {
                kinds
                    .iter()
                    .any(|k| matches!(k.as_str(), Some("lib" | "rlib" | "proc-macro")))
            })
        })
        .and_then(|t| t["name"].as_str())
        .unwrap_or(package);
    PkgRef {
        spec: format!("{package}@{version}"),
        json_name: lib_target.replace('-', "_"),
        target_dir: PathBuf::from(
            meta["target_directory"]
                .as_str()
                .expect("target_directory in cargo metadata"),
        ),
    }
}

/// Build rustdoc JSON for the crate with the given features and render the
/// public API lines (sorted, blanket impls omitted). With `hidden`,
/// `#[doc(hidden)]` items are documented too. All builds go through one
/// directly-spawned `cargo rustdoc` so the disambiguated `--package` spec
/// and the nightly-only `--document-hidden-items` flag are both available.
fn try_surface(
    workspace_root: &Path,
    pkg: &PkgRef,
    features: &[String],
    hidden: bool,
) -> Result<Vec<String>, String> {
    let json_path = build_json(workspace_root, pkg, features, hidden)?;
    let api = public_api::Builder::from_rustdoc_json(json_path)
        .omit_blanket_impls(true)
        .sorted(true)
        .build()
        .map_err(|e| {
            format!(
                "public-api parse failed: {e} (usually a rustdoc JSON format \
                 mismatch between the '{}' toolchain and the rustdoc-types \
                 version public-api compiled against — update the toolchain, \
                 or pin one via the ZEN_API_DOC_TOOLCHAIN env var)",
                toolchain()
            )
        })?;
    Ok(api.items().map(|item| item.to_string()).collect())
}

/// One `cargo rustdoc` for every build in the matrix. The JSON lands at
/// `<target-dir>/doc/<json_name>.json` (overwritten per build — the caller
/// parses it immediately). `--cap-lints allow` keeps deny-warnings
/// environments from failing surface extraction; lint gating belongs to the
/// regular CI jobs, not the snapshot.
fn build_json(
    workspace_root: &Path,
    pkg: &PkgRef,
    features: &[String],
    hidden: bool,
) -> Result<PathBuf, String> {
    let mut cmd = Command::new("rustup");
    cmd.args(["run", &toolchain(), "cargo", "rustdoc"])
        .arg("--manifest-path")
        .arg(workspace_root.join("Cargo.toml"))
        .args(["--package", &pkg.spec, "--lib", "--quiet"]);
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    cmd.args([
        "--",
        "-Z",
        "unstable-options",
        "--output-format",
        "json",
        "--cap-lints",
        "allow",
    ]);
    if hidden {
        cmd.arg("--document-hidden-items");
    }
    let out = cmd
        .output()
        .map_err(|e| format!("failed to spawn cargo rustdoc: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "rustdoc JSON build failed: {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .find(|l| l.contains("error"))
                .unwrap_or("see rustdoc output")
        ));
    }
    let path = pkg
        .target_dir
        .join("doc")
        .join(format!("{}.json", pkg.json_name));
    if !path.exists() {
        return Err(format!("rustdoc JSON not found at {}", path.display()));
    }
    Ok(path)
}

fn surface(workspace_root: &Path, pkg: &PkgRef, features: &[String], hidden: bool) -> Vec<String> {
    try_surface(workspace_root, pkg, features, hidden).unwrap_or_else(|e| {
        panic!(
            "{e} for {} (features {features:?}); \
             set ZEN_API_DOC=off to skip the public-API snapshot test",
            pkg.spec
        )
    })
}

const FILE_MAIN: &str = ".txt";
const FILE_FEATURES: &str = ".features.txt";
const FILE_INTERNAL: &str = ".internal.txt";

fn snapshot_one(
    workspace_root: &Path,
    meta: &serde_json::Value,
    package: &str,
    extra: &Extra,
    excluded_cfg: &[String],
    attribute: bool,
    base_feats: &[String],
) -> Vec<(&'static str, String)> {
    let crate_ident = package.replace('-', "_");
    let pkg = pkg_ref(meta, package);
    let (auto_included, auto_excluded) = split_features(meta, package, excluded_cfg);

    let (feature_label, included_feats) = match extra {
        Extra::None => (String::new(), Vec::new()),
        Extra::PublicFeatures => (auto_included.join(","), auto_included),
        Extra::Pinned(csv) => (
            csv.clone(),
            csv.split(',').map(str::to_owned).collect::<Vec<_>>(),
        ),
    };
    let excluded_feats = auto_excluded;

    // Build matrix. The default + included builds are load-bearing (panic on
    // failure); the excluded / hidden builds degrade to a note in the
    // internal file (excluded feature unions are allowed to be unbuildable).
    let base_lines = surface(workspace_root, &pkg, base_feats, false);

    // The included build is a superset of the baseline by construction.
    let mut included_feats = included_feats;
    for f in base_feats {
        if !included_feats.contains(f) {
            included_feats.push(f.clone());
        }
    }
    let included_lines = if included_feats == base_feats {
        base_lines.clone()
    } else {
        surface(workspace_root, &pkg, &included_feats, false)
    };

    let mut notes: Vec<String> = Vec::new();

    let mut with_excluded_feats = included_feats.clone();
    with_excluded_feats.extend(excluded_feats.iter().cloned());
    let excluded_lines = if excluded_feats.is_empty() {
        included_lines.clone()
    } else {
        match try_surface(workspace_root, &pkg, &with_excluded_feats, false) {
            Ok(lines) => lines,
            Err(e) => {
                notes.push(format!(
                    "excluded-feature surface ({}) not buildable: {}",
                    excluded_feats.join(","),
                    e.lines().next().unwrap_or("unknown error")
                ));
                included_lines.clone()
            }
        }
    };

    // Hidden build: widest feature set that built, with doc(hidden) items on.
    let hidden_base = if excluded_lines.len() >= included_lines.len() && notes.is_empty() {
        &with_excluded_feats
    } else {
        &included_feats
    };
    let hidden_lines = match try_surface(workspace_root, &pkg, hidden_base, true) {
        Ok(lines) => lines,
        Err(e) => {
            notes.push(format!(
                "doc(hidden) surface not buildable: {}",
                e.lines().next().unwrap_or("unknown error")
            ));
            excluded_lines.clone()
        }
    };

    // Disjoint line sets.
    let base_set: HashSet<&str> = base_lines.iter().map(String::as_str).collect();
    let included_set: HashSet<&str> = included_lines.iter().map(String::as_str).collect();
    let excluded_set: HashSet<&str> = excluded_lines.iter().map(String::as_str).collect();

    let feat_added: Vec<String> = included_lines
        .iter()
        .filter(|l| !base_set.contains(l.as_str()))
        .cloned()
        .collect();
    let feat_removed: Vec<String> = base_lines
        .iter()
        .filter(|l| !included_set.contains(l.as_str()))
        .cloned()
        .collect();
    let excl_added: Vec<String> = excluded_lines
        .iter()
        .filter(|l| !included_set.contains(l.as_str()))
        .cloned()
        .collect();
    let hidden_added: Vec<String> = hidden_lines
        .iter()
        .filter(|l| !excluded_set.contains(l.as_str()))
        .cloned()
        .collect();

    let main = transform(&base_lines, &crate_ident);
    let features = transform(&feat_added, &crate_ident);
    let removed: Vec<String> = feat_removed
        .iter()
        .map(|l| strip_crate_prefix(l, &crate_ident))
        .collect();
    let mut internal_lines = hidden_added;
    internal_lines.extend(excl_added.iter().cloned());
    internal_lines.sort();
    internal_lines.dedup();
    let excl_added_set: HashSet<&str> = excl_added.iter().map(String::as_str).collect();
    let hidden_count = internal_lines
        .iter()
        .filter(|l| !excl_added_set.contains(l.as_str()))
        .count();
    let internal = transform(&internal_lines, &crate_ident);

    let header_common = "# (regenerated on every `cargo test` by zenutils-apidoc; \
         ZEN_API_DOC=check verifies, =off skips).\n\
         # Encodings: crate-name prefix stripped; auto traits collapse to a\n\
         # count + exceptions; trait impls collapse to one roster line per\n\
         # type (method signatures live at the trait definition); blanket\n\
         # impls omitted; re-export duplicates annotated `[also: path]`.\n\
         # DO NOT EDIT BY HAND — commit regenerated changes with the code.\n";

    let overview = format!(
        "#\n# files: {package}{FILE_MAIN} {} lines (supported surface) | \
         {package}{FILE_FEATURES} {} added (features: {}) | \
         {package}{FILE_INTERNAL} {} lines ({} hidden + {} excluded-feature)\n",
        main.total_lines(),
        features.total_lines(),
        if feature_label.is_empty() {
            "none"
        } else {
            &feature_label
        },
        internal.total_lines(),
        hidden_count,
        excl_added.len(),
    );

    let mut out = Vec::new();

    // --- <crate>.txt : supported surface ---
    let mut a = if base_feats.is_empty() {
        format!("# {package} public API — supported surface (default features)\n")
    } else {
        format!(
            "# {package} public API — supported surface (default features + {})\n",
            base_feats.join(",")
        )
    };
    a.push_str(header_common);
    a.push_str(&overview);
    a.push('\n');
    a.push_str(&main.render_summary());
    a.push_str(&main.render_body());
    out.push((FILE_MAIN, a));
    eprintln!("{package} [supported]: {} lines", main.total_lines());

    // --- <crate>.features.txt : non-excluded feature additions ---
    let mut b = format!(
        "# {package} public API — additions from non-default features\n\
         # features: {}\n",
        if feature_label.is_empty() {
            "(none)"
        } else {
            &feature_label
        }
    );
    b.push_str(header_common);
    b.push('\n');
    if features.total_lines() == 0 && removed.is_empty() {
        b.push_str("(no additional public surface)\n");
    } else {
        b.push_str(&features.render_summary());
        b.push_str(&features.render_body());
        if !removed.is_empty() {
            let _ = write!(b, "\n## removed by features ({} lines)\n\n", removed.len());
            for l in &removed {
                b.push_str(l);
                b.push('\n');
            }
        }
        // Opt-in per-feature attribution: one extra build per feature, each
        // delta'd against the default surface; lines that only appear when
        // features combine land in the interactions section.
        if attribute && included_feats.len() > 1 {
            let base_set: HashSet<&str> = base_lines.iter().map(String::as_str).collect();
            let mut attributed_union: HashSet<String> = HashSet::new();
            for feat in &included_feats {
                match try_surface(workspace_root, &pkg, std::slice::from_ref(feat), false) {
                    Ok(lines) => {
                        let delta: Vec<String> = lines
                            .iter()
                            .filter(|l| !base_set.contains(l.as_str()))
                            .map(|l| strip_crate_prefix(l, &crate_ident))
                            .collect();
                        attributed_union.extend(delta.iter().cloned());
                        let _ = write!(
                            b,
                            "\n## added by feature: {feat} ({} lines)\n\n",
                            delta.len()
                        );
                        for l in &delta {
                            b.push_str(l);
                            b.push('\n');
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(
                            b,
                            "\nNOTE: feature {feat} not buildable alone: {}",
                            e.lines().next().unwrap_or("unknown error")
                        );
                    }
                }
            }
            let interactions: Vec<String> = feat_added
                .iter()
                .map(|l| strip_crate_prefix(l, &crate_ident))
                .filter(|l| !attributed_union.contains(l))
                .collect();
            if !interactions.is_empty() {
                let _ = write!(
                    b,
                    "\n## feature interactions (lines requiring several features) ({} lines)\n\n",
                    interactions.len()
                );
                for l in &interactions {
                    b.push_str(l);
                    b.push('\n');
                }
            }
        }
    }
    out.push((FILE_FEATURES, b));
    eprintln!(
        "{package} [+features {feature_label}]: {} added lines",
        features.total_lines()
    );

    // --- <crate>.internal.txt : hidden + excluded-feature surface ---
    let mut c = format!(
        "# {package} public API — doc(hidden) items and excluded-feature surface\n\
         # excluded features: {}\n",
        if excluded_feats.is_empty() {
            "(none)".to_owned()
        } else {
            excluded_feats.join(",")
        }
    );
    c.push_str(header_common);
    c.push('\n');
    for n in &notes {
        let _ = writeln!(c, "NOTE: {n}");
    }
    if internal.total_lines() == 0 {
        c.push_str("(no hidden or excluded-feature surface)\n");
    } else {
        c.push_str(&internal.render_summary());
        c.push_str(&internal.render_body());
    }
    out.push((FILE_INTERNAL, c));
    eprintln!(
        "{package} [internal]: {} lines ({} hidden, {} excluded-feature)",
        internal.total_lines(),
        hidden_count,
        excl_added.len()
    );

    out
}

// ---------------------------------------------------------------------------
// Transformation: raw public-api lines → encoded sections.

const AUTO_TRAITS: [&str; 6] = [
    "Freeze",
    "RefUnwindSafe",
    "Send",
    "Sync",
    "Unpin",
    "UnwindSafe",
];

/// Auto-trait paths → short name. `StructuralPartialEq` and the unstable
/// `UnsafeUnpin` are dropped entirely (the former tracks the `PartialEq`
/// derive, already in the roster; the latter is an unstable artifact).
fn auto_trait_short(path: &str) -> Option<&'static str> {
    match path {
        "core::marker::Freeze" => Some("Freeze"),
        "core::marker::Send" => Some("Send"),
        "core::marker::Sync" => Some("Sync"),
        "core::marker::Unpin" => Some("Unpin"),
        "core::panic::unwind_safe::RefUnwindSafe" => Some("RefUnwindSafe"),
        "core::panic::unwind_safe::UnwindSafe" => Some("UnwindSafe"),
        _ => None,
    }
}

fn is_dropped_marker(path: &str) -> bool {
    matches!(
        path,
        "core::marker::StructuralPartialEq" | "core::marker::UnsafeUnpin"
    )
}

#[derive(Default)]
struct AutoInfo {
    present: BTreeSet<&'static str>,
    negative: BTreeSet<&'static str>,
    conditional: Vec<String>,
}

#[derive(Default)]
struct Transformed {
    /// Item lines (after prefix-strip, member-drop, dedupe), in input order.
    items: Vec<String>,
    /// type → sorted trait names (non-auto, unconditional).
    rosters: BTreeMap<String, BTreeSet<String>>,
    /// Conditional (where-bearing) trait impls, verbatim.
    conditional_impls: Vec<String>,
    /// type → auto-trait info.
    autos: BTreeMap<String, AutoInfo>,
    tally: Tally,
    per_module: BTreeMap<String, usize>,
}

impl Transformed {
    fn auto_complete_count(&self) -> usize {
        self.autos.values().filter(|a| auto_is_complete(a)).count()
    }

    fn auto_exceptions(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .autos
            .iter()
            .filter(|(_, a)| !auto_is_complete(a))
            .map(|(ty, a)| {
                let missing: Vec<String> = AUTO_TRAITS
                    .iter()
                    .filter(|t| !a.present.contains(*t))
                    .map(|t| format!("!{t}"))
                    .collect();
                format!("{ty}: {}", missing.join(" "))
            })
            .collect();
        v.sort();
        v
    }

    fn total_lines(&self) -> usize {
        let auto_lines = if self.autos.is_empty() {
            0
        } else {
            1 + self.auto_exceptions().len()
        };
        self.items.len() + self.rosters.len() + self.conditional_impls.len() + auto_lines
    }

    fn render_summary(&self) -> String {
        let mut s = String::from("## summary\n#\n");
        let t = &self.tally;
        for (label, n) in [
            ("pub modules", t.modules),
            ("pub types (struct/enum/trait/alias)", t.types),
            ("pub consts/statics", t.consts),
            ("pub macros", t.macros),
            ("free functions", t.free_fns),
            ("inherent methods", t.assoc_fns),
            ("struct fields", t.fields),
            ("enum variants", t.variants),
            ("re-exports", t.reexports),
            (
                "trait roster entries (type × trait)",
                self.roster_entry_count(),
            ),
            (
                "conditional trait impls (verbatim)",
                self.conditional_impls.len(),
            ),
            ("auto-trait-complete types", self.auto_complete_count()),
            ("auto-trait exceptions", self.auto_exceptions().len()),
            ("other", t.other),
        ] {
            if n == 0 {
                continue;
            }
            let _ = writeln!(s, "#   {label:<38} {n:>6}");
        }
        if !self.per_module.is_empty() {
            s.push_str("#\n# per-module pub lines:\n");
            for (module, n) in &self.per_module {
                let _ = writeln!(s, "#   {module:<28} {n:>6}");
            }
        }
        s
    }

    fn roster_entry_count(&self) -> usize {
        self.rosters.values().map(BTreeSet::len).sum()
    }

    fn render_body(&self) -> String {
        let mut s = String::new();
        let _ = write!(s, "\n## items ({} lines)\n\n", self.items.len());
        for l in &self.items {
            s.push_str(l);
            s.push('\n');
        }
        if !self.rosters.is_empty() || !self.conditional_impls.is_empty() {
            let _ = write!(s, "\n## trait impls ({} types)\n\n", self.rosters.len());
            for (ty, traits) in &self.rosters {
                let list: Vec<&str> = traits.iter().map(String::as_str).collect();
                let _ = writeln!(s, "{ty}: {}", list.join(", "));
            }
            for l in &self.conditional_impls {
                s.push_str(l);
                s.push('\n');
            }
        }
        if !self.autos.is_empty() {
            let exceptions = self.auto_exceptions();
            let _ = write!(s, "\n## auto traits\n\n");
            let _ = writeln!(
                s,
                "{} types implement all of: {}",
                self.auto_complete_count(),
                AUTO_TRAITS.join(", ")
            );
            for l in &exceptions {
                s.push_str(l);
                s.push('\n');
            }
        }
        s
    }
}

fn auto_is_complete(a: &AutoInfo) -> bool {
    a.negative.is_empty()
        && a.conditional.is_empty()
        && AUTO_TRAITS.iter().all(|t| a.present.contains(t))
}

/// Strip `{crate_ident}::` everywhere in the line (paths and signatures).
fn strip_crate_prefix(line: &str, crate_ident: &str) -> String {
    line.replace(&format!("{crate_ident}::"), "")
}

/// `core::`/`alloc::`/`std::` trait paths shorten to their final segment
/// (with generic args); everything else is kept whole.
fn simplify_trait_path(path: &str) -> String {
    let root = path.split("::").next().unwrap_or("");
    if matches!(root, "core" | "alloc" | "std") {
        path_segments(path)
            .last()
            .map_or_else(|| path.to_owned(), |s| (*s).to_owned())
    } else {
        path.to_owned()
    }
}

/// Parse an impl line body (`Trait for Type` / `Type` / `!Trait for Type`),
/// already `where`-checked by the caller.
enum ImplKind<'a> {
    Inherent,
    Trait {
        trait_path: &'a str,
        for_type: &'a str,
        negative: bool,
    },
}

fn parse_impl_body(body: &str) -> ImplKind<'_> {
    let (negative, body) = match body.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    // Find ` for ` at angle-depth 0.
    let bytes = body.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i + 5 <= bytes.len() {
        match bytes[i] {
            b'<' | b'(' => depth += 1,
            b'>' | b')' => depth = depth.saturating_sub(1),
            b' ' if depth == 0 && body[i..].starts_with(" for ") => {
                return ImplKind::Trait {
                    trait_path: &body[..i],
                    for_type: &body[i + 5..],
                    negative,
                };
            }
            _ => {}
        }
        i += 1;
    }
    ImplKind::Inherent
}

/// The full transformation pipeline for one disjoint line set.
fn transform(lines: &[String], crate_ident: &str) -> Transformed {
    let stripped: Vec<String> = lines
        .iter()
        .map(|l| strip_crate_prefix(l, crate_ident))
        .collect();

    let mut t = Transformed::default();
    // Trait-impl member attribution: members directly follow their impl line
    // in public-api's sorted output (verified empirically); track the type
    // whose trait-impl members should be dropped.
    let mut trait_member_ctx: Option<String> = None;

    let mut kept: Vec<String> = Vec::new();
    for line in &stripped {
        let l = strip_attrs(line);
        if let Some(rest) = impl_body_text(l) {
            let (body, has_where) = match rest.find(" where ") {
                Some(i) => (&rest[..i], true),
                None => (rest, false),
            };
            match parse_impl_body(body) {
                ImplKind::Inherent => {
                    // The inherent-impl line itself carries no information
                    // beyond its members, which are kept.
                    trait_member_ctx = None;
                }
                ImplKind::Trait {
                    trait_path,
                    for_type,
                    negative,
                } => {
                    trait_member_ctx = Some(for_type.to_owned());
                    if is_dropped_marker(trait_path) {
                        continue;
                    }
                    if let Some(short) = auto_trait_short(trait_path) {
                        let info = t.autos.entry(for_type.to_owned()).or_default();
                        if has_where {
                            info.conditional.push(line.clone());
                        } else if negative {
                            info.negative.insert(short);
                        } else {
                            info.present.insert(short);
                        }
                        continue;
                    }
                    if has_where || negative {
                        t.conditional_impls.push(line.clone());
                    } else {
                        t.rosters
                            .entry(for_type.to_owned())
                            .or_default()
                            .insert(simplify_trait_path(trait_path));
                    }
                }
            }
            continue;
        }

        // Non-impl line. Drop fn/const/type members of the current
        // trait-impl context (their signatures are fixed by the trait);
        // fields and variants are the type's own surface and always kept.
        if let Some(ctx) = &trait_member_ctx {
            if let Some(body) = l.strip_prefix("pub ") {
                let member = body
                    .strip_prefix("fn ")
                    .or_else(|| body.strip_prefix("const "))
                    .or_else(|| body.strip_prefix("type "));
                if let Some(sig) = member {
                    let path = leading_path(sig);
                    if path
                        .strip_prefix(ctx.as_str())
                        .is_some_and(|r| r.starts_with("::"))
                    {
                        continue;
                    }
                }
            }
            trait_member_ctx = None;
        }
        classify(l, &mut t.tally, &mut t.per_module);
        kept.push(line.clone());
    }

    t.items = dedupe_reexport_paths(kept);
    t
}

/// Collapse identical items reachable at multiple paths: keep the shortest
/// path, annotate with the alternates' parent paths. Key = item kind + final
/// path segment + signature tail; only exact-signature duplicates collapse.
fn dedupe_reexport_paths(lines: Vec<String>) -> Vec<String> {
    #[derive(Default)]
    struct Group {
        first_idx: usize,
        entries: Vec<(usize, String, String)>, // (idx, path, full line)
    }
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for (idx, line) in lines.iter().enumerate() {
        let l = strip_attrs(line);
        let Some(body) = l.strip_prefix("pub ") else {
            continue;
        };
        let (kind, rest) = match body.split_once(' ') {
            Some((k @ ("fn" | "struct" | "enum" | "trait" | "type" | "const" | "static"), r)) => {
                (k, r)
            }
            _ => continue,
        };
        let path = leading_path(rest);
        let segs = path_segments(path);
        let Some(last) = segs.last() else { continue };
        // Members (Type::method) never dedupe — only top-level items.
        if segs.len() >= 2
            && segs[segs.len() - 2]
                .chars()
                .next()
                .is_some_and(char::is_uppercase)
        {
            continue;
        }
        let sig_tail = &rest[path.len()..];
        let key = format!("{kind} {last}{sig_tail}");
        let g = groups.entry(key).or_insert_with(|| Group {
            first_idx: idx,
            entries: Vec::new(),
        });
        g.first_idx = g.first_idx.min(idx);
        g.entries.push((idx, path.to_owned(), line.clone()));
    }

    let mut drop_idx: HashSet<usize> = HashSet::new();
    let mut annotate: BTreeMap<usize, String> = BTreeMap::new();
    for g in groups.values() {
        if g.entries.len() < 2 {
            continue;
        }
        let canonical = g
            .entries
            .iter()
            .min_by_key(|(idx, path, _)| (path.len(), *idx))
            .expect("non-empty group");
        let mut others: Vec<String> = g
            .entries
            .iter()
            .filter(|(idx, _, _)| *idx != canonical.0)
            .map(|(idx, path, _)| {
                drop_idx.insert(*idx);
                parent_path(path)
            })
            .collect();
        others.sort();
        others.dedup();
        annotate.insert(canonical.0, format!(" [also: {}]", others.join(", ")));
    }

    lines
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !drop_idx.contains(idx))
        .map(|(idx, line)| match annotate.get(&idx) {
            Some(suffix) => format!("{line}{suffix}"),
            None => line,
        })
        .collect()
}

fn parent_path(path: &str) -> String {
    let segs = path_segments(path);
    if segs.len() <= 1 {
        "(root)".to_owned()
    } else {
        segs[..segs.len() - 1].join("::")
    }
}

// ---------------------------------------------------------------------------
// Line taxonomy (item lines only; impls are handled by the transform).

#[derive(Default, Clone, Copy)]
struct Tally {
    modules: usize,
    types: usize,
    consts: usize,
    macros: usize,
    free_fns: usize,
    assoc_fns: usize,
    fields: usize,
    variants: usize,
    reexports: usize,
    other: usize,
}

/// For an `impl` line, return the part after `impl` / `impl<...>` — the
/// implemented trait (or inherent type). `None` if not an impl line.
fn impl_body_text(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("impl")?;
    if let Some(r) = rest.strip_prefix(' ') {
        return Some(r);
    }
    if rest.starts_with('<') {
        let bytes = rest.as_bytes();
        let mut depth = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'<' => depth += 1,
                b'>' => {
                    depth -= 1;
                    if depth == 0 {
                        return rest[i + 1..].strip_prefix(' ');
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Strip leading `#[...]` attributes (e.g. `#[non_exhaustive] `).
fn strip_attrs(mut line: &str) -> &str {
    while line.starts_with("#[") {
        match line.find("] ") {
            Some(i) => line = &line[i + 2..],
            None => break,
        }
    }
    line
}

/// Split a path on `::` at angle-bracket depth 0 (so `Type<'a>::method`
/// splits into `Type<'a>` + `method`, not inside the generics).
fn path_segments(path: &str) -> Vec<&str> {
    let bytes = path.as_bytes();
    let mut segments = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => depth = depth.saturating_sub(1),
            b':' if depth == 0 && bytes.get(i + 1) == Some(&b':') => {
                segments.push(&path[start..i]);
                i += 2;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    segments.push(&path[start..]);
    segments
}

/// The path portion of a classified line body: everything up to the first
/// depth-0 `(`, ` `, or `: ` type annotation.
fn leading_path(body: &str) -> &str {
    let bytes = body.as_bytes();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => depth = depth.saturating_sub(1),
            b'(' | b' ' if depth == 0 => return &body[..i],
            b':' if depth == 0 => {
                if bytes.get(i + 1) == Some(&b':') {
                    i += 2;
                    continue;
                }
                return &body[..i];
            }
            _ => {}
        }
        i += 1;
    }
    body
}

/// `module` bucket for a path like `module::Item::member` (crate prefix
/// already stripped) — the first segment when it names a module
/// (lowercase), else `(root)`.
fn module_of(path: &str) -> String {
    let segs = path_segments(path);
    if segs.len() >= 2 {
        let m = segs[0].split('<').next().unwrap_or(segs[0]);
        if m.chars()
            .next()
            .is_some_and(|c| c.is_lowercase() || c == '_')
        {
            return m.to_owned();
        }
    }
    "(root)".to_owned()
}

fn classify(l: &str, tally: &mut Tally, per_module: &mut BTreeMap<String, usize>) {
    let Some(body) = l.strip_prefix("pub ") else {
        tally.other += 1;
        return;
    };
    let keyword_stripped = body
        .strip_prefix("mod ")
        .or_else(|| body.strip_prefix("struct "))
        .or_else(|| body.strip_prefix("enum "))
        .or_else(|| body.strip_prefix("trait "))
        .or_else(|| body.strip_prefix("type "))
        .or_else(|| body.strip_prefix("union "))
        .or_else(|| body.strip_prefix("const "))
        .or_else(|| body.strip_prefix("static "))
        .or_else(|| body.strip_prefix("fn "))
        .or_else(|| body.strip_prefix("use "))
        .unwrap_or(body);
    *per_module
        .entry(module_of(leading_path(keyword_stripped)))
        .or_default() += 1;

    if body.starts_with("mod ") {
        tally.modules += 1;
    } else if body.starts_with("struct ")
        || body.starts_with("enum ")
        || body.starts_with("trait ")
        || body.starts_with("type ")
        || body.starts_with("union ")
    {
        tally.types += 1;
    } else if body.starts_with("const ") || body.starts_with("static ") {
        tally.consts += 1;
    } else if body.starts_with("macro") {
        tally.macros += 1;
    } else if body.starts_with("use ") {
        tally.reexports += 1;
    } else if let Some(sig) = body.strip_prefix("fn ") {
        let path = leading_path(sig);
        let segs = path_segments(path);
        let parent = if segs.len() >= 2 {
            segs[segs.len() - 2]
        } else {
            ""
        };
        let parent = parent.split('<').next().unwrap_or(parent);
        if parent.chars().next().is_some_and(char::is_uppercase) {
            tally.assoc_fns += 1;
        } else {
            tally.free_fns += 1;
        }
    } else {
        // Bare path lines: `pub Path::field: Type` (field) or
        // `pub Path::Variant` / `pub Path::Variant(..)` (enum variant).
        let path = leading_path(body);
        let after = &body[path.len()..];
        if after.starts_with(':') {
            tally.fields += 1;
        } else {
            tally.variants += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(lines: &[&str]) -> Transformed {
        let owned: Vec<String> = lines.iter().map(|s| (*s).to_owned()).collect();
        transform(&owned, "demo")
    }

    #[test]
    fn pkg_ref_disambiguates_spec_and_honors_lib_name_override() {
        let meta: serde_json::Value = serde_json::json!({
            "target_directory": "/ws/target",
            "packages": [{
                "name": "my-crate",
                "version": "0.1.2",
                "targets": [
                    {"name": "my-crate-cli", "kind": ["bin"]},
                    {"name": "custom-lib-name", "kind": ["lib"]},
                ],
            }],
        });
        let pkg = pkg_ref(&meta, "my-crate");
        // `name@version` spec stays unambiguous even when the registry
        // release of the same crate is in the resolve graph.
        assert_eq!(pkg.spec, "my-crate@0.1.2");
        // rustdoc's JSON filename comes from the lib target name (which a
        // `[lib] name` override can change), not the package name.
        assert_eq!(pkg.json_name, "custom_lib_name");
        assert_eq!(pkg.target_dir, PathBuf::from("/ws/target"));
    }

    #[test]
    fn crate_prefix_stripped_everywhere() {
        let out = t(&["pub fn demo::util::helper(demo::util::Thing) -> demo::Kind"]);
        assert_eq!(out.items, ["pub fn util::helper(util::Thing) -> Kind"]);
    }

    #[test]
    fn trait_impls_collapse_to_roster_and_members_drop() {
        let out = t(&[
            "impl core::clone::Clone for demo::Thing",
            "pub fn demo::Thing::clone(&self) -> demo::Thing",
            "impl core::fmt::Debug for demo::Thing",
            "pub fn demo::Thing::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result",
            "impl serde::ser::Serialize for demo::Thing",
            "pub fn demo::Thing::serialize<S>(&self, S) -> Result<S::Ok, S::Error>",
            "impl demo::Thing",
            "pub fn demo::Thing::inherent(&self) -> u32",
        ]);
        let roster = out.rosters.get("Thing").expect("Thing roster");
        let names: Vec<&str> = roster.iter().map(String::as_str).collect();
        assert_eq!(names, ["Clone", "Debug", "serde::ser::Serialize"]);
        // Trait-impl methods dropped; inherent method kept.
        assert_eq!(out.items, ["pub fn Thing::inherent(&self) -> u32"]);
        assert_eq!(out.tally.assoc_fns, 1);
    }

    #[test]
    fn auto_traits_count_and_exceptions() {
        let mut lines = Vec::new();
        for tr in [
            "core::marker::Freeze",
            "core::marker::Send",
            "core::marker::Sync",
            "core::marker::Unpin",
            "core::panic::unwind_safe::RefUnwindSafe",
            "core::panic::unwind_safe::UnwindSafe",
        ] {
            lines.push(format!("impl {tr} for demo::Complete"));
        }
        // Partial type: explicit negatives for the unwind traits.
        for tr in [
            "core::marker::Freeze",
            "core::marker::Send",
            "core::marker::Sync",
            "core::marker::Unpin",
        ] {
            lines.push(format!("impl {tr} for demo::Partial"));
        }
        lines.push("impl !core::panic::unwind_safe::RefUnwindSafe for demo::Partial".into());
        lines.push("impl !core::panic::unwind_safe::UnwindSafe for demo::Partial".into());
        // Marker noise that must vanish entirely.
        lines.push("impl core::marker::StructuralPartialEq for demo::Complete".into());
        let owned: Vec<String> = lines;
        let out = transform(&owned, "demo");
        assert_eq!(out.auto_complete_count(), 1);
        assert_eq!(
            out.auto_exceptions(),
            ["Partial: !RefUnwindSafe !UnwindSafe"]
        );
        assert!(out.rosters.is_empty());
    }

    #[test]
    fn conditional_impls_kept_verbatim() {
        let out = t(&[
            "impl<T> core::marker::Send for demo::Wrap<T> where T: core::marker::Send",
            "impl<T> demo::Trait for demo::Wrap<T> where T: core::clone::Clone",
        ]);
        let auto = out.autos.get("Wrap<T>").expect("auto info");
        assert_eq!(auto.conditional.len(), 1);
        assert!(!auto_is_complete(auto));
        assert_eq!(out.conditional_impls.len(), 1);
        assert!(out.conditional_impls[0].contains("where"));
    }

    #[test]
    fn fields_and_variants_survive_trait_member_context() {
        // A field line directly after a trait impl line must NOT be treated
        // as an impl member.
        let out = t(&[
            "impl core::clone::Clone for demo::Config",
            "pub demo::Config::level: u8",
            "pub demo::Kind::VariantA",
        ]);
        assert_eq!(out.items.len(), 2);
        assert_eq!(out.tally.fields, 1);
        assert_eq!(out.tally.variants, 1);
    }

    #[test]
    fn reexport_duplicates_annotated() {
        let out = t(&[
            "pub fn demo::compress(&[u8]) -> Vec<u8>",
            "pub fn demo::deflate::compress(&[u8]) -> Vec<u8>",
            "pub fn demo::deflate::other(&[u8]) -> Vec<u8>",
        ]);
        assert_eq!(
            out.items,
            [
                "pub fn compress(&[u8]) -> Vec<u8> [also: deflate]",
                "pub fn deflate::other(&[u8]) -> Vec<u8>",
            ]
        );
    }

    #[test]
    fn members_never_dedupe_across_types() {
        let out = t(&[
            "pub fn demo::A::len(&self) -> usize",
            "pub fn demo::B::len(&self) -> usize",
        ]);
        assert_eq!(out.items.len(), 2);
    }

    #[test]
    fn classify_taxonomy() {
        let out = t(&[
            "pub mod demo::util",
            "pub struct demo::util::Thing",
            "pub demo::util::Thing::field: u32",
            "pub fn demo::util::Thing::method(&self) -> u32",
            "pub fn demo::util::helper(u32) -> u32",
            "pub fn demo::root_fn() -> bool",
            "pub enum demo::Kind",
            "pub demo::Kind::VariantA",
            "pub demo::Kind::VariantB(u8)",
            "pub const demo::MAX: usize",
            "#[non_exhaustive] pub struct demo::Opts",
        ]);
        let t = &out.tally;
        assert_eq!(t.modules, 1);
        assert_eq!(t.types, 3);
        assert_eq!(t.fields, 1);
        assert_eq!(t.variants, 2);
        assert_eq!(t.free_fns, 2);
        assert_eq!(t.assoc_fns, 1);
        assert_eq!(t.consts, 1);
        assert_eq!(out.per_module.get("util"), Some(&4));
        assert!(out.per_module.contains_key("(root)"));
    }

    #[test]
    fn simplify_trait_paths() {
        assert_eq!(simplify_trait_path("core::clone::Clone"), "Clone");
        assert_eq!(simplify_trait_path("core::convert::From<u8>"), "From<u8>");
        assert_eq!(
            simplify_trait_path("serde::ser::Serialize"),
            "serde::ser::Serialize"
        );
        assert_eq!(simplify_trait_path("zencodec::Encode"), "zencodec::Encode");
    }
}
