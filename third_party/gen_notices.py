#!/usr/bin/env python3
import hashlib
import json
import os
import sys
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MANIFEST = os.path.join(ROOT, "third_party", "manifest.json")
OUTPUT = os.path.join(ROOT, "NOTICES")
REQUIRED = ("id", "title", "kind", "paths", "upstream", "copyright", "license",
            "license_files", "notice", "status", "text")
KINDS = {"translated-code", "data-table", "terminology", "standard-content",
         "attribution", "test-vector"}
NOTICE = {"required", "courtesy", "none"}
STATUS = {"ok", "decision"}
WIDTH = 78
def wrap(text, indent=""):
    words = text.split()
    lines, line = [], indent
    for w in words:
        if len(line) + len(w) + (0 if line == indent else 1) > WIDTH and line != indent:
            lines.append(line)
            line = indent + w
        else:
            line = line + ("" if line == indent else " ") + w
    if line.strip():
        lines.append(line)
    return lines
def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()
def read_text(rel):
    with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
        return f.read().replace("\r\n", "\n").rstrip("\n")
def validate(manifest):
    errors, pending = [], []
    seen = set()
    for e in manifest.get("entries", []):
        eid = e.get("id", "?")
        for k in REQUIRED:
            if k not in e:
                errors.append(f"{eid}: missing field {k}")
        if eid in seen:
            errors.append(f"{eid}: duplicate id")
        seen.add(eid)
        if e.get("kind") not in KINDS:
            errors.append(f"{eid}: unknown kind {e.get('kind')!r}")
        if e.get("notice") not in NOTICE:
            errors.append(f"{eid}: unknown notice {e.get('notice')!r}")
        if e.get("status") not in STATUS:
            errors.append(f"{eid}: unknown status {e.get('status')!r}")
        if e.get("notice") == "required" and not e.get("license_files") and not e.get("copyright"):
            errors.append(f"{eid}: a required notice needs a licence file or a copyright line")
        for p in e.get("paths", []):
            full = os.path.join(ROOT, p.rstrip("/"))
            if p.endswith("/") and not os.path.isdir(full):
                errors.append(f"{eid}: missing directory {p}")
            elif not p.endswith("/") and not os.path.isfile(full):
                errors.append(f"{eid}: missing file {p}")
        for p, want in e.get("sha256", {}).items():
            full = os.path.join(ROOT, p)
            if not os.path.isfile(full):
                errors.append(f"{eid}: pinned file missing {p}")
            elif sha256(full) != want:
                errors.append(f"{eid}: {p} SHA-256 {sha256(full)} != pinned {want}")
        for p in e.get("license_files", []):
            full = os.path.join(ROOT, p)
            if not os.path.isfile(full):
                errors.append(f"{eid}: missing licence file {p}")
            elif read_text(p).lstrip().startswith("PENDING"):
                pending.append(f"{eid}: licence file {p} is a PENDING placeholder")
        if e.get("status") == "decision":
            pending.append(f"{eid}: status decision (licence terms not yet settled)")
    return errors, pending
def render(manifest):
    entries = manifest["entries"]
    out = []
    out.append("NOTICES")
    out.append("")
    out.append("Copyright 2026 Thales Matheus Mendonça Santos")
    out.append("Licensed under the Apache License, Version 2.0 (see LICENSE).")
    out.append("")
    out += wrap("This file lists the material in this package that is derived from "
                "other works and reproduces the notices that their licences or terms "
                "of use require. The package's own licence is declared in the "
                "`license` field of Cargo.toml. This file is generated from "
                "third_party/manifest.json by third_party/gen_notices.py; edit the "
                "manifest, not this file.")
    out.append("")
    out.append("Contents")
    for i, e in enumerate(entries, 1):
        out.append(f"  {i:2}. {e['title']} [{e['license']}; notice {e['notice']}]")
    out.append("")
    used_files = []
    for i, e in enumerate(entries, 1):
        out.append("=" * WIDTH)
        out.append(f"{i}. {e['title']}")
        out.append("=" * WIDTH)
        out.append("")
        up = e["upstream"]
        out += wrap(f"Source: {up['project']}", "")
        out += wrap(f"Location: {up['url']}", "")
        out += wrap(f"Revision: {up['revision']}", "")
        out += wrap(f"Licence: {e['license']}", "")
        out.append(f"Notice: {e['notice']}")
        if e["status"] == "decision":
            out.append("Status: DECISION PENDING (see third_party/manifest.json)")
        out.append("Files in this package:")
        for p in e["paths"]:
            out.append(f"  {p}")
        out.append("")
        for c in e["copyright"]:
            out += wrap(c)
        if e["copyright"]:
            out.append("")
        out += wrap(e["text"])
        out.append("")
        if e["license_files"]:
            out.append("Licence and notice texts: " + ", ".join(
                f"[{os.path.basename(p)}]" for p in e["license_files"]))
            out.append("")
            for p in e["license_files"]:
                if p not in used_files:
                    used_files.append(p)
    out.append("=" * WIDTH)
    out.append("Licence and notice texts")
    out.append("=" * WIDTH)
    for p in used_files:
        out.append("")
        out.append("-" * WIDTH)
        out.append(f"[{os.path.basename(p)}]  ({p})")
        out.append("-" * WIDTH)
        out.append("")
        out.append(read_text(p))
    out.append("")
    return "\n".join(out)
def main(argv):
    check = "--check" in argv
    strict = "--strict" in argv
    with open(MANIFEST, encoding="utf-8") as f:
        manifest = json.load(f)
    errors, pending = validate(manifest)
    for p in pending:
        print("pending:", p, file=sys.stderr)
    if errors:
        for e in errors:
            print("error:", e, file=sys.stderr)
        return 1
    text = render(manifest)
    if check:
        try:
            with open(OUTPUT, encoding="utf-8", newline="") as f:
                current = f.read().replace("\r\n", "\n")
        except FileNotFoundError:
            current = None
        if current != text:
            print("error: NOTICES is out of date; run third_party/gen_notices.py", file=sys.stderr)
            return 1
        print(f"NOTICES up to date: {len(manifest['entries'])} entries, {len(pending)} pending")
    else:
        with open(OUTPUT, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print(f"wrote {OUTPUT}: {len(manifest['entries'])} entries, {len(pending)} pending")
    if strict and pending:
        return 1
    return 0
if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
