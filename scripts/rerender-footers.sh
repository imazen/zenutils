#!/usr/bin/env bash
# Re-render the crosslink footer into every README that carries one.
#
#   scripts/rerender-footers.sh [--apply] [--commit] [--push] [ROOT ...]
#
# Default is a DRY RUN: it reports which READMEs would change and how, and
# touches nothing. --apply writes the files; --commit also commits per repo;
# --push also pushes. Each stage is opt-in because this spans ~50 repositories.
#
# For each README carrying "## Image tech I maintain" it renders the footer with
# --self set to the crate the README belongs to (the [package] name from the
# nearest Cargo.toml, else the directory name), splices it in, and regenerates
# README.crates.md when one exists beside it.
#
# Run scripts/check-crosslink-targets.sh FIRST — this copies whatever the
# registry says into ~100 files, so a dead target multiplies.
set -uo pipefail
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
APPLY=0; COMMIT=0; PUSH=0; roots=()
while [ $# -gt 0 ]; do
  case "$1" in
    --apply)  APPLY=1; shift ;;
    --commit) APPLY=1; COMMIT=1; shift ;;
    --push)   APPLY=1; COMMIT=1; PUSH=1; shift ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) roots+=("$1"); shift ;;
  esac
done
[ ${#roots[@]} -eq 0 ] && roots=("$(cd "$HERE/../.." && pwd)")
[ "$APPLY" -eq 0 ] && echo "DRY RUN — nothing will be written (pass --apply to write)"

# A repo whose .workongoing was refreshed in the last 5 minutes belongs to
# another session; editing its worktree underneath it is the concurrent-edit
# clobber the marker exists to prevent.
claimed_elsewhere() {
  local m="$1/.workongoing" ts age
  [ -f "$m" ] || return 1
  ts=$(awk 'NR==1{print $1}' "$m")
  age=$(( $(date -u +%s) - $(date -j -u -f "%Y-%m-%dT%H:%M:%SZ" "$ts" +%s 2>/dev/null || echo 0) ))
  [ "$age" -lt 300 ] && [ "$age" -gt -300 ]
}

changed=0; same=0; skipped=0; repos=()
for root in "${roots[@]}"; do
  while IFS= read -r readme; do
    case "$readme" in *--*/*|*/target/*|*/node_modules/*) continue ;; esac
    grep -q '^## Image tech I maintain' "$readme" || continue
    dir=$(dirname "$readme")
    top=$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null)
    if [ -n "$top" ] && claimed_elsewhere "$top"; then
      printf "SKIP    %-58s (claimed: %s)\n" "${readme#$root/}" \
        "$(cut -d' ' -f2 "$top/.workongoing")"
      skipped=$((skipped+1)); continue
    fi
    self=$(awk '/^\[package\]/{p=1;next} /^\[/{p=0} p&&/^name *=/{gsub(/.*= *"|"/,"");print;exit}' \
             "$dir/Cargo.toml" 2>/dev/null)
    [ -n "$self" ] || self=$(basename "$dir")
    tmp="$readme.rerender.tmp"; cp "$readme" "$tmp"
    sh "$HERE/render-crosslink-footer.sh" --self "$self" \
      | sh "$HERE/splice-footer.sh" "$tmp" >/dev/null 2>&1
    if cmp -s "$readme" "$tmp"; then
      rm -f "$tmp"; same=$((same+1)); continue
    fi
    changed=$((changed+1))
    d=$(diff <(grep -c . "$readme") <(grep -c . "$tmp") >/dev/null && echo "" || echo " (line count changed)")
    printf "CHANGE  %-58s self=%s%s\n" "${readme#$root/}" "$self" "$d"
    if [ "$APPLY" -eq 1 ]; then
      mv "$tmp" "$readme"
      [ -f "$dir/README.crates.md" ] && sh "$HERE/gen-readme-crates.sh" "$dir" >/dev/null 2>&1
      r=$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null) && repos+=("$r")
    else
      rm -f "$tmp"
    fi
  done < <(find "$root" -maxdepth 4 -name 'README.md' -not -path '*/target/*' \
             -not -path '*/node_modules/*' -not -path '*/.git/*' 2>/dev/null)
done

echo "---"
echo "would change: $changed   already current: $same   skipped (claimed): $skipped"
[ "$APPLY" -eq 0 ] && exit 0

printf '%s\n' "${repos[@]}" | sort -u | while read -r r; do
  [ -n "$r" ] || continue
  git -C "$r" diff --quiet -- '*README*.md' && continue
  echo "== $(basename "$r")"
  if [ "$COMMIT" -eq 1 ]; then
    git -C "$r" add -- '*README*.md'
    git -C "$r" -c user.name="Lilith River" -c user.email="jill@imazen.io" \
      commit -q -m "docs(readme): re-render crosslink footer from the zenutils registry" \
      && echo "   committed"
    if [ "$PUSH" -eq 1 ]; then
      b=$(git -C "$r" symbolic-ref --quiet --short HEAD 2>/dev/null)
      if [ -n "$b" ]; then git -C "$r" push -q origin "$b" && echo "   pushed $b"
      else echo "   SKIP push: detached HEAD (jj repo — advance the bookmark yourself)"; fi
    fi
  fi
done
