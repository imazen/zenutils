#!/usr/bin/env bash
# Check every zen-native README against docs/readme-conventions.md.
#
#   scripts/readme-lint.sh [ROOT ...]        (default: the dirs above zenutils)
#
# Mechanical checks only — it verifies structure, never judges prose. One line
# per README with the checks it fails, then a summary of which check fails most
# often (that is the one worth a batch fix).
#
#   BADGE   H1 lacks the flat-square badge row (section 2)
#   BRANCH  a badge pins branch=, so it breaks on repos whose default differs
#   FACTS   no facts line under the intro: no_std / unsafe / MSRV / platforms (4a)
#   STATUS  no Status section stating maturity and what is not supported (4b)
#   QUICK   no Quick start section (section 4)
#   FOOTER  no crosslink footer (section 3)
#   STALE   footer differs from what the registry renders today (section 3)
#   CRATES  no README.crates.md beside a published crate's README (section 1)
#   RDME    Cargo.toml does not point readme= at README.crates.md (section 1)
#   LONG    over 400 lines; depth belongs in docs/ (section 4e)
set -uo pipefail
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
roots=("$@"); [ ${#roots[@]} -eq 0 ] && roots=("$(cd "$HERE/../.." && pwd)")
declare -A tally
total=0; clean=0; internal=0

for root in "${roots[@]}"; do
  while IFS= read -r readme; do
    case "$readme" in *--*/*|*/target/*|*/node_modules/*) continue ;; esac
    dir=$(dirname "$readme")
    # zen-native only: must have a Cargo.toml with a package name
    self=$(awk '/^\[package\]/{p=1;next} /^\[/{p=0} p&&/^name *=/{gsub(/.*= *"|"/,"");print;exit}' \
             "$dir/Cargo.toml" 2>/dev/null)
    [ -n "$self" ] || continue
    # Internal crates (publish = false) have no public landing page, so the
    # badge/footer/crates.io rules do not apply to them.
    if grep -qE '^publish *= *false' "$dir/Cargo.toml" 2>/dev/null; then
      internal=$((internal+1)); continue
    fi
    total=$((total+1)); f=()

    grep -qE '^# .*img\.shields\.io.*flat-square' "$readme" || f+=(BADGE)
    grep -qE '^# .*img\.shields\.io[^)]*branch=' "$readme" && f+=(BRANCH)
    head -25 "$readme" | grep -qEi 'no_std|forbid\(unsafe_code\)|MSRV|unsafe' || f+=(FACTS)
    grep -qiE '^#+ +status\b' "$readme" || f+=(STATUS)
    grep -qiE '^#+ +(quick ?start|install)' "$readme" || f+=(QUICK)
    if grep -q '^## Image tech I maintain' "$readme"; then
      tmp="$readme.lint.tmp"; cp "$readme" "$tmp"
      sh "$HERE/render-crosslink-footer.sh" --self "$self" \
        | sh "$HERE/splice-footer.sh" "$tmp" >/dev/null 2>&1
      cmp -s "$readme" "$tmp" || f+=(STALE)
      rm -f "$tmp"
    else
      f+=(FOOTER)
    fi
    [ -f "$dir/README.crates.md" ] || f+=(CRATES)
    grep -qE '^readme *= *"README\.crates\.md"' "$dir/Cargo.toml" || f+=(RDME)
    [ "$(grep -c . "$readme")" -gt 400 ] && f+=(LONG)

    if [ ${#f[@]} -eq 0 ]; then clean=$((clean+1)); continue; fi
    for k in "${f[@]}"; do tally[$k]=$(( ${tally[$k]:-0} + 1 )); done
    printf "%-22s %s\n" "$self" "${f[*]}"
  done < <(find "$root" -maxdepth 4 -name 'README.md' -not -path '*/target/*' \
             -not -path '*/node_modules/*' -not -path '*/.git/*' 2>/dev/null | sort)
done

echo "---"
printf "%d publishable READMEs checked, %d fully clean (%d internal crates skipped)\n" "$total" "$clean" "$internal"
for k in "${!tally[@]}"; do printf "%6d  %s\n" "${tally[$k]}" "$k"; done | sort -rn
