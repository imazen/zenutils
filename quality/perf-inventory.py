#!/usr/bin/env python3
"""Read-only inventory of Rust performance harnesses; does not measure speed."""

import argparse
import datetime
import json
from pathlib import Path
import re
import subprocess
import tomllib


def git(root, *args):
    result = subprocess.run(
        ["git", "-C", str(root), *args], capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


def inventory(root):
    root = root.resolve(strict=True)
    if Path(git(root, "rev-parse", "--show-toplevel")).resolve() != root:
        raise ValueError(f"{root}: pass the repository root")
    # Limit reads to tracked files: no target directories, corpora, untracked
    # experiments, local path dependencies, or recursive submodule traversal.
    tracked = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"],
        capture_output=True, check=True,
    ).stdout.decode().split("\0")
    packages, markers, harnesses = [], [], []
    for relative in sorted(filter(None, tracked)):
        path = root / relative
        if path.is_symlink() or not path.is_file():
            continue
        if path.name == "Cargo.toml":
            manifest = tomllib.loads(path.read_text())
            if "package" in manifest:
                package = manifest["package"]
                dependencies = []
                tables = [("", manifest)]
                tables.extend(manifest.get("target", {}).items())
                for target, table in tables:
                    for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
                        for name, spec in table.get(kind, {}).items():
                            actual = spec.get("package", name) if isinstance(spec, dict) else name
                            if actual == "coz":
                                dependencies.append({"kind": kind, "target": target, "spec": spec})
                packages.append({
                    "name": package["name"], "manifest": relative,
                    "coz_dependencies": dependencies,
                    "explicit_benches": [b["name"] for b in manifest.get("bench", [])],
                    "explicit_examples": [e["name"] for e in manifest.get("example", [])],
                })
        if path.suffix == ".rs":
            parts = Path(relative).parts
            if "benches" in parts or "examples" in parts:
                harnesses.append(relative)
            for number, line in enumerate(path.read_text().splitlines(), 1):
                if re.search(r"\bcoz\s*::\s*(?:progress|begin|end|scope)\s*!", line):
                    markers.append({"file": relative, "line": number})
    return {
        "repository": str(root),
        "revision": git(root, "rev-parse", "HEAD"),
        "dirty": bool(git(root, "status", "--porcelain", "--untracked-files=normal")),
        "packages": packages, "harness_sources": harnesses,
        # Textual presence can include comments or cfg-disabled source. It is
        # deliberately distinct from a successful instrumented profile.
        "textual_coz_markers": markers,
        "profile_verified": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repositories", nargs="+", type=Path)
    args = parser.parse_args()
    failures = False
    for root in args.repositories:
        try:
            record = inventory(root)
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            record = {"repository": str(root), "error": str(error)}
            failures = True
        record["observed_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        print(json.dumps(record, sort_keys=True))
    raise SystemExit(1 if failures else 0)


if __name__ == "__main__":
    main()
