#!/usr/bin/env sh
# Generate a trimmed README.crates.md (the crates.io README) from README.md (the
# GitHub README). This keeps the two surfaces in sync from ONE hand-edited file,
# so the split never drifts: you edit README.md; README.crates.md is regenerated.
#
# Transform:
#   1. H1 badge row -> strip ALL badges, keep just `# crate-name`. A crates.io
#      README is locked to the specific published version the reader is viewing,
#      but CI / crates.io-version / lib.rs / docs.rs badges all reflect HEAD /
#      latest — they are misleading on a version-pinned page. So crates.io gets
#      no badges; the GitHub README keeps the full row (GitHub is always HEAD).
#   2. Drop any block between `<!-- crates.io:skip-start -->` and
#      `<!-- crates.io:skip-end -->` (use it for heavy benchmark tables, large
#      images, or anything HEAD-specific / that renders poorly on crates.io).
#   3. Prepend a "generated — do not edit" banner.
# Everything else (intro, quick-start, usage, crosslink footer, License) passes
# through verbatim — note links/images in the kept sections MUST be absolute,
# because crates.io has no repo to resolve relative paths against.
#
# Usage:  scripts/gen-readme-crates.sh [CRATE_DIR]   (default: .)
# Then in Cargo.toml:  readme = "README.crates.md"
set -eu

DIR="${1:-.}"
SRC="$DIR/README.md"
OUT="$DIR/README.crates.md"
[ -f "$SRC" ] || { echo "no README.md in $DIR" >&2; exit 1; }

{
  printf '%s\n\n' '<!-- GENERATED FROM README.md by zenutils gen-readme-crates.sh — DO NOT EDIT. -->'
  awk '
    !hdr && /^# / {
      hdr=1
      p=index($0, " [![")
      print (p>0) ? substr($0,1,p-1) : $0
      next
    }
    /<!-- *crates\.io:skip-start *-->/ { skip=1; next }
    /<!-- *crates\.io:skip-end *-->/   { skip=0; next }
    skip { next }
    { print }
  ' "$SRC"
} > "$OUT"

echo "wrote $OUT"
