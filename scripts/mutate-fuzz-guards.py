#!/usr/bin/env python3
"""Mutation-verify the zenutils-fuzz seed-expectation guards.

`RegressionSuite`'s whole reason to exist is refusing to pass vacuously, so its
own guards must not be able to rot into no-ops unnoticed. This script proves
each one is actually pinned by a test: it reverts a single guard to the
permissive behaviour it replaced, runs the suite, and records which tests
noticed. A mutation that produces NO failures means that guard is unprotected.

Run from the repo root:

    python3 scripts/mutate-fuzz-guards.py

Exit code 0 means every mutation was caught and the baseline is green before
and after. The working file is restored from an in-memory copy of the pristine
source after every mutation, and the run aborts if the baseline is not green.

Each mutation is anchored on an exact substring of `zenutils-fuzz/src/lib.rs`.
Anchors rot when that file is edited; a mutation whose anchor no longer matches
exactly once is reported as ANCHOR-BAD and fails the run rather than silently
skipping. Re-anchor it against the current source when that happens.
"""
import subprocess, sys, os, re, pathlib

REPO = pathlib.Path(__file__).resolve().parent.parent
LIB = REPO / "zenutils-fuzz/src/lib.rs"
LOG = pathlib.Path(os.environ.get("MUTATE_LOG", REPO / "target/mutate-fuzz-guards.log"))

J = "\n".join

MUTATIONS = [
    ("A1-absent-dir-noop",
     "(a) an ABSENT seed dir under a declared minimum must FAIL",
     "                SeedExpectation::AtLeast(n) => panic!(",
     J(["                SeedExpectation::AtLeast(_n) => Vec::new(),",
        "                #[allow(unreachable_patterns)]",
        "                SeedExpectation::AtLeast(n) => panic!("])),

    ("A2-unreadable-dir-noop",
     "(a) an UNREADABLE / not-a-directory seed path must FAIL",
     J(["        Ok(_) => {",
        "            return Err(ScanError::Io {"]),
     J(["        Ok(_) => {",
        "            return Ok(Vec::new());",
        "        }",
        "        #[allow(unreachable_patterns)]",
        "        Ok(_) => {",
        "            return Err(ScanError::Io {"])),

    ("A3-count-check-disabled",
     "(a) an EMPTY or SHORT corpus under a declared minimum must FAIL",
     "            if seeds.len() < n {",
     "            if false && seeds.len() < n {"),

    ("B1-seeds-replayed-inflated",
     "(b) the report must state the TRUE replay count",
     J(["        self.seed_paths.len()",
        "    }",
        "",
        "    /// Number of registered targets."]),
     J(["        self.seed_paths.len() + 1",
        "    }",
        "",
        "    /// Number of registered targets."])),

    ("B2-invocations-drops-targets",
     "(b) invocations must be seeds x targets",
     "        self.seed_paths.len() * self.target_count",
     "        self.seed_paths.len()"),

    ("C1-empty-message-looks-absent",
     "(c) absent / empty / short must be distinguishable from the message",
     "exists but contains no ",
     "does not exist and contains no "),

    ("C2-short-message-looks-empty",
     "(c) a SHORT corpus must not be reported as an empty one",
     "yielded {} seed(s) but at ",
     "contains no seeds, yielded {} seed(s) but at "),

    ("D1-readme-counted-as-seed",
     "README.md / *.txt / dotfiles must never count as seeds",
     '        if lower.ends_with(".md") || lower.ends_with(".txt") {',
     '        if false && (lower.ends_with(".md") || lower.ends_with(".txt")) {'),

    ("E1-permissive-by-default",
     "(d) the permissive behaviour must be OPT-IN, not the default",
     "            expectation: SeedExpectation::Undeclared,",
     "            expectation: SeedExpectation::AllowEmpty,"),
]


def run_tests():
    p = subprocess.run(
        ["nice", "-n", "19", "cargo", "test", "-p", "zenutils-fuzz", "-j", "4"],
        cwd=REPO, capture_output=True, text=True,
    )
    out = p.stdout + p.stderr
    failed = sorted(set(re.findall(r"^test (\S+) \.\.\. FAILED$", out, re.M)))
    build_err = ("error[" in out) or ("error: could not compile" in out)
    return p.returncode, failed, build_err, out


def main():
    LOG.parent.mkdir(parents=True, exist_ok=True)
    log = LOG.open("w")
    pristine = LIB.read_text()
    results = []

    print("=== BASELINE (unmutated) ===")
    LIB.write_text(pristine)
    rc, failed, berr, out = run_tests()
    log.write("### BASELINE rc=%s failed=%s\n%s\n\n" % (rc, failed, out))
    print("baseline rc=%s failures=%s build_error=%s" % (rc, failed, berr))
    if rc != 0:
        print("BASELINE IS NOT GREEN — aborting")
        log.close()
        sys.exit(1)

    try:
        for mid, behaviour, old, new in MUTATIONS:
            n_hits = pristine.count(old)
            if n_hits != 1:
                print("!! %s: anchor matched %d times (need exactly 1) — re-anchor it "
                      "against the current lib.rs" % (mid, n_hits))
                results.append((mid, behaviour, "ANCHOR-BAD(%d)" % n_hits, []))
                continue
            LIB.write_text(pristine.replace(old, new, 1))
            rc, failed, berr, out = run_tests()
            log.write("### %s rc=%s build_error=%s failed=%s\n%s\n\n"
                      % (mid, rc, berr, failed, out))
            if berr:
                verdict = "BUILD-ERROR"
            elif rc != 0 and failed:
                verdict = "CAUGHT"
            else:
                verdict = "SURVIVED"
            results.append((mid, behaviour, verdict, failed))
            print("%-12s %-30s %s" % (verdict, mid, behaviour))
            for f in failed:
                print("             failing test: %s" % f)
            LIB.write_text(pristine)
    finally:
        # Never leave a mutated source behind, even on Ctrl-C or a crash.
        LIB.write_text(pristine)

    print("\n=== RESTORED; re-verifying baseline ===")
    rc, failed, berr, out = run_tests()
    print("restored rc=%s failures=%s" % (rc, failed))
    log.write("### RESTORED rc=%s failed=%s\n" % (rc, failed))
    log.close()

    print("\n=== SUMMARY ===")
    bad = [r for r in results if r[2] != "CAUGHT"]
    for mid, behaviour, verdict, failed in results:
        print("%-12s %-30s -> %d failing test(s)" % (verdict, mid, len(failed)))
    print("\nfull log: %s" % LOG)
    sys.exit(1 if (bad or rc != 0) else 0)


if __name__ == "__main__":
    main()
