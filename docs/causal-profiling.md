# Causal profiling Rust crates

Start with an operation users perform: encode one image, decode one image,
resize one buffer, convert a strided image, or score one image pair. Optimize
that operation only after collecting a reproducible baseline and preserving
its exact output.

## Inventory

`python3 quality/perf-inventory.py /path/to/repo [...]` emits one JSON record
per repository. It reads tracked manifests and Rust sources, including nested
packages, and reports benchmark/example sources, explicit Cargo targets,
textual Coz markers, Coz dependencies, revision, and tree cleanliness. It needs
Python 3.11 or later and Git; it does not build or modify the repositories.
Obtain any repository ownership required by your workspace before scanning.

The inventory includes excluded tool crates and historical tracked harnesses.
Check workspace membership and build the selected target before using it.
Textual markers may occur in comments or disabled code. `profile_verified`
is always false: an inventory cannot establish that a profile works.

## Profile and validate

Use the Rust bindings documented in [coz-rs](https://github.com/plasma-umass/coz/tree/master/rust)
and the [Coz profiler](https://github.com/plasma-umass/coz).
Keep instrumentation in a development harness. Add a `coz::progress!`
marker after a completed operation, outside input setup and verification.
Build the harness with DWARF line tables, for example
`CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release --example <harness>`.
Use runtime CPU dispatch for the baseline and candidate.

Check the installed `coz run --help` for output-format and scope options.
Run a short probe before a long experiment. A valid run needs recorded
progress, experiments attributed to the intended source, and enough repeated
observations to distinguish an effect from noise. Empty or corrupt output is
a profiling failure. A predicted virtual speedup is a hypothesis to test.

Measure baseline and candidate with the same compiler, features, CPU tier,
inputs, and interleaved benchmark protocol. Cover tiny through large inputs,
relevant content classes, and applicable quality and mode settings. Report
the measured coverage and omissions. Keep setup costs visible separately
from repeated-operation costs. Validate strided input and boundary sizes.

Require exact output parity for an optimization that preserves behavior.
For codec outputs, also inspect them with an independent decoder. Preserve
raw profiles, commands, revisions, correctness evidence, and benchmark data.
Land only a measured improvement whose correctness and patch have received
independent review; record unresolved cases as unresolved.
