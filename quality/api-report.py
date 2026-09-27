#!/usr/bin/env python3
"""api-report.py — public-API surface inventory + YAGNI/exposure flags.

Advisory. Answers three questions per workspace crate:

  How big is the surface?   pub / pub(crate) / pub field / re-export counts.
  What's exposed wrongly?   pub-in-bin, pub fields on pub structs, undocumented
                            pub items, unsealed traits, missing non_exhaustive.
  What's unused internally? For `publish = false` crates: pub leaf items that
                            no in-workspace dependent ever names in source
                            (consumer map from `cargo metadata`). Such items
                            are candidates for pub(crate) or removal — the
                            workspace proves they have no internal caller.

Committed `docs/public-api/<crate>.txt` snapshots (zenutils-apidoc) are used
for the authoritative item count where present; otherwise items are counted
by source scan (approximation — macros hide items a snapshot would catch).

Usage:
  api-report.py [ROOT] [--siblings DIR] [--json] [--verbose] [--crates a,b]
"""
import json
import os
import re
import subprocess
import sys

SKIP_DIRS = {".git", ".jj", "target", "node_modules", "tests", "benches",
             "examples"}
COMMON_IDENTS = {
    "new", "default", "Error", "Config", "Options", "Settings", "Params",
    "Builder", "Result", "from", "into", "clone", "fmt", "write", "read",
    "open", "close", "run", "main", "Drop", "Display", "Debug", "send",
}
PUB_ITEM_RE = re.compile(
    r"^\s*pub(?:\((crate|super|self|in\s+[^)]+)\))?\s+"
    r"(?:(?:async|unsafe|const|extern(?:\s+\"[A-Za-z]+\")?)\s+)*"
    r"(fn|struct|enum|trait|type|const|static|mod|use|union)\s+([A-Za-z_]\w*)")
PUB_FIELD_RE = re.compile(r"^\s*pub\s+([a-z_]\w*)\s*:")
VIS_PUB_RE = re.compile(r"^\s*pub\s")
VIS_QUAL_RE = re.compile(r"^\s*pub\(")
STRUCT_RE = re.compile(r"^\s*(?:#\[[^\n]*\]\s*)*pub\s+struct\s+([A-Za-z_]\w*)")
ENUM_RE = re.compile(r"^\s*(?:#\[[^\n]*\]\s*)*pub\s+enum\s+([A-Za-z_]\w*)")
TRAIT_RE = re.compile(r"^\s*pub\s+(?:unsafe\s+|auto\s+)*trait\s+([A-Za-z_]\w*)")
NONEXH_RE = re.compile(r"#\[non_exhaustive\]")
DOC_RE = re.compile(r"(///|//!|#\s*\[\s*doc)")


def cargo_metadata(root):
    try:
        p = subprocess.run(["cargo", "metadata", "--no-deps",
                            "--format-version", "1", "--manifest-path",
                            os.path.join(root, "Cargo.toml")],
                           capture_output=True, text=True, timeout=120)
        if p.returncode == 0:
            return json.loads(p.stdout)
    except Exception:
        pass
    return None


def parse_manifest(path):
    name = publish = None
    lib = bins = False
    try:
        sec = None
        for line in open(path, encoding="utf-8", errors="replace"):
            s = line.strip()
            if s.startswith("["):
                sec = s.strip("[] ").split(".")[0]
                continue
            if sec == "package":
                if s.startswith("name"):
                    name = s.split("=", 1)[1].strip().strip('"')
                elif s.startswith("publish") and "false" in s:
                    publish = False
            elif sec == "lib":
                lib = True
            elif sec == "bin" or s.startswith("[[bin]]"):
                bins = True
    except OSError:
        pass
    return name, publish, lib, bins


def members(root):
    meta = cargo_metadata(root)
    out = {}
    if meta:
        for pkg in meta["packages"]:
            d = os.path.dirname(pkg["manifest_path"])
            kinds = {t["kind"][0] for t in pkg["targets"]}
            out[pkg["name"]] = {
                "dir": d, "publish": pkg.get("publish") != [],
                "lib": "lib" in kinds or "rlib" in kinds,
                "bin": "bin" in kinds,
                "deps": [dep["name"] for dep in pkg["dependencies"]
                         if dep.get("path")],
            }
        return out, True
    # Fallback: crates/*/Cargo.toml + root package
    for cand in sorted(glob_crates(root)):
        name, publish, lib, bins = parse_manifest(os.path.join(cand, "Cargo.toml"))
        if name:
            out[name] = {"dir": cand, "publish": publish is not False,
                         "lib": True, "bin": bins, "deps": []}
    return out, False


def glob_crates(root):
    out = []
    for sub in ("crates", ".", "src/.."):
        d = os.path.join(root, sub)
        if not os.path.isdir(d):
            continue
        for entry in sorted(os.listdir(d)):
            p = os.path.join(d, entry)
            if os.path.isfile(os.path.join(p, "Cargo.toml")):
                out.append(p)
    return out


def rs_files(d, include_tests=False):
    for base, dirs, files in os.walk(d):
        dirs[:] = [x for x in dirs if x in SKIP_DIRS and include_tests or
                   x not in SKIP_DIRS]
        for f in files:
            if f.endswith(".rs"):
                yield os.path.join(base, f)


def scan_crate(root, info):
    """Regex-scan one crate's sources. Returns stats dict + flag list."""
    stats = {"pub": 0, "pub_qual": 0, "fields": 0, "use": 0, "traits": 0,
             "pub_in_bin": 0, "undoc": 0, "nonexh_missing": 0,
             "sealed_traits": 0, "idents": {}}
    flags = []
    src = os.path.join(info["dir"], "src")
    crate_text = ""
    file_lines = {}
    for f in rs_files(info["dir"]):
        try:
            file_lines[f] = open(f, encoding="utf-8", errors="replace").read().splitlines()
        except OSError:
            continue
        crate_text += "\n".join(file_lines[f]) + "\n"
    sealed_names = set(re.findall(r"Sealed\b", crate_text)) and True

    for f, lines in file_lines.items():
        in_bin = info["bin"] and (f.endswith("main.rs") or "/bin/" in f or
                                  "/examples/" in f)
        in_struct = 0          # brace depth inside a pub struct body
        doc_block = False      # a /// or #[doc] in the contiguous attr block above
        attr_lines = 0         # lines since last non-attr, non-doc line
        for i, line in enumerate(lines):
            s = line.strip()
            if DOC_RE.search(s):
                doc_block = True
            if in_struct:
                in_struct += line.count("{") - line.count("}")
                if PUB_FIELD_RE.match(line):
                    stats["fields"] += 1
                    flags.append(("PUB-FIELD", f, i + 1, s.rstrip("{,")))
                if in_struct <= 0:
                    in_struct = 0
                continue
            m = PUB_ITEM_RE.match(line)
            if m:
                qual, kind, name = m.groups()
                if in_bin:
                    stats["pub_in_bin"] += 1
                    flags.append(("PUB-IN-BIN", f, i + 1, f"{kind} {name}"))
                if qual:
                    stats["pub_qual"] += 1
                else:
                    stats["pub"] += 1
                    stats["idents"][name] = (f, i + 1, kind)
                    if kind in ("struct", "enum"):
                        # non_exhaustive must be in the attribute block above
                        above = "\n".join(lines[max(0, i - 4):i])
                        if not NONEXH_RE.search(above):
                            stats["nonexh_missing"] += 1
                    if not doc_block and attr_lines == 0:
                        pass
                    if not doc_block:
                        stats["undoc"] += 1
                        if attr_lines < 200:  # detail cap; summary keeps count
                            flags.append(("NO-DOC", f, i + 1, f"{kind} {name}"))
                    if kind == "use":
                        stats["use"] += 1
                        flags.append(("REEXPORT", f, i + 1,
                                      s[:80].rstrip(";")))
                    if kind == "trait":
                        stats["traits"] += 1
                        if not (sealed_names and
                                re.search(r"trait\s+" + name +
                                          r"[^:{]*:\s*[^\n{]*[Ss]ealed", line)):
                            flags.append(("UNSEALED?", f, i + 1,
                                          f"pub trait {name}"))
                if kind == "struct" and "{" in line:
                    in_struct = 1 + line.count("{") - line.count("}") - 1
            # doc/attr tracking for the NEXT item
            if not s or s.startswith(("///", "//!", "#[", "//")):
                attr_lines = 0
            else:
                attr_lines += 1
                doc_block = False
    return stats, flags


def snapshot_counts(root):
    out = {}
    d = os.path.join(root, "docs", "public-api")
    if not os.path.isdir(d):
        return out
    for f in os.listdir(d):
        if f.endswith(".txt") and not f.startswith(("ABLATION", "README")):
            try:
                n = sum(1 for l in open(os.path.join(d, f),
                                        encoding="utf-8", errors="replace")
                        if l.startswith("pub "))
                out[f[:-4]] = n
            except OSError:
                pass
    return out


def leaf_consumer_map(root, mems):
    """For publish=false crates: which dependents name each pub leaf ident?"""
    dep_src = {}
    for name, info in mems.items():
        blob = []
        for sub in ("src", "tests", "benches", "examples"):
            for f in rs_files(os.path.join(info["dir"], sub)) if \
                    os.path.isdir(os.path.join(info["dir"], sub)) else []:
                try:
                    blob.append(open(f, encoding="utf-8", errors="replace").read())
                except OSError:
                    pass
        dep_src[name] = "\n".join(blob)
    dependents = {n: [m for m, i in mems.items() if n in i["deps"]]
                  for n in mems}
    orphan = {}
    for name, info in mems.items():
        if info["publish"]:
            continue
        deps = dependents.get(name, [])
        if not deps:
            continue
        haystack = "\n".join(dep_src.get(d, "") for d in deps)
        for ident, (f, ln, kind) in list(_crate_idents_cache[name].items()):
            if ident in COMMON_IDENTS or len(ident) < 4:
                continue
            if not re.search(r"\b" + re.escape(ident) + r"\b", haystack):
                orphan.setdefault(name, []).append((ident, kind, f, ln))
    return orphan


_crate_idents_cache = {}


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    flags = {a for a in argv[1:] if a.startswith("--")}
    root = os.path.abspath(args[0] if args else ".")
    only = None
    for a in argv[1:]:
        if a.startswith("--crates="):
            only = set(a.split("=", 1)[1].split(","))
        if a.startswith("--siblings="):
            pass  # consumers outside the workspace: future flag
    mems, have_meta = members(root)
    if not mems:
        print("api-report: no cargo workspace members found", file=sys.stderr)
        return 2
    snaps = snapshot_counts(root)
    report = {}
    all_flags = []

    for name, info in sorted(mems.items()):
        if only and name not in only:
            continue
        stats, flags = scan_crate(root, info)
        _crate_idents_cache[name] = stats.pop("idents")
        stats["snapshot_items"] = snaps.get(name)
        stats["publish"] = info["publish"]
        stats["bin"] = info["bin"]
        report[name] = stats
        for kind, f, ln, detail in flags:
            all_flags.append({"crate": name, "kind": kind,
                              "file": os.path.relpath(f, root),
                              "line": ln, "detail": detail})

    orphans = leaf_consumer_map(root, mems) if have_meta else {}

    if "--json" in flags:
        print(json.dumps({"crates": report,
                          "orphans": {k: [{"item": i, "kind": k2,
                                           "file": os.path.relpath(f, root),
                                           "line": ln}
                                          for i, k2, f, ln in v]
                                      for k, v in orphans.items()},
                          "flags": all_flags}, indent=1))
        return 0

    print(f"{'crate':<28} {'pub':>5} {'pub()':>5} {'fld':>4} {'use':>4} "
          f"{'trait':>5} {'undoc':>5} {'orphan':>6} {'snap':>5}")
    tot_pub = tot_q = tot_fld = tot_use = tot_orph = 0
    for name, s in report.items():
        orph = len(orphans.get(name, []))
        mark = "" if s["publish"] else " (internal)"
        print(f"{name + mark:<28} {s['pub']:>5} {s['pub_qual']:>5} "
              f"{s['fields']:>4} {s['use']:>4} {s['traits']:>5} "
              f"{s['undoc']:>5} {orph:>6} "
              f"{s['snapshot_items'] if s['snapshot_items'] else '-':>5}")
        tot_pub += s["pub"]; tot_q += s["pub_qual"]; tot_fld += s["fields"]
        tot_use += s["use"]; tot_orph += orph
    print(f"{'TOTAL':<28} {tot_pub:>5} {tot_q:>5} {tot_fld:>4} "
          f"{tot_use:>4} {'':>5} {'':>5} {tot_orph:>6}")

    by_kind = {}
    for f in all_flags:
        by_kind.setdefault(f["kind"], []).append(f)
    order = ["PUB-IN-BIN", "PUB-FIELD", "UNSEALED?", "NO-DOC", "REEXPORT"]
    titles = {
        "PUB-IN-BIN": "pub items inside binary targets (internal exposure)",
        "PUB-FIELD": "pub fields on pub structs (policy: prefer builders)",
        "UNSEALED?": "pub traits with no visible sealed bound (verify)",
        "NO-DOC": "pub items with no doc comment",
        "REEXPORT": "pub use re-exports",
    }
    for k in order:
        fs = by_kind.get(k, [])
        if not fs:
            continue
        print(f"\n--- {titles[k]} — {len(fs)} ---")
        limit = None if "--verbose" in flags else 25
        for f in fs[:limit]:
            print(f"  {f['crate']}  {f['file']}:{f['line']}  {f['detail']}")
        if limit and len(fs) > limit:
            print(f"  … {len(fs) - limit} more (--verbose)")

    if orphans:
        print(f"\n--- ZERO-CONSUMER (internal crates; pub item never named "
              f"by any in-workspace dependent) — {tot_orph} ---")
        shown = 0
        for name, items in sorted(orphans.items()):
            print(f"  {name}:")
            for ident, kind, f, ln in items[:(20 if "--verbose" not in flags
                                              else None)]:
                print(f"    {kind} {ident}  {os.path.relpath(f, root)}:{ln}")
                shown += 1
            if len(items) > 20 and "--verbose" not in flags:
                print(f"    … {len(items) - 20} more (--verbose)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
