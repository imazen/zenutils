#!/usr/bin/env python3
"""check-stale-docs.py — find dead references in a repo's markdown docs.

Mechanical staleness signals only; no judgement of prose. Classes:

  DEADLINK    [text](path) or ref-def target that resolves to nothing on disk
  DEADREF     backticked `dir/file` whose first segment exists (so it was a
              real repo-relative path) but the file is gone — or whose basename
              survives elsewhere (moved, not deleted)
  DEADSCRIPT  backticked `name.sh|py|rs|toml|yml` basename found nowhere
  DEADJUST    `just <recipe>` naming a recipe no justfile defines
  UNRESOLVED  path-shaped token whose root segment doesn't exist — could be a
              slug/job-id, a sibling-repo path, or a deleted tree; quiet tier
  EXTPATH     absolute path under /mnt /tmp /usr /home /etc /var /opt or ~/
              absent on THIS machine — informational only

Deliberately NOT flagged: tokens that look like git refs (feat/foo, v1.2),
CLI `--flag/value` pairs, platform tuples (linux/arm64), KEY=VALUE spans,
and anything with a URI scheme. Dead refs inside CHANGELOG/RELEASES files or
docs carrying a HISTORICAL/frozen/Superseded banner or a dated filename are
"snapshot" findings — history, not staleness; reported separately.

Usage:
  check-stale-docs.py [ROOT]            report (exit 0 always)
  check-stale-docs.py ROOT --strict     exit 1 if living docs have findings
  check-stale-docs.py ROOT --living     suppress snapshot-doc findings
  check-stale-docs.py ROOT --quiet      also suppress UNRESOLVED + EXTPATH
  check-stale-docs.py ROOT --json       machine-readable findings
"""
import json
import os
import re
import subprocess
import sys
import urllib.parse

SKIP_DIRS = {".git", ".jj", "target", "node_modules", "vendor",
             "__pycache__", ".cargo", "dist", "build"}
SNAPSHOT_RE = re.compile(r"(19|20)\d\d[-_]\d\d[-_]\d\d")
BANNER_RE = re.compile(r"HISTORICAL|Status:\s*(HISTORICAL|Frozen|Concluded|Superseded)"
                       r"|frozen 20\d\d", re.I)
SNAPSHOT_NAMES = {"CHANGELOG.md", "RELEASES.md", "HISTORY.md"}
SNAPSHOT_NAME_RE = re.compile(r"^(RFC_|RFC\d+)", re.I)  # point-in-time proposals
SNAPSHOT_PATH_RE = re.compile(r"(^|/)(docs/status/|worklogs?/)" , re.I)
MONTH_RE = re.compile(r"[-_](19|20)\d\d[-_]\d\d(?:\.|_|$)")
GENERIC_BASENAMES = {"main.rs", "lib.rs", "mod.rs", "build.rs"}
NEVER_ROOT = {"target", "node_modules", "vendor", ".git", ".jj"}
MD_LINK_RE = re.compile(r"\[[^\]\n]*\]\(([^)\s]+)(?:\s+[\"'][^\"']*[\"'])?\)")
REFDEF_RE = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*(\S+)", re.M)
HTML_LINK_RE = re.compile(r"""(?:href|src)\s*=\s*["']([^"'>#]+)["']""", re.I)
CODESPAN_RE = re.compile(r"`([^`\n]+)`")
PATH_TOKEN_RE = re.compile(r"^(~|/|\.\.?/)?[\w.+@-]+(?:/[\w.+@-]+)+/?$")
BASENAME_RE = re.compile(r"^[\w][\w.+-]*\.(?:sh|py|rs)$")
JUST_CMD_RE = re.compile(r"^just\s+([a-zA-Z][\w-]*)")
JUSTFILE_RECIPE_RE = re.compile(r"^([a-zA-Z][\w-]*)[^:=]*:(?!=)")
SCHEME_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")
PLATFORM_RE = re.compile(
    r"^(linux|windows|darwin|macos|freebsd|android|ios|wasm32|x86|x86_64"
    r"|amd64|arm64|aarch64|armv7|riscv64)/", re.I)
GITISH_RE = re.compile(r"^(feat|fix|feature|hotfix|release|origin|upstream"
                       r"|dependabot|renovate|refs|tags?|heads?)/", re.I)
FS_ROOTS = {"mnt", "tmp", "var", "usr", "home", "etc", "opt", "root",
            "media", "srv", "data"}
# A dead ref named inside a deletion/replacement narrative ("was deleted
# 2026-06-25", "replaced the deleted X", "folded into Y") is provenance,
# not staleness — downgrade to MENTIONED.
DELTA_CONTEXT_RE = re.compile(
    r"\b(?:was|were|has been|have been|is|are|now)\s+"
    r"(?:deleted|removed|retired|superseded|folded|renamed|replaced|moved)\b"
    r"|\b(?:the\s+)?(?:deleted|removed|retired|superseded|former|old)\b"
    r"|\b(?:deleted|removed|folded|renamed|moved)\s+"
    r"(?:by|with|in|into|to|on|back in|during)\b"
    r"|\bno longer exists\b|\bused to be\b|\breplaces?\b", re.I)
TRAIL = " \t\n`',.;:()[]{}\"'<>"
# Leading strip must NOT eat '.', '/', '~' — they carry meaning
# (`./x`, `../x`, `~/x`, `.github/…`, absolute paths).
LEAD = " \t\n`',;:()[]{}\"'<>"


def repo_root(path):
    try:
        return subprocess.run(
            ["git", "-C", path, "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, timeout=10).stdout.strip() \
            or os.path.abspath(path)
    except Exception:
        return os.path.abspath(path)


def git_refs(root):
    try:
        p = subprocess.run(["git", "-C", root, "for-each-ref",
                            "--format=%(refname:short)"],
                           capture_output=True, text=True, timeout=15)
        return set(p.stdout.split())
    except Exception:
        return set()


def walk_files(root):
    rels, bases = set(), set()
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for f in files:
            p = os.path.join(base, f)
            rels.add(os.path.relpath(p, root))
            bases.add(f)
    return rels, bases


def basenames_map(root):
    """basename -> [repo-relative paths] for --fix disambiguation."""
    m = {}
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for f in files:
            m.setdefault(f, []).append(
                os.path.relpath(os.path.join(base, f), root))
    return m


def md_files(root):
    out = []
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        out += [os.path.join(base, f) for f in files
                if f.endswith((".md", ".mdx"))]
    return sorted(out)


def just_recipes(root):
    recipes = set()
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for f in files:
            if f == "justfile" or f.endswith(".just"):
                try:
                    for line in open(os.path.join(base, f),
                                     encoding="utf-8"):
                        m = JUSTFILE_RECIPE_RE.match(line)
                        if m and not line.startswith((" ", "\t", "#")):
                            recipes.add(m.group(1))
                except OSError:
                    pass
    return recipes


def is_snapshot(root, path, head):
    name = os.path.basename(path)
    rel = os.path.relpath(path, root)
    if name in SNAPSHOT_NAMES or SNAPSHOT_NAME_RE.match(name) \
            or SNAPSHOT_RE.search(name) \
            or MONTH_RE.search(name) or "_WORKLOG" in name \
            or SNAPSHOT_PATH_RE.search(rel):
        return True
    return bool(BANNER_RE.search(head))


class Checker:
    def __init__(self, root):
        self.root = root
        self.rels, self.bases = walk_files(root)
        self.recipes = just_recipes(root)
        self.refs = git_refs(root)

    def link_ok(self, doc_dir, target):
        t = urllib.parse.unquote(target.split("#", 1)[0].strip())
        if not t or SCHEME_RE.match(t):
            return True
        return os.path.exists(os.path.join(doc_dir, t)) or \
            os.path.exists(os.path.join(self.root, t))

    def token(self, doc_dir, tok):
        """(class, detail) for one path-ish token, or None."""
        t = tok.strip(LEAD).rstrip(TRAIL)
        if not t or t.startswith("-"):
            return None
        if "=" in t:                      # KEY=value — check the value half
            t = t.split("=", 1)[1].strip(LEAD).rstrip(TRAIL)
        if not t:
            return None
        if t.startswith("~/") or t.startswith("/"):
            first = t.lstrip("~/").split("/", 1)[0]
            if t.startswith("/") and first not in FS_ROOTS:
                return None               # API endpoint, URL path, register…
            if not os.path.exists(os.path.expanduser(t)):
                return ("EXTPATH", t)
            return None
        if not PATH_TOKEN_RE.match(t):
            if BASENAME_RE.match(t) and t not in self.bases \
                    and t not in GENERIC_BASENAMES:
                return ("DEADSCRIPT", t)
            return None
        if PLATFORM_RE.match(t):
            return None
        if os.path.exists(os.path.join(doc_dir, t)) or \
                os.path.exists(os.path.join(self.root, t)) or \
                os.path.normpath(t) in self.rels:
            return None
        if t in self.refs or GITISH_RE.match(t):
            return None
        first = t.split("/", 1)[0]
        if first in NEVER_ROOT:
            return None                  # build output; absence is normal
        base = os.path.basename(t.rstrip("/"))
        moved = base in self.bases and base not in ("Cargo.toml", "main.rs",
                                                  "lib.rs", "mod.rs")
        if first in self.rels or os.path.isdir(os.path.join(self.root, first))\
                or moved:
            return ("DEADREF", t + ("  (basename exists elsewhere — moved?)"
                                    if moved else ""))
        return ("UNRESOLVED", t)

    def scan(self, path):
        doc_dir = os.path.dirname(path)
        try:
            text = open(path, encoding="utf-8", errors="replace").read()
        except OSError:
            return []
        out = []
        line_starts = [0]
        line_starts += [m.end() for m in re.finditer(r"\n", text)]

        def line_of(off):
            import bisect
            return bisect.bisect_right(line_starts, off)

        def emit(cls, off, detail):
            out.append({"file": os.path.relpath(path, self.root),
                        "line": line_of(off), "class": cls,
                        "detail": detail})

        for rx in (MD_LINK_RE, REFDEF_RE, HTML_LINK_RE):
            for m in rx.finditer(text):
                if not self.link_ok(doc_dir, m.group(1)):
                    emit("DEADLINK", m.start(), m.group(1))
        for m in CODESPAN_RE.finditer(text):
            span = m.group(1).strip()
            jm = JUST_CMD_RE.match(span)
            if jm and self.recipes and jm.group(1) not in self.recipes \
                    and jm.group(1) not in ("just", "if", "set"):
                emit("DEADJUST", m.start(), f"just {jm.group(1)}")
            seen_span = False
            for tok in re.split(r"[\s|,]+", span):
                r = self.token(doc_dir, tok)
                if r and not seen_span:
                    emit(r[0], m.start(), r[1])
                    seen_span = True

        lines = text.splitlines()
        for f in out:
            if f["class"] in ("DEADSCRIPT", "DEADREF", "UNRESOLVED"):
                lo = max(0, f["line"] - 4)
                ctx = "\n".join(lines[lo:f["line"] + 3])
                if DELTA_CONTEXT_RE.search(ctx):
                    f["class"] = "MENTIONED"
        return out


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    flags = {a for a in argv[1:] if a.startswith("--")}
    root = repo_root(args[0] if args else ".")
    ck = Checker(root)

    living, snapshot = [], []
    docs = md_files(root)
    for d in docs:
        try:
            head = "".join(open(d, encoding="utf-8",
                                errors="replace").readlines()[:30])
        except OSError:
            continue
        (snapshot if is_snapshot(root, d, head) else living).extend(ck.scan(d))

    if "--fix" in flags:
        # In-place rewrite of dead refs whose basename resolves to exactly
        # one repo path (files moved; prose kept pointing at the old spot).
        # Ambiguous basenames and everything else is left for a human.
        bmap = basenames_map(root)
        fixed = skipped = 0
        per_file = {}
        for f in living:          # never rewrite dated snapshots
            if f["class"] not in ("DEADREF", "DEADLINK"):
                continue
            tok = f["detail"].split("  (", 1)[0]
            # Dated path segments (unredacted_2026-09-25/…) usually denote an
            # external/archive namespace that happens to share a basename —
            # rewriting it to the in-repo file would falsify the record.
            if any(SNAPSHOT_RE.search(seg) or MONTH_RE.search(seg)
                   for seg in tok.split("/")):
                skipped += 1
                continue
            base = os.path.basename(tok.rstrip("/"))
            cand = bmap.get(base, [])
            if len(cand) != 1:
                skipped += 1
                continue
            per_file.setdefault(f["file"], []).append(
                (f["line"], tok, cand[0]))
        for rel, edits in per_file.items():
            p = os.path.join(root, rel)
            try:
                lines = open(p, encoding="utf-8").read().splitlines(True)
            except OSError:
                continue
            dirty = False
            for ln, old, new in edits:
                if ln <= len(lines) and old in lines[ln - 1]:
                    # preserve a doc-relative prefix convention: strip the
                    # repo-root-relative path down if the doc already uses
                    # paths relative to its own dir for siblings
                    lines[ln - 1] = lines[ln - 1].replace(old, new, 1)
                    dirty = True
                    fixed += 1
                else:
                    skipped += 1
            if dirty:
                open(p, "w", encoding="utf-8").writelines(lines)
        print(f"--fix: rewrote {fixed} ref(s), skipped {skipped} ambiguous")

    quiet = "--quiet" in flags
    def keep(f):
        if not quiet:
            return True
        return f["class"] not in ("UNRESOLVED", "EXTPATH", "MENTIONED")

    living = [f for f in living if keep(f)]
    snapshot = [f for f in snapshot if keep(f)]

    if "--json" in flags:
        print(json.dumps({"root": root, "living": living,
                          "snapshot": [] if "--living" in flags else snapshot},
                         indent=1))
    else:
        def dump(fs, title):
            if not fs:
                return
            print(f"--- {title} ---")
            cur = None
            for f in fs:
                if f["file"] != cur:
                    cur = f["file"]
                    print(cur)
                print(f"  :{f['line']}  {f['class']:<10} {f['detail']}")
        dump(living, "living docs")
        if "--living" not in flags:
            dump(snapshot, "dated/historical snapshots (informational)")
        print("===")
        print(f"{len(docs)} docs | living findings: {len(living)} "
              f"| snapshot: {len(snapshot)}")
        if quiet:
            print("(UNRESOLVED/EXTPATH/MENTIONED hidden — drop --quiet to see)")
    return 1 if "--strict" in flags and living else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
