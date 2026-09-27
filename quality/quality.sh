#!/usr/bin/env bash
# quality.sh — advisory code-quality sweep for a zen repo. NEVER fails a build;
# it reports. (Per user policy 2026-09-27: lint findings fix forward, they do
# not gate CI.)
#
#   quality.sh [--root REPO] [--quick] [--jobs N]
#
#   --root   repo to scan (default: cwd; auto-detects the git/jj root)
#   --quick  skip the compile-heavy stages (clippy, deny build metadata)
#   --jobs   parallelism hint forwarded to cargo (default: nproc-2)
#
# Stages, each independent — a missing tool prints NOTE and moves on:
#   fmt       cargo fmt --check
#   clippy    rustc+clippy warning census (json -> per-lint counts)
#   exposure  api-report.py: surface size, YAGNI/exposure flags, orphans
#   docs      check-stale-docs.py: dead links/refs/recipes in *.md
#   unused    cargo shear (or machete): unused dependencies
#   deny      cargo deny check: advisories + dup versions (needs deny.toml)
#   typos     typos-cli on tracked text (needs _typos.toml or typos config)
#   complex   cargo crap / cccc: worst complexity hotspots
#   shell     shellcheck on scripts/**/*.sh
#
# Adopt in a consumer repo: see quality/README.md ("Adopting the kit").
set -uo pipefail
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT="" ; QUICK=0 ; JOBS=$(( $(nproc 2>/dev/null || echo 8) - 2 ))
while [ $# -gt 0 ]; do
  case "$1" in
    --root)  ROOT="$2"; shift 2 ;;
    --quick) QUICK=1; shift ;;
    --jobs)  JOBS="$2"; shift 2 ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    *) ROOT="$1"; shift ;;
  esac
done
ROOT=$(cd "${ROOT:-.}" && git rev-parse --show-toplevel 2>/dev/null || pwd)
cd "$ROOT" || exit 2

have() { command -v "$1" >/dev/null 2>&1 || cargo "$1" --version >/dev/null 2>&1; }
sec()  { printf '\n\033[1m== %s ==\033[0m\n' "$*"; }
note() { printf '  NOTE: %s\n' "$*"; }
summary=""

sec "fmt — cargo fmt --check"
if cargo fmt --version >/dev/null 2>&1; then
  if cargo fmt --check >/dev/null 2>&1; then echo "  clean"; summary="$summary fmt:clean"
  else
    n=$(cargo fmt --check --verbose 2>/dev/null | grep -c '^Diff in' || true)
    echo "  $n file(s) would reformat — run \`cargo fmt\`"; summary="$summary fmt:$n"
  fi
else note "rustfmt not installed"; fi

if [ "$QUICK" = 0 ]; then
sec "clippy — warning census (advisory)"
  if cargo clippy --version >/dev/null 2>&1; then
    tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
    # --locked: a read-only sweep must never rewrite Cargo.lock (zenmetrics
    # rule: the lock is regenerated only via scripts/ci/lock.sh).
    cargo clippy --locked --workspace --all-targets --message-format json \
        -j "$JOBS" 2>/dev/null \
      | python3 - "$tmp" <<'PY'
import json, sys, collections
warns = collections.Counter(); errors = 0; files = set()
for line in sys.stdin:
    try: m = json.loads(line)
    except Exception: continue
    if m.get("reason") != "compiler-message": continue
    msg = m["message"]
    if msg["level"] == "error": errors += 1
    if msg["level"] != "warning": continue
    code = (msg.get("code") or {}).get("code") or "rustc/other"
    warns[code] += 1
    for sp in msg.get("spans", []):
        if sp.get("is_primary"): files.add(sp["file_name"])
tot = sum(warns.values())
print(f"  {tot} warnings across {len(files)} files" +
      (f", {errors} errors" if errors else ""))
for k, v in warns.most_common(15):
    print(f"    {v:>5}  {k}")
open(sys.argv[1], "w").write(str(tot))
PY
    summary="$summary clippy:$(cat "$tmp" 2>/dev/null || echo '?')warn"
  else note "cargo-clippy missing"; fi
fi

sec "api surface — exposure & YAGNI inventory"
python3 "$HERE/api-report.py" "$ROOT" 2>/dev/null | tail -40 || \
  note "api-report.py failed"

sec "docs — dead references"
python3 "$HERE/check-stale-docs.py" "$ROOT" --quiet 2>/dev/null | tail -8 || \
  note "check-stale-docs.py failed"

sec "unused deps"
if cargo shear --version >/dev/null 2>&1; then
  cargo shear 2>&1 | tail -15; summary="$summary shear:run"
elif cargo machete --version >/dev/null 2>&1; then
  cargo machete 2>&1 | tail -15; summary="$summary machete:run"
else note "install cargo-shear (cargo binstall cargo-shear) to enable"; fi

if [ "$QUICK" = 0 ]; then
sec "cargo deny"
  if [ -f deny.toml ] && cargo deny --version >/dev/null 2>&1; then
    cargo deny --locked check 2>&1 | tail -20; summary="$summary deny:run"
  else note "no deny.toml or cargo-deny missing"; fi
fi

sec "typos"
if command -v typos >/dev/null 2>&1; then
  typos --format brief 2>/dev/null | tail -15; summary="$summary typos:run"
else note "install typos-cli (cargo binstall typos-cli) to enable"; fi

sec "complexity hotspots"
if cargo crap --version >/dev/null 2>&1; then
  cargo crap --top 12 2>/dev/null | tail -15 || note "crap run failed"
elif command -v cccc >/dev/null 2>&1; then
  cccc . --lang rust --top 12 2>/dev/null | tail -15 || note "cccc failed"
else note "install cargo-crap or cccc for per-function complexity"; fi

sec "shellcheck"
if command -v shellcheck >/dev/null 2>&1 && [ -d scripts ]; then
  find scripts -name '*.sh' -print0 | xargs -0 shellcheck -S warning \
    2>/dev/null | tail -20
else note "shellcheck or scripts/ absent"; fi

printf '\n\033[1m== SUMMARY ==\033[0m%s\n' "$summary"
echo "advisory only — nothing here gates CI; fix forward."
exit 0
