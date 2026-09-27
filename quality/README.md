# zenutils/quality — the shared code-quality kit

One canonical copy of the org's advisory quality tooling. Design choices:

- **Lives in this repo, not copied per-repo** — the safe_push/hygiene
  mirror-copy pattern drifts; a cloneable repo versioned by git rev doesn't.
  Consumer repos resolve the kit at `../zenutils/quality` (the workspace
  layout), via `$ZENUTILS`, or via a pinned clone at `.quality-kit/`.
- **Advisory, never gating** (user policy 2026-09-27): checks report; they
  do not fail CI. The contract is that agents run `just quality` and warning
  counts shrink over time. Repos that *want* a gate get one by calling the
  same script with `--strict` (checkers that support it) in their own job.
- **Python 3 stdlib + bash only** for the checkers — no pip deps, so they
  run identically on dev boxes and bare CI runners.

## What's here

| Tool | What it does |
|---|---|
| `quality.sh` | Orchestrates the whole advisory sweep against a repo root. |
| `check-stale-docs.py` | Dead intra-repo markdown links, backticked paths to files that don't exist, `just <recipe>` names no justfile defines, dead `*.sh`/`*.py` basenames. Separates *living* docs from dated/RFC/HISTORICAL snapshots and append-only logs; refs named inside a deletion narrative ("was deleted 2026-…") downgrade to `MENTIONED`. `--quiet` hides informational classes, `--fix` rewrites living-doc refs whose basename resolves to exactly one repo path (never touches snapshots or dated-segment paths), `--strict` fails on living-doc hits, `--json`/`--living` for tooling. |
| `api-report.py` | Per-crate pub surface census: pub vs pub(crate) counts, pub fields, re-exports, undocumented items, unsealed traits, pub-in-bin, and **zero-consumer items** — pub items in `publish = false` crates that no in-workspace dependent names in source (the YAGNI/exposure flag). Merges committed `docs/public-api/*.txt` snapshots for authoritative published-surface counts. |
| `api-diff.py` | Diff of `docs/public-api/*.txt` between two git refs → markdown report of added/removed items + per-item policy flags. The PR bot's brain. |
| `templates/` | `clippy.toml`, `workspace-lints.toml` (the `[workspace.lints]` block + member `[lints] workspace = true` recipe), `deny.toml`, `_typos.toml`, `justfile.snippet`, `api-guard-caller.yml`, `CLAUDE-quality-block.md`. |

## The PR bot — api-guard

`.github/workflows/api-guard.yml` is a **reusable workflow** (`workflow_call`).
Consumer repo adoption = one file:

```yaml
# .github/workflows/api-guard.yml
name: api-guard
on: pull_request
permissions: { contents: read, pull-requests: write }
jobs:
  api-guard:
    uses: imazen/zenutils/.github/workflows/api-guard.yml@main
```

On each PR it posts/updates one comment (marker `<!-- api-guard -->`) with:
net surface delta per crate snapshot, every new item carrying a policy flag
(pub field, unsealed trait, no doc comment, no `#[non_exhaustive]`, re-export),
removed items (breaking if published), and a folded list of unflagged new
items. Without committed snapshots it says so and stops — adopt
`zenutils-apidoc` to give it teeth (that also makes API diffs part of every
code diff review locally, not just in the bot).

## The policy the flags encode (~/work/zen/AGENTS.md)

- Every `pub` item is a compatibility commitment → `pub(crate)` by default;
  new pub needs a concrete current caller.
- Builders over pub fields; `#[non_exhaustive]` where extension is plausible.
- Traits sealed unless external impls are a required feature.
- No speculative crate-root re-exports or convenience helpers.

`unreachable_pub` (rustc lint, in `templates/workspace-lints.toml`) is the
compiler-enforced half of the first rule; `api-report.py`'s zero-consumer
scan is the workspace-level half.

## Adopting the kit in a repo

1. Copy `templates/workspace-lints.toml` into the root `Cargo.toml`
   (adjusting `check-cfg` for custom cfgs), then add `[lints] workspace =
   true` to each member's `Cargo.toml`.
2. Copy `templates/clippy.toml`, `templates/deny.toml` (edit license
   exceptions + git-source allowlist), `templates/_typos.toml` to the root.
3. Paste `templates/justfile.snippet` into the justfile.
4. Drop `templates/api-guard-caller.yml` at `.github/workflows/api-guard.yml`.
5. Paste `templates/CLAUDE-quality-block.md` into the repo `CLAUDE.md`/`AGENTS.md`.
6. Optional: `apidoc/` runner crate for committed `docs/public-api/`
   snapshots (see `zenutils-apidoc` README — one 25-line test file).

`just quality` then runs everything; `just quality-quick` skips the
compile-heavy stages.
