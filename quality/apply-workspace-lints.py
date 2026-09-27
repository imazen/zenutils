#!/usr/bin/env python3
"""apply-workspace-lints.py — add `[lints] workspace = true` to every
workspace member Cargo.toml that lacks a [lints] table.

The workspace root must already carry [workspace.lints] (see
templates/workspace-lints.toml). Idempotent; prints each file it touched.
Append the block at end of file — cargo accepts it in any order.

Usage: apply-workspace-lints.py [ROOT] [--dry]
"""
import os
import re
import subprocess
import sys

BLOCK = "\n[lints]\nworkspace = true\n"


def member_manifests(root):
    p = subprocess.run(["cargo", "metadata", "--no-deps",
                        "--format-version", "1", "--manifest-path",
                        os.path.join(root, "Cargo.toml")],
                       capture_output=True, text=True)
    if p.returncode != 0:
        print("cargo metadata failed; is this a cargo workspace?",
              file=sys.stderr)
        sys.exit(2)
    import json
    return [pkg["manifest_path"] for pkg in json.loads(p.stdout)["packages"]]


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    root = os.path.abspath(args[0] if args else ".")
    dry = "--dry" in argv
    touched = skipped = 0
    for mp in member_manifests(root):
        text = open(mp, encoding="utf-8").read()
        if re.search(r"^\[lints", text, re.M):
            skipped += 1
            continue
        if re.search(r"^lints\s*=", text, re.M):   # inline form already
            skipped += 1
            continue
        if dry:
            print(f"would edit {mp}")
        else:
            with open(mp, "a", encoding="utf-8") as f:
                f.write(BLOCK)
            print(f"edited   {mp}")
        touched += 1
    print(f"---\n{touched} manifests updated, {skipped} already had [lints]")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
