#!/usr/bin/env python3
"""api-diff.py — public-API surface diff between two git refs, as markdown.

Designed for the api-guard PR bot: compares committed
`docs/public-api/<crate>.txt` snapshots (zenutils-apidoc format: one `pub …`
item line each) at BASE vs HEAD — zero build required — and flags the ADDED
items against AGENTS.md API policy (pub fields, unsealed traits, missing
docs, missing #[non_exhaustive], re-export growth). Removed items are listed
too (semver note: removals from published crates are breaking).

Repos without committed snapshots get a source-scan fallback note; a future
--deep mode can run `cargo public-api diff` with a nightly toolchain.

Usage:
  api-diff.py --root REPO --base SHA --head SHA [--json] [--comment FILE]
"""
import argparse
import json
import os
import re
import subprocess
import sys

ITEM_RE = re.compile(r"^pub\s")
FIELD_ITEM_RE = re.compile(r"^pub\s+(?:const\s+)?[\w:]+::(\w+):\s")
FN_ITEM_RE = re.compile(r"^pub fn\s+([\w:]+)")
TYPE_ITEM_RE = re.compile(r"^pub (struct|enum|trait|union|type)\s+([\w:]+)")
USE_ITEM_RE = re.compile(r"^pub use\s+")
MOD_ITEM_RE = re.compile(r"^pub mod\s+([\w:]+)")
CONST_ITEM_RE = re.compile(r"^pub (?:const|static)\s+([\w:]+)")
VARIANT_ITEM_RE = re.compile(r"^pub\s+([\w:]+::\w+)$")
NONEXH_RE = re.compile(r"#\[non_exhaustive\]")
DOC_RE = re.compile(r"(///|//!|#\s*\[\s*doc)")
SKIP_DIRS = {"target", ".git", ".jj", "node_modules"}


def git_show(root, ref, path):
    p = subprocess.run(["git", "-C", root, "show", f"{ref}:{path}"],
                       capture_output=True, text=True)
    return p.stdout if p.returncode == 0 else None


def snapshot_files(root, ref):
    p = subprocess.run(
        ["git", "-C", root, "ls-tree", "-r", "--name-only", ref,
         "--", "docs/public-api"], capture_output=True, text=True)
    return [l for l in p.stdout.splitlines()
            if l.endswith(".txt") and not os.path.basename(l).startswith("ABLATION")]


def items_of(text):
    return {l.strip() for l in text.splitlines() if ITEM_RE.match(l)}


def find_leaf_src(crate_name, root):
    """crate dir for source lookups (crates/<name> or <name>)."""
    for cand in (f"crates/{crate_name}", crate_name,
                 f"src/../../{crate_name}"):
        d = os.path.join(root, cand)
        if os.path.isdir(os.path.join(d, "src")):
            return d
    return None


def leaf_ident(line):
    m = FN_ITEM_RE.match(line)
    if m:
        return m.group(1).split("::")[-1], "fn"
    m = TYPE_ITEM_RE.match(line)
    if m:
        return m.group(2).split("::")[-1], m.group(1)
    m = FIELD_ITEM_RE.match(line)
    if m:
        return m.group(1), "field"
    m = CONST_ITEM_RE.match(line)
    if m:
        return m.group(1).split("::")[-1], "const"
    m = MOD_ITEM_RE.match(line)
    if m:
        return m.group(1).split("::")[-1], "mod"
    m = VARIANT_ITEM_RE.match(line)
    if m:
        return m.group(1).split("::")[-1], "variant"
    return None, None


def item_flags(root, crate, line, src_cache):
    """Policy flags for one added pub item, checked against head sources."""
    flags = []
    ident, kind = leaf_ident(line)
    if USE_ITEM_RE.match(line):
        flags.append("re-export (re-exports widen surface — confirm needed)")
        return flags
    if kind == "field":
        if line.startswith("pub const"):
            flags.append("pub const — surface growth; confirm it belongs")
        else:
            flags.append("pub field (policy prefers builders/getters)")
        return flags
    if ident is None:
        return flags
    srcdir = find_leaf_src(crate, root)
    if not srcdir:
        return flags
    if srcdir not in src_cache:
        blob = []
        for base, dirs, files in os.walk(os.path.join(srcdir, "src")):
            dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
            for fn in files:
                if fn.endswith(".rs"):
                    p = os.path.join(base, fn)
                    try:
                        blob.append((p, open(p, encoding="utf-8",
                                             errors="replace").read()))
                    except OSError:
                        pass
        src_cache[srcdir] = blob
    pat = re.compile(r"^\s*(?:#\[[^\n]*\]\s*)*pub\s+(?:unsafe\s+|async\s+)*"
                     + (r"trait\s+" if kind == "trait" else
                        r"(?:struct|enum)\s+" if kind in ("struct", "enum") else
                        r"(?:fn|const|static|mod|type)\s+") + re.escape(ident)
                     + r"\b", re.M)
    for p, text in src_cache[srcdir]:
        m = pat.search(text)
        if not m:
            continue
        start = text.rfind("\n", 0, m.start()) + 1
        above = text[max(0, m.start() - 800):start]
        last_lines = above.rstrip().splitlines()
        has_doc = any(DOC_RE.search(l) for l in last_lines[-8:] if l.strip())
        has_nonexh = any(NONEXH_RE.search(l) for l in last_lines[-6:])
        if not has_doc:
            flags.append("no doc comment")
        if kind in ("struct", "enum") and not has_nonexh:
            flags.append("pub {} without #[non_exhaustive]".format(kind))
        if kind == "trait" and "Sealed" not in text and \
                "private" not in text[:m.start()].lower():
            flags.append("pub trait — sealed? (verify intended impl openness)")
        flags.append(f"src: {os.path.relpath(p, root)}")
        break
    return flags


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--base", required=True)
    ap.add_argument("--head", required=True)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--comment", metavar="FILE")
    a = ap.parse_args(argv[1:])
    root = os.path.abspath(a.root)

    files = sorted(set(snapshot_files(root, a.base)) |
                   set(snapshot_files(root, a.head)))
    src_cache = {}
    crates = {}
    for f in files:
        crate = os.path.basename(f).replace(".txt", "")
        base_t = git_show(root, a.base, f) or ""
        head_t = git_show(root, a.head, f) or ""
        added = sorted(items_of(head_t) - items_of(base_t))
        removed = sorted(items_of(base_t) - items_of(head_t))
        if added or removed:
            crates.setdefault(crate.replace(".features", "")
                              .replace(".internal", ""), {})[f] = {
                "added": added, "removed": removed}

    flagged = []
    for crate, per_file in crates.items():
        core = crate
        for f, d in per_file.items():
            for line in d["added"]:
                fl = item_flags(root, core, line, src_cache)
                flagged.append({"crate": core, "item": line, "file": f,
                                "flags": fl})

    added_n = sum(len(d["added"]) for c in crates.values() for d in c.values())
    removed_n = sum(len(d["removed"]) for c in crates.values()
                    for d in c.values())

    if a.json:
        print(json.dumps({"added": added_n, "removed": removed_n,
                          "crates": crates, "flags": flagged}, indent=1))

    out = []
    out.append("<!-- api-guard -->")
    out.append("### API surface report")
    if not files:
        out.append("_No `docs/public-api/*.txt` snapshots found at either ref"
                   " — adopt `zenutils-apidoc` (`just api-doc`) for item-level"
                   " diffs._")
    elif added_n == 0 and removed_n == 0:
        out.append("No public-API surface change. ✅")
    else:
        out.append(f"**Δ surface: +{added_n} / −{removed_n}** "
                   "items across snapshot files.\n")
        for crate, per_file in sorted(crates.items()):
            for f, d in sorted(per_file.items()):
                out.append(f"**`{f}`**: +{len(d['added'])} −{len(d['removed'])}")
        needs = [f for f in flagged if any("doc comment" in x or
                                           "field" in x or "sealed" in x or
                                           "non_exhaustive" in x or
                                           "re-export" in x
                                           for x in f["flags"])]
        if needs:
            out.append("\n#### Needs a look\n")
            for f in needs:
                fl = "; ".join(x for x in f["flags"] if not x.startswith("src:"))
                src = next((x[5:] for x in f["flags"] if x.startswith("src:")), "")
                out.append(f"- `{f['item']}` — {fl}"
                           + (f" ({src})" if src else ""))
        plain = [f for f in flagged if f not in needs]
        if plain:
            out.append("\n<details><summary>New items (no flags): "
                       f"{len(plain)}</summary>\n")
            for f in plain:
                out.append(f"- `{f['item']}`")
            out.append("\n</details>")
        if removed_n:
            out.append("\n#### Removed (breaking if published)\n")
            for crate, per_file in sorted(crates.items()):
                for f, d in sorted(per_file.items()):
                    for line in d["removed"][:30]:
                        out.append(f"- `{line}`")
    out.append("\n— api-guard (advisory; "
               "[rules](https://github.com/imazen/zenutils/blob/main/quality/README.md))")
    body = "\n".join(out)
    if a.comment:
        open(a.comment, "w").write(body)
    if not a.json:
        print(body)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
