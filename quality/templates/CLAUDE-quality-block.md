## Quality hygiene (advisory, standing)

Run `just quality` before pushing (or after any multi-file change): fmt
check, clippy warning census, unused deps, `cargo deny`, typos, stale-doc
scan, API-surface exposure report. Nothing here gates CI — the contract is
that warning counts shrink over time, never grow. If your change adds
rustc/clippy warnings, fix them in the same commit; don't leave them for
"later". New `pub` items need a concrete current caller — default to
`pub(crate)` (`unreachable_pub` is warned workspace-wide); the api-guard PR
bot flags new surface plus pub fields, unsealed traits, missing docs, and
re-export growth. `just coverage` (llvm-cov) measures test coverage — check
it before refactoring anything without tests.
