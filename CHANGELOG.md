# Changelog

All notable changes to crates in this workspace are documented here, following
[Keep a Changelog](https://keepachangelog.com/).

## zenutils-fuzz

### [Unreleased]

#### Added
- Initial `zenutils-fuzz` crate: a fuzz-regression seed-corpus runner
  (`RegressionSuite`) moved from the un-versioned `zen-fuzz-regress` helper.
  Walks `fuzz/regression/*` and feeds every seed through every registered
  fuzz-target entry point; a panic surfaces seed path + target name. Ships
  with 6 unit tests covering no-op/empty/recursion/meta-skip/panic paths.
