#!/usr/bin/env sh
# Splice a freshly-rendered crosslink footer into a README, in place.
# Reads the new footer from stdin; rewrites the file at $1.
#
#   scripts/render-crosslink-footer.sh --self zenpng | scripts/splice-footer.sh zenpng/README.md
#
# Idempotent: if the README already ends with a "## The zen* image toolkit"
# section, everything from that heading to EOF is replaced. First-time, it strips
# a trailing contiguous block of link-reference definitions (the legacy footer)
# if one is at EOF, then appends. If it can't cleanly find either, it appends and
# prints a warning so you can remove a stray legacy footer by hand.
set -eu
F="${1:?usage: splice-footer.sh README.md  (footer on stdin)}"
[ -f "$F" ] || { echo "no such file: $F" >&2; exit 1; }
NEW=$(cat)
TMP="$F.splice.tmp"

MARK=$(grep -n '^## Image tech I maintain' "$F" | head -1 | cut -d: -f1 || true)
if [ -n "${MARK:-}" ]; then
  head -n $((MARK-1)) "$F" | awk 'BEGIN{} {lines[NR]=$0} END{n=NR; while(n>0 && lines[n]~/^[[:space:]]*(---)?[[:space:]]*$/) n--; for(i=1;i<=n;i++) print lines[i]}' > "$TMP"
  printf '\n%s\n' "$NEW" >> "$TMP"
  mv "$TMP" "$F"; echo "replaced existing footer in $F"; exit 0
fi

# First-time: strip a trailing run of link-defs (+ blanks), if present at EOF.
STRIPPED=$(awk '{lines[NR]=$0} END{
  n=NR
  while(n>0 && (lines[n]~/^[[:space:]]*$/ || lines[n]~/^\[[^]]+\]:[[:space:]]/)) n--
  # require that the trailing run actually contained at least one link-def
  hasdef=0; for(i=n+1;i<=NR;i++) if(lines[i]~/^\[[^]]+\]:[[:space:]]/) hasdef=1
  for(i=1;i<=n;i++) print lines[i]
  print (hasdef?"__STRIPPED__":"__NONE__") > "/dev/stderr"
}' "$F" 2> "$F.flag")
FLAG=$(cat "$F.flag"; rm -f "$F.flag")
printf '%s\n' "$STRIPPED" | awk '{lines[NR]=$0} END{n=NR; while(n>0 && lines[n]~/^[[:space:]]*$/) n--; for(i=1;i<=n;i++) print lines[i]}' > "$TMP"
printf '\n%s\n' "$NEW" >> "$TMP"
mv "$TMP" "$F"
if [ "$FLAG" = "__STRIPPED__" ]; then echo "stripped legacy link-defs and appended footer in $F"
else echo "WARNING: no legacy footer detected in $F — appended new footer; check for a stray old one" >&2; fi
