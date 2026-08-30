#!/usr/bin/env sh
# Verify every repo the crosslink footer links to is still public and unarchived.
#
#   scripts/check-crosslink-targets.sh [--tsv PATH]
#
# A private or archived target renders as a 404 for readers, and the footer ships
# in ~125 READMEs — one dead repo is 125 dead links. Requires `gh` (authenticated).
# Exits nonzero and lists offenders if any target is private, archived, or missing.
set -eu
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TSV="$HERE/../docs/zen-crates.tsv"
[ "${1:-}" = "--tsv" ] && TSV="$2"
[ -f "$TSV" ] || { echo "registry not found: $TSV" >&2; exit 1; }

# repos from the registry, plus the hard-coded product links in the renderer
{ awk -F'\t' '!/^#/ && NF>=3 { print $3 }' "$TSV"
  printf '%s\n' \
    https://github.com/imazen/imageflow \
    https://github.com/imazen/imageflow-dotnet-server \
    https://github.com/imazen/resizer \
    https://github.com/imazen/imageflow-dotnet \
    https://github.com/imazen/imageflow-node \
    https://github.com/imazen/imageflow-go
} | sed 's|https://github.com/||' | sort -u > "$TSV.targets.tmp"

bad=0
while read -r slug; do
  [ -n "$slug" ] || continue
  info=$(gh api "repos/$slug" --jq '"\(.visibility) \(.archived)"' 2>/dev/null) || {
    echo "MISSING   $slug (404 or no access)"; bad=1; continue; }
  vis=${info%% *}; arch=${info##* }
  [ "$vis" = "public" ]  || { echo "PRIVATE   $slug"; bad=1; }
  [ "$arch" = "false" ]  || { echo "ARCHIVED  $slug"; bad=1; }
done < "$TSV.targets.tmp"
rm -f "$TSV.targets.tmp"

[ "$bad" -eq 0 ] && echo "all crosslink targets public and unarchived"
exit "$bad"
