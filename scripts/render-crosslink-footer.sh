#!/usr/bin/env sh
# Render the canonical "zen* image toolkit" crosslink footer from docs/zen-crates.tsv.
#
# Usage:
#   scripts/render-crosslink-footer.sh [--self CRATE] [--tsv PATH]
#
# --self CRATE   bold the current crate in the list and omit its own link-def
#                (so a crate doesn't self-link in its own footer).
# --tsv PATH     registry path (default: docs/zen-crates.tsv next to this script).
#
# Emits a Markdown block to stdout: a "## The zen* image toolkit" heading, one
# line per group with inline links, then the link-reference definitions. Paste
# (or splice) it at the bottom of a crate's README, replacing any prior footer.
#
# Apply to a README in place (replaces the existing footer + its link-defs):
#   scripts/render-crosslink-footer.sh --self zenpng | scripts/splice-footer.sh path/to/README.md
set -eu

SELF=""
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TSV="$HERE/../docs/zen-crates.tsv"
while [ $# -gt 0 ]; do
  case "$1" in
    --self) SELF="$2"; shift 2 ;;
    --tsv)  TSV="$2";  shift 2 ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[ -f "$TSV" ] || { echo "registry not found: $TSV" >&2; exit 1; }

awk -F'\t' -v self="$SELF" '
  BEGIN {
    ng = split("codecs internals compression process color framework metrics ml tools", gord, " ")
    label["codecs"]="Codecs"; label["internals"]="Codec internals"
    label["compression"]="Compression"; label["process"]="Resize & process"
    label["color"]="Pixels & color"; label["framework"]="Pipeline & framework"
    label["metrics"]="Perceptual metrics"; label["ml"]="Pickers & ML"
    label["tools"]="Benchmarking & tools"
  }
  /^#/ || NF<3 { next }
  {
    name=$1; grp=$2; repo=$3
    items[grp] = items[grp] (items[grp]==""?"":" \xC2\xB7 ") (name==self ? "**" name "**" : "[" name "]")
    if (name != self) { defs = defs "[" name "]: " repo "\n" }
  }
  END {
    print "## The zen* image toolkit"
    print ""
    if (self != "")
      print "`" self "` is part of the **zen\\*** image toolkit from [Imazen](https://imazen.io) \xE2\x80\x94 pure-Rust, `#![forbid(unsafe_code)]` crates for decoding, processing, measuring, and encoding images."
    else
      print "The **zen\\*** image toolkit from [Imazen](https://imazen.io) \xE2\x80\x94 pure-Rust, `#![forbid(unsafe_code)]` crates for decoding, processing, measuring, and encoding images."
    print ""
    for (i=1;i<=ng;i++) { g=gord[i]; if (items[g]!="") print "**" label[g] "** \xE2\x80\x94 " items[g] "  " }
    print ""
    printf "%s", defs
  }
' "$TSV"
