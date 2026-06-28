# zen* README & onboarding conventions

The single source of truth for how every zen-native crate presents itself on
GitHub and crates.io. Apply this to all zen-native published crates (codecs,
processing, metrics, pixels/color, compression, framework, ML, tools). Upstream
forks we don't own (`fax`, `tiff`, `weezl`, `rawloader`, `cavif`,
`aom-decoder-rs`, `mozjpeg-rs`) are out of scope — don't inject our conventions
into them.

Tooling lives next to this doc:

- `docs/zen-crates.tsv` — the crate registry (edit here when the family changes).
- `scripts/render-crosslink-footer.sh --self <crate>` — renders the footer.
- `scripts/gen-readme-crates.sh <crate-dir>` — regenerates `README.crates.md`.
- `scripts/splice-footer.sh <README>` — splices a rendered footer in place.

---

## 1. Two files per crate: GitHub vs crates.io

Each crate keeps **two** READMEs:

| File | Surface | Badges | Heavy content |
|------|---------|--------|---------------|
| `README.md` | GitHub repo home | full badge row | benchmarks, images, deep usage |
| `README.crates.md` | crates.io (`readme = "README.crates.md"`) | **none** | trimmed |

`README.crates.md` is **generated** from `README.md` — never hand-edited — so the
split can't drift:

```sh
zenutils/scripts/gen-readme-crates.sh .      # writes ./README.crates.md
```

Set it in `Cargo.toml`:

```toml
[package]
readme = "README.crates.md"
```

Two rules make the generator deterministic:

1. **Mark GitHub-only sections** in `README.md` so they're dropped on crates.io:

   ```markdown
   <!-- crates.io:skip-start -->
   ## Benchmarks
   ...big table / chart images...
   <!-- crates.io:skip-end -->
   ```

2. **All links and images in kept sections must be absolute.** crates.io has no
   repo to resolve `./docs/foo.png` or `[x](src/lib.rs)` against — relative
   targets 404 there. Use `https://github.com/imazen/<crate>/blob/main/...` or
   `https://docs.rs/<crate>`.

Why no badges on crates.io: a crates.io README is **locked to the specific
published version** the reader is viewing, but every badge (CI, crates.io
version, lib.rs, docs.rs) reflects HEAD / latest — so on a version-pinned page
they are at best redundant (version/license/docs already sit in the sidebar) and
at worst misleading (the CI badge for an old version shows a newer commit's
status). Anything not locked to the version being read doesn't belong there. The
GitHub README keeps the full badge row because GitHub always renders HEAD, which
is exactly what those badges describe. Same logic applies to HEAD-only nav (e.g.
a "latest docs site" link) — wrap it in `crates.io:skip` if it would mislead on a
pinned page.

Packaging gotchas (check before changing `Cargo.toml`):

- If `[package]` has an `include = [...]` whitelist, replace `"/README.md"` with
  `"/README.crates.md"` so the registry README actually ships. If it uses
  `exclude` instead (or neither), nothing to change — both READMEs ship and the
  `readme` field picks which one crates.io renders.
- **If `lib.rs` does `include_str!("../README.md")`** (common for README doctests
  or `#![doc = include_str!(...)]`), README.md must stay in the published tarball
  — keep it in `include` (add `README.crates.md` alongside it; don't replace).
  Removing it breaks the docs.rs build.

---

## 2. Badges

Inline, on the same line as the `# crate-name` H1. **Always `?style=flat-square`.**
Route every badge through shields.io for consistent height. **Omit the `branch=`
query param** so the CI badge follows each repo's default branch (some repos are
`main`, some `master`).

**GitHub `README.md` — full row, in this order:**

```markdown
# <crate> [![CI](https://img.shields.io/github/actions/workflow/status/imazen/<repo>/ci.yml?style=flat-square&label=CI)](https://github.com/imazen/<repo>/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/<crate>?style=flat-square)](https://crates.io/crates/<crate>) [![lib.rs](https://img.shields.io/crates/v/<crate>?style=flat-square&label=lib.rs&color=blue)](https://lib.rs/crates/<crate>) [![docs.rs](https://img.shields.io/docsrs/<crate>?style=flat-square)](https://docs.rs/<crate>) [![license](https://img.shields.io/crates/l/<crate>?style=flat-square)](#license)
```

Optional extras, after `license`, only if the crate actually has them: MSRV
(`https://img.shields.io/badge/MSRV-<ver>-blue?style=flat-square`) and codecov
(`https://img.shields.io/codecov/c/github/imazen/<repo>?style=flat-square`).

**crates.io `README.crates.md` — no badges at all** (the generator strips the
whole badge row, leaving just `# crate-name`; nothing to hand-write).

Custom-license crates (e.g. AGPL/commercial dual) use a badge label that states
it, e.g. `https://img.shields.io/badge/license-AGPL--3.0%20%2F%20Commercial-blue?style=flat-square`.

---

## 3. Crosslink footer

Every README ends with the same footer, rendered from the registry so it never
goes stale:

```sh
zenutils/scripts/render-crosslink-footer.sh --self <crate> >> README.md
# or splice over an old footer:
zenutils/scripts/render-crosslink-footer.sh --self <crate> | zenutils/scripts/splice-footer.sh README.md
```

It renders an `## Image tech I maintain` section: a two-column table grouping the
image crates (Codecs / Codec internals / Compression / Processing / Pixels &
color / Pipeline & framework / Metrics / Pickers & ML) plus a Products row
(Imageflow / Imageflow Server / ImageResizer), a `### General Rust awesomeness`
line for the tools, and profile links. `--self <crate>` bolds the current crate
(no self-link) and omits its own link-def. The footer links to **repos**
(`github.com/imazen/<repo>`) — repo links behave identically on both surfaces and
aren't circular on crates.io. When the family gains or loses a crate, edit
`docs/zen-crates.tsv` and re-render every footer. See `docs/crosslink-footer.md`
for the current rendered block.

Hub crates (`zencodec`, `zencodecs`, `zenpipe`) may additionally carry a
format-specific table higher up; the rendered footer is still required.

---

## 4. README skeleton

```
# <crate> <badges>

<one-paragraph intro: what it is, what's special, key guarantees
 (pure Rust, forbid(unsafe_code), no_std, SIMD)>

## Quick start
```toml
[dependencies]
<crate> = "X.Y"          # full version, never truncated
```
```rust
// the ONE-SHOT path — the core job in one call (see §5)
```

## <features / usage / API highlights>

<!-- crates.io:skip-start -->
## Benchmarks
<repro link + key chart; full methodology in benchmarks/>
<!-- crates.io:skip-end -->

## License
<SPDX or dual-license note>

## Image tech I maintain    <- rendered footer (always last)
```

---

## 5. One-shot onboarding functions

Every crate exposes a top-level free function that does its **core job in one
call**, with sane defaults, for someone who hasn't read the docs. It's purely
additive — the builder/config path stays as the power API.

Naming:

| Kind | Signature shape | Example |
|------|-----------------|---------|
| Encoder | `encode_<fmt>(pixels, w, h) -> Result<Vec<u8>>` | `zenpng::encode_rgba8(&rgba, w, h)?` |
| Encoder w/ quality | `encode_<fmt>_quality(pixels, w, h, q) -> Result<Vec<u8>>` | `zenjpeg::encode_rgb8_quality(&rgb, w, h, 85)?` |
| Decoder | `decode_rgba8(bytes) -> Result<(Vec<u8>, u32, u32)>` | `zengif::decode_rgba8(bytes)?` |
| Transform | `<verb>_rgba8(src, sw, sh, ...) -> Result<Vec<u8>>` | `zenresize::resize_rgba8(&src, sw, sh, dw, dh)?` |
| Metric | `<noun>(ref, dist, w, h) -> Result<f64>` | `fast_ssim2::score(&r, &d, w, h)?` |

Rules:
- Additive only — no signature changes to existing items (`cargo semver-checks`
  must stay clean for a patch/minor bump).
- Gate behind whatever features the crate already needs (`encode`, `std`, …).
- Carry a **doctest** that is the literal copy-paste in the README Quick start.
- Default to the most common pixel format (RGBA8, or the crate's natural unit);
  the builder path covers everything else.
- Honor the strided-buffer rule for multi-row ops — the one-shot may assume tight
  packing, but it must call the strided-correct primitive underneath.

---

## 6. Fair-benchmark docs

Benchmarks that compare against other crates/codecs live in
`benchmarks/<topic>_<YYYY-MM-DD>.md`, are committed, and **must be reproducible
from the file alone**. See `~/work/claudehints/topics/benchmarking.md` for the
integrity rules (no faked names, no `-C target-cpu=native`, no Kodak/gradient
overfit). New benches use **zenbench**, not criterion.

Every benchmark markdown MUST contain:

1. **Environment** — CPU model, RAM, OS, `rustc -V`, build profile. Built
   **without** `-C target-cpu=native` (runtime SIMD dispatch is what ships).
2. **Exact repro for this repo**
   ```sh
   git clone https://github.com/imazen/<repo> && cd <repo>
   git checkout <full-commit-sha>      # the commit these numbers came from
   <exact run command, e.g. cargo run --release -p <bench> -- ...>
   ```
3. **Exact repro for every competitor** — crate + pinned version, or
   `git clone <url> && git checkout <sha>` + build command. Pin the commit; never
   "latest".
4. **Threading mode, stated explicitly** — single-thread (`RAYON_NUM_THREADS=1`
   / feature off) vs N-thread, and the thread count. Never compare single-thread
   A against multi-thread B. Show both modes when a crate offers both.
5. **IO is excluded from the timed region** — load corpus bytes/pixels into RAM
   *before* the measured loop; decode from `&[u8]`, encode into `Vec<u8>`; no
   file open/read/write inside the timed closure. Consume the output (hash/sum)
   so it isn't optimized away.
6. **Apples-to-apples inputs** — same images, same dimensions, same pixel format,
   same quality/effort target across all contenders; say so.
7. **The right chart** (see §7) plus a one-line statement of the decision it
   supports.

zenbench output modes to use:

| Want | Mode |
|------|------|
| sorted throughput bar chart in the terminal | zenbench CLI (`sort_by_speed`) |
| self-contained SVG report | `--format=html` (`to_html()`) |
| standalone SVG charts | `charts` feature (charts-rs) |
| embeddable chart image URLs | `quickchart` module |
| paired A/B delta with CI | zenbench's paired-difference stats |

(zenbench does not produce violin/PDF/regression plots itself — for those, export the
raw per-call samples and plot them with your tool of choice.)

---

## 7. Choosing the chart (for developer decisions)

| Question a developer is asking | Chart | Notes |
|--------------------------------|-------|-------|
| "Which is fastest?" | horizontal **bar**, sorted by throughput (MP/s or MB/s) | one bar per contender; separate series for 1-thread vs N-thread |
| "Speed vs quality/size (codecs)?" | **RD / Pareto scatter**: x = bpp or bytes, y = SSIMULACRA2 / butteraugli / zensim | one line per codec swept across quality; show the frontier |
| "Is the A/B delta real / how noisy?" | **violin** or PDF of per-call times; or paired-difference CI | distribution beats a single mean |
| "How does it scale with image size?" | **line/grouped bar**, x = pixels (log) | fit `total = α + β·pixels`; report intercept (fixed overhead) AND slope |
| "Memory?" | reported from heaptrack / `time -v`, never extrapolated | measure each size |

Avoid pie charts, 3D, and dual-axis plots — they obscure the comparison.

---

## 8. Per-crate checklist

- [ ] H1 badge row matches §2 (flat-square, correct order, no `branch=`).
- [ ] One-paragraph intro + **Quick start** using the one-shot fn (§5).
- [ ] One-shot fn added (additive), feature-gated, with a doctest.
- [ ] Body reflects the **current** API — reconciled against CHANGELOG entries and
      GitHub releases since the last README overhaul (docs lie; trace source).
- [ ] Benchmarks (if any) follow §6 and link to `benchmarks/…md`; heavy tables
      wrapped in `crates.io:skip` markers.
- [ ] Crosslink footer rendered with `--self` (§3), placed last.
- [ ] `README.crates.md` regenerated; `readme = "README.crates.md"` in Cargo.toml.
- [ ] `cargo semver-checks` clean for the intended bump.
