#!/usr/bin/env sh
# Render the canonical zen* crosslink footer from docs/zen-crates.tsv.
#
# Usage:
#   scripts/render-crosslink-footer.sh [--self CRATE] [--tsv PATH]
#
# --self CRATE   bold the current crate in the list and omit its own link-def
#                (so a crate doesn't self-link in its own footer).
# --tsv PATH     registry path (default: docs/zen-crates.tsv next to this script).
#
# Emits a Markdown block to stdout: an "## Image tech I maintain" table (image
# crates grouped, plus the Imageflow/ImageResizer products), a "General Rust
# awesomeness" tools line, profile links, and all link-reference definitions.
# Paste/splice it at the bottom of a crate's README, replacing any prior footer.
#
#   scripts/render-crosslink-footer.sh --self zenpng | scripts/splice-footer.sh README.md
set -eu

SELF=""
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TSV="$HERE/../docs/zen-crates.tsv"
while [ $# -gt 0 ]; do
  case "$1" in
    --self) SELF="$2"; shift 2 ;;
    --tsv)  TSV="$2";  shift 2 ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done
[ -f "$TSV" ] || { echo "registry not found: $TSV" >&2; exit 1; }

awk -F'\t' -v self="$SELF" '
  function cell(g) { return (items[g]=="") ? "—" : items[g] }
  BEGIN {
    label["codecs"]="**Codecs** \xC2\xB9"; label["internals"]="Codec internals"
    label["compression"]="Compression"; label["process"]="Processing"
    label["color"]="Pixels \x26 color"; label["framework"]="Pipeline \x26 framework"
    label["metrics"]="Metrics"; label["ml"]="Pickers \x26 ML"
    SEP=" \xC2\xB7 "
  }
  /^#/ || NF<3 { next }
  {
    name=$1; grp=$2; repo=$3
    token = (name==self ? "**" name "**" : "[" name "]")
    items[grp] = items[grp] (items[grp]==""?"":SEP) token
    if (name != self) defs = defs "[" name "]: " repo "\n"
  }
  END {
    print "## Image tech I maintain"
    print ""
    print "| | |"
    print "|:--|:--|"
    nimg = split("codecs internals compression process color framework metrics ml", ord, " ")
    for (i=1;i<=nimg;i++) { g=ord[i]; if (items[g]!="") print "| " label[g] " | " cell(g) " |" }
    print "| Products | [Imageflow] image engine ([.NET][imageflow-dotnet]" SEP "[Node][imageflow-node]" SEP "[Go][imageflow-go])" SEP "[Imageflow Server]" SEP "[ImageResizer] (C#) |"
    print ""
    print "<sub>\xC2\xB9 pure-Rust, `#![forbid(unsafe_code)]` codecs, as of 2026</sub>"
    print ""
    print "### General Rust awesomeness"
    print ""
    print (items["tools"]=="" ? "—" : items["tools"])
    print ""
    print "[Open source](https://www.imazen.io/open-source)" SEP "[@imazen](https://github.com/imazen)" SEP "[@lilith](https://github.com/lilith)" SEP "[lib.rs/~lilith](https://lib.rs/~lilith)"
    print ""
    printf "%s", defs
    print "[Imageflow]: https://github.com/imazen/imageflow"
    print "[Imageflow Server]: https://github.com/imazen/imageflow-dotnet-server"
    print "[ImageResizer]: https://github.com/imazen/resizer"
    print "[imageflow-dotnet]: https://github.com/imazen/imageflow-dotnet"
    print "[imageflow-node]: https://github.com/imazen/imageflow-node"
    print "[imageflow-go]: https://github.com/imazen/imageflow-go"
  }
' "$TSV"
