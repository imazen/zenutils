#!/usr/bin/env bash
# How far has each README's BODY fallen behind its code?
#
#   scripts/readme-drift.sh [ROOT ...]        (default: the dirs above zenutils)
#
# The crosslink footer is re-rendered across every repo at once, so "git log
# README.md" reports a fresh date for READMEs whose prose has not been touched in
# months. This ignores the footer: for each commit that touched README.md it
# hashes only the text ABOVE the "## Image tech I maintain" heading, and reports
# the last commit where that text actually changed. Columns after it are what has
# landed since: commits, distinct .rs files, and CHANGELOG version headings.
#
# Sorted worst-first. High RSFILES with an old BODY is the signal — the README is
# describing an API that has moved.
set -u
NOW=$(date +%s)
roots=("$@")
[ ${#roots[@]} -eq 0 ] && roots=("$(cd "$(dirname "$0")/../.." && pwd)")

printf "%-24s %-11s %-5s %-7s %-7s %s\n" REPO BODY_LAST DAYS COMMITS RSFILES RELEASES_SINCE
for root in "${roots[@]}"; do
  for d in "$root"/*/; do
    [ -d "$d/.git" ] || continue
    [ -f "$d/README.md" ] || continue
    case "$d" in *--*) continue;; esac   # scratch worktrees
    mapfile -t commits < <(git -C "$d" log --format=%H -- README.md 2>/dev/null)
    [ ${#commits[@]} -gt 0 ] || continue
    prev=""; last=""; found=""
    for c in "${commits[@]}"; do
      h=$(git -C "$d" show "$c:README.md" 2>/dev/null \
          | awk '/^## Image tech I maintain/{exit} {print}' | shasum | cut -d' ' -f1)
      if [ -n "$prev" ] && [ "$h" != "$prev" ]; then found="$last"; break; fi
      prev="$h"; last="$c"
    done
    [ -n "$found" ] || found="${commits[-1]}"
    ts=$(git -C "$d" log -1 --format=%ct "$found")
    days=$(( (NOW - ts) / 86400 ))
    n=$(git -C "$d" rev-list --count HEAD --not "$found" 2>/dev/null)
    rs=$(git -C "$d" log --name-only --format= "$found..HEAD" -- '*.rs' 2>/dev/null | sort -u | grep -c .)
    rel=$(git -C "$d" log "$found..HEAD" -p --format= -- CHANGELOG.md 2>/dev/null \
          | grep -E '^\+#+ +\[?[0-9]' | sed 's/^\+#* *//;s/ *-.*//' | sort -u | tr '\n' ' ')
    printf "%-24s %-11s %-5s %-7s %-7s %s\n" \
      "$(basename "$d")" "$(git -C "$d" log -1 --format=%cs "$found")" "$days" "$n" "$rs" "${rel:-—}"
  done
done | { read -r hdr; echo "$hdr"; sort -k5 -nr; }
