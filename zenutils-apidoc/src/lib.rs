//! Public-API snapshot tests for whole workspaces.
//!
//! Regenerates committed public-API surface snapshots
//! (`<workspace>/docs/public-api/<crate>.txt`, one per published crate) from a
//! single `cargo test`, so API changes always show up as a git diff next to
//! the code change that caused them. This is the shared implementation of the
//! `public_api_doc.rs` test that previously lived as a drifting copy in every
//! zen repo.
//!
//! ```no_run
//! // tests/public_api_doc.rs — the whole file, for most workspaces:
//! #[test]
//! fn public_api_surface_docs_are_current() {
//!     zenutils_apidoc::run(); // auto-discovers publishable library members
//! }
//! ```
//!
//! Workspaces that need control use the builder:
//!
//! ```no_run
//! #[test]
//! fn public_api_surface_docs_are_current() {
//!     zenutils_apidoc::ApiDoc::new()
//!         .crates(["zenpipe", "zencodecs", "zenfilters"])
//!         .no_extra_section("zenpipe") // --all-features does not build
//!         .pinned_features("zencodecs", "jxl-encode,cms")
//!         .run();
//! }
//! ```
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
//! # Snapshot layout
//!
//! Each `<crate>.txt` contains:
//! - `## summary` — generated line taxonomy (free functions vs methods vs
//!   fields/variants vs auto-trait/derived impl lines, plus a per-module
//!   table). Raw rustdoc item lines dwarf the real API; the summary keeps the
//!   headline honest.
//! - `## default features (N lines)` — the full surface. Auto-trait impl
//!   lines are kept on purpose: losing `Send`/`Sync` on a public type is a
//!   semver break and must show in the diff. Blanket impls are omitted
//!   (`cargo public-api --simplified` equivalent).
//! - `## added by non-default features: ... (N lines)` — DELTA only: lines
//!   not present in the default section. Underscore-prefixed features are
//!   internal/research gates and excluded; the feature list comes from
//!   `cargo metadata`, so new features appear automatically.
//! - `## removed by non-default features (N lines)` — only when enabling
//!   features removes surface (a `cfg(not(feature = ...))` gate).
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

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Toolchain used for rustdoc JSON: `ZEN_API_DOC_TOOLCHAIN` env override, or
/// the tracking `nightly`.
fn toolchain() -> String {
    std::env::var("ZEN_API_DOC_TOOLCHAIN").unwrap_or_else(|_| "nightly".to_owned())
}

/// How the extra (non-default-features) snapshot section is built for one
/// crate.
enum Extra {
    /// All manifest features except `default` and `_*`-prefixed (default).
    PublicFeatures,
    /// No extra section — snapshot default features only.
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
    out_dir: Option<String>,
}

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

    /// Skip the extra-features section for `crate_name` (its full feature
    /// set doesn't build, or default features are the only public surface).
    pub fn no_extra_section(mut self, crate_name: &str) -> Self {
        self.overrides.push((crate_name.to_owned(), Extra::None));
        self
    }

    /// Use a pinned `--features` csv for `crate_name`'s extra section
    /// instead of "all features except `default` and `_*`".
    pub fn pinned_features(mut self, crate_name: &str, features_csv: &str) -> Self {
        self.overrides.push((
            crate_name.to_owned(),
            Extra::Pinned(features_csv.to_owned()),
        ));
        self
    }

    /// Output directory relative to the workspace root.
    /// Default: `docs/public-api`.
    pub fn out_dir(mut self, rel: &str) -> Self {
        self.out_dir = Some(rel.to_owned());
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

        let meta = workspace_metadata();
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
            let doc = snapshot_one(&workspace_root, &meta, package, extra);
            let path = out_dir.join(format!("{package}.txt"));
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

fn workspace_metadata() -> serde_json::Value {
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
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

/// All manifest features of `package` except `default` and underscore-
/// prefixed internal gates, sorted for determinism.
fn public_features(meta: &serde_json::Value, package: &str) -> Vec<String> {
    let pkg = meta["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|p| p["name"] == package)
        .unwrap_or_else(|| panic!("{package} not in workspace metadata"));
    let mut feats: Vec<String> = pkg["features"]
        .as_object()
        .expect("features map")
        .keys()
        .filter(|k| *k != "default" && !k.starts_with('_'))
        .cloned()
        .collect();
    feats.sort();
    feats
}

/// Build rustdoc JSON for `package` with the given features and render the
/// public API lines (sorted, blanket impls omitted — the
/// `cargo public-api --simplified` equivalent; auto-trait and derived impls
/// are kept so `Send`/`Sync` regressions show in the diff).
fn surface(workspace_root: &Path, package: &str, features: &[String]) -> Vec<String> {
    let mut builder = rustdoc_json::Builder::default()
        .toolchain(toolchain())
        .manifest_path(workspace_root.join("Cargo.toml"))
        .package(package)
        .quiet(true);
    if !features.is_empty() {
        builder = builder.features(features);
    }
    let json_path = builder.build().unwrap_or_else(|e| {
        panic!(
            "rustdoc JSON build failed for {package} (features {features:?}): {e}; \
             set ZEN_API_DOC=off to skip the public-API snapshot test"
        )
    });
    let api = public_api::Builder::from_rustdoc_json(json_path)
        .omit_blanket_impls(true)
        .sorted(true)
        .build()
        .unwrap_or_else(|e| {
            panic!(
                "public-api parse failed for {package}: {e}\n\
                 (usually a rustdoc JSON format mismatch between the '{}' \
                 toolchain and the rustdoc-types version public-api compiled \
                 against — update the toolchain, or pin one via the \
                 ZEN_API_DOC_TOOLCHAIN env var)",
                toolchain()
            )
        });
    api.items().map(|item| item.to_string()).collect()
}

fn snapshot_one(
    workspace_root: &Path,
    meta: &serde_json::Value,
    package: &str,
    extra: &Extra,
) -> String {
    let default_lines = surface(workspace_root, package, &[]);

    let (feature_label, extra_features) = match extra {
        Extra::None => (String::new(), Vec::new()),
        Extra::PublicFeatures => {
            let feats = public_features(meta, package);
            (feats.join(","), feats)
        }
        Extra::Pinned(csv) => (
            csv.clone(),
            csv.split(',').map(str::to_owned).collect::<Vec<_>>(),
        ),
    };

    let (delta_lines, removed_lines) = if extra_features.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let all_lines = surface(workspace_root, package, &extra_features);
        let default_set: HashSet<&str> = default_lines.iter().map(String::as_str).collect();
        let all_set: HashSet<&str> = all_lines.iter().map(String::as_str).collect();
        (
            all_lines
                .iter()
                .filter(|l| !default_set.contains(l.as_str()))
                .cloned()
                .collect(),
            default_lines
                .iter()
                .filter(|l| !all_set.contains(l.as_str()))
                .cloned()
                .collect(),
        )
    };

    let mut doc = format!(
        "# {package} public API surface\n\
         # Generated by zenutils-apidoc from a `cargo test` snapshot test\n\
         # (regenerated on every `cargo test`; ZEN_API_DOC=check verifies, =off skips).\n\
         # The features section is a DELTA: only lines added relative to the\n\
         # default-features section. Underscore-prefixed features are internal\n\
         # and excluded. Line counts are raw rustdoc item lines — see the\n\
         # summary block for the honest item taxonomy.\n\
         # DO NOT EDIT BY HAND — commit regenerated changes together with the code.\n\n"
    );
    doc.push_str(&render_summary(
        &default_lines,
        &delta_lines,
        removed_lines.len(),
    ));

    let _ = write!(
        doc,
        "\n## default features ({} lines)\n\n",
        default_lines.len()
    );
    for line in &default_lines {
        doc.push_str(line);
        doc.push('\n');
    }
    eprintln!(
        "{package} [default features]: {} lines",
        default_lines.len()
    );

    if !extra_features.is_empty() {
        let _ = write!(
            doc,
            "\n## added by non-default features: {feature_label} ({} lines)\n\n",
            delta_lines.len()
        );
        for line in &delta_lines {
            doc.push_str(line);
            doc.push('\n');
        }
        eprintln!(
            "{package} [+{feature_label}]: {} added lines",
            delta_lines.len()
        );
        if !removed_lines.is_empty() {
            let _ = write!(
                doc,
                "\n## removed by non-default features ({} lines)\n\n",
                removed_lines.len()
            );
            for line in &removed_lines {
                doc.push_str(line);
                doc.push('\n');
            }
            eprintln!(
                "{package} [+{feature_label}]: {} REMOVED lines (cfg(not) gate?)",
                removed_lines.len()
            );
        }
    }
    doc
}

// ---------------------------------------------------------------------------
// Line taxonomy: classify each public-api line so the summary reports honest
// counts instead of a raw line total.

/// Marker / auto traits whose impl lines are compiler-controlled plumbing
/// (still diff-guarded — losing `Send` is a semver break — but counted apart
/// from hand-written or derived trait impls).
const AUTO_TRAITS: &[&str] = &[
    "core::marker::Freeze",
    "core::marker::Send",
    "core::marker::StructuralPartialEq",
    "core::marker::Sync",
    "core::marker::Unpin",
    "core::marker::UnsafeUnpin",
    "core::panic::unwind_safe::RefUnwindSafe",
    "core::panic::unwind_safe::UnwindSafe",
];

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
    impls_auto: usize,
    impls_other: usize,
    reexports: usize,
    other: usize,
}

impl Tally {
    fn rows(&self) -> [(&'static str, usize); 12] {
        [
            ("pub modules", self.modules),
            ("pub types (struct/enum/trait/alias)", self.types),
            ("pub consts/statics", self.consts),
            ("pub macros", self.macros),
            ("free functions", self.free_fns),
            ("associated functions (methods)", self.assoc_fns),
            ("struct fields", self.fields),
            ("enum variants", self.variants),
            ("impl lines (auto traits)", self.impls_auto),
            ("impl lines (derived + manual)", self.impls_other),
            ("re-exports", self.reexports),
            ("other", self.other),
        ]
    }
}

/// For an `impl` line, return the part after `impl` / `impl<...>` — the
/// implemented trait (or inherent type). `None` if not an impl line.
fn impl_body(line: &str) -> Option<&str> {
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

/// `module` bucket for a path like `crate::module::Item::member` — the
/// second segment when it names a module (lowercase), else `(root)`.
fn module_of(path: &str) -> String {
    let segs = path_segments(path);
    if segs.len() >= 3 {
        let m = segs[1].split('<').next().unwrap_or(segs[1]);
        if m.chars()
            .next()
            .is_some_and(|c| c.is_lowercase() || c == '_')
        {
            return m.to_owned();
        }
    }
    "(root)".to_owned()
}

fn classify(line: &str, tally: &mut Tally, per_module: &mut BTreeMap<String, usize>) {
    let l = strip_attrs(line);
    if let Some(rest) = impl_body(l) {
        // Impl lines carry no `pub` path; not attributed to a module.
        if AUTO_TRAITS.iter().any(|t| rest.starts_with(t)) {
            tally.impls_auto += 1;
        } else {
            tally.impls_other += 1;
        }
        return;
    }
    let Some(body) = l.strip_prefix("pub ") else {
        tally.other += 1;
        return;
    };
    // Track the module bucket for every pub line.
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

fn tally_section(lines: &[String]) -> (Tally, BTreeMap<String, usize>) {
    let mut tally = Tally::default();
    let mut per_module = BTreeMap::new();
    for line in lines {
        classify(line, &mut tally, &mut per_module);
    }
    (tally, per_module)
}

fn render_summary(
    default_lines: &[String],
    delta_lines: &[String],
    removed_count: usize,
) -> String {
    let (dt, dmods) = tally_section(default_lines);
    let (ft, fmods) = tally_section(delta_lines);
    let mut s = String::from("## summary\n#\n");
    let _ = writeln!(s, "# {:<38} {:>8} {:>10}", "kind", "default", "+features");
    let _ = writeln!(
        s,
        "# {:<38} {:>8} {:>10}",
        "lines total",
        default_lines.len(),
        delta_lines.len()
    );
    for ((label, d), (_, f)) in dt.rows().into_iter().zip(ft.rows()) {
        if d == 0 && f == 0 {
            continue;
        }
        let _ = writeln!(s, "#   {label:<36} {d:>8} {f:>10}");
    }
    if removed_count > 0 {
        let _ = writeln!(
            s,
            "#   {:<36} {:>8} {:>10}",
            "removed by features", "-", removed_count
        );
    }
    s.push_str("#\n# per-module pub lines (default + feature-additions):\n");
    let modules: BTreeMap<&str, (usize, usize)> = dmods
        .iter()
        .map(|(m, n)| (m.as_str(), (*n, 0)))
        .chain(fmods.iter().map(|(m, n)| (m.as_str(), (0, *n))))
        .fold(BTreeMap::new(), |mut acc, (m, (d, f))| {
            let e = acc.entry(m).or_insert((0, 0));
            e.0 += d;
            e.1 += f;
            acc
        });
    for (module, (d, f)) in modules {
        let _ = writeln!(s, "#   {module:<24} {d:>6} +{f}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_taxonomy() {
        let lines: Vec<String> = [
            "pub mod demo",
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
            "impl core::marker::Send for demo::util::Thing",
            "impl<'a> core::marker::Sync for demo::Ref<'a>",
            "impl core::clone::Clone for demo::util::Thing",
            "#[non_exhaustive] pub struct demo::Opts",
            "pub fn demo::Generic<T>::with(T) -> Self",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let (t, mods) = tally_section(&lines);
        assert_eq!(t.modules, 2);
        assert_eq!(t.types, 3); // Thing, Kind, Opts
        assert_eq!(t.fields, 1);
        assert_eq!(t.variants, 2);
        assert_eq!(t.free_fns, 2); // helper, root_fn
        assert_eq!(t.assoc_fns, 2); // Thing::method, Generic::with
        assert_eq!(t.consts, 1);
        assert_eq!(t.impls_auto, 2); // Send + generic Sync
        assert_eq!(t.impls_other, 1); // Clone
        assert_eq!(t.other, 0);
        // `pub mod demo::util` itself buckets to "(root)" — a module
        // declaration is owned by its parent; only items *inside* count.
        assert_eq!(mods.get("util"), Some(&4));
        assert!(mods.contains_key("(root)"));
    }

    #[test]
    fn generic_receiver_is_assoc_fn() {
        let lines: Vec<String> =
            vec!["pub fn demo::fetch::Cached<demo::fetch::Shell>::new() -> Self".to_owned()];
        let (t, _) = tally_section(&lines);
        assert_eq!(t.assoc_fns, 1);
        assert_eq!(t.free_fns, 0);
    }
}
