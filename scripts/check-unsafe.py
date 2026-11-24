#!/usr/bin/env python3
import os
import re
import sys
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src")
LIB = os.path.join(SRC, "lib.rs")
UNSAFE = re.compile(r"\bunsafe\b")
MOD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
PATH = re.compile(r'#\[path\s*=\s*"([^"]+)"\]\s*$')
FORBID = "#![forbid(unsafe_code)]"
ROOT_ATTRS = ("#![deny(unsafe_code)]", "#![deny(unsafe_op_in_unsafe_fn)]")
def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()
def code_of(line):
    s = line.strip()
    if s.startswith("//"):
        return ""
    s = re.sub(r'"(?:\\.|[^"\\])*"', '""', s)
    return s.split("//")[0]
def has_unsafe(path):
    return any(UNSAFE.search(code_of(line)) for line in read(path).splitlines())
def children(path, missing):
    lines = read(path).splitlines()
    here = os.path.dirname(path)
    stem = os.path.splitext(os.path.basename(path))[0]
    base = here if stem in ("mod", "lib") else os.path.join(here, stem)
    kids = []
    for i, line in enumerate(lines):
        m = MOD.match(line)
        if not m:
            continue
        name = m.group(1)
        explicit = None
        for j in range(i - 1, max(-1, i - 4), -1):
            pm = PATH.search(lines[j].strip())
            if pm:
                explicit = os.path.normpath(os.path.join(here, pm.group(1)))
                break
        if explicit:
            kids.append(explicit)
            continue
        a = os.path.join(base, name + ".rs")
        b = os.path.join(base, name, "mod.rs")
        if os.path.exists(a):
            kids.append(a)
        elif os.path.exists(b):
            kids.append(b)
        else:
            missing.append((path, name))
    return kids
class Tree:
    def __init__(self):
        self.missing = []
        self.kids = {}
        self.memo = {}
    def children(self, path):
        if path not in self.kids:
            self.kids[path] = children(path, self.missing)
        return self.kids[path]
    def subtree_unsafe(self, path):
        if path not in self.memo:
            self.memo[path] = has_unsafe(path) or any(
                self.subtree_unsafe(k) for k in self.children(path))
        return self.memo[path]
    def plan(self, path, forbid, keep, every):
        every.append(path)
        for k in self.children(path):
            if self.subtree_unsafe(k):
                if has_unsafe(k):
                    keep.append(k)
                self.plan(k, forbid, keep, every)
            else:
                forbid.append(k)
                self.walk(k, every)
    def walk(self, path, every):
        for k in self.children(path):
            every.append(k)
            self.walk(k, every)
def insert(path):
    with open(path, encoding="utf-8", newline="") as f:
        text = f.read()
    if FORBID in text:
        return False
    nl = "\r\n" if "\r\n" in text else "\n"
    lines = text.split(nl)
    i = 0
    while i < len(lines) and (lines[i].startswith("//!") or (
            lines[i].strip() == "" and i + 1 < len(lines) and lines[i + 1].startswith("//!"))):
        i += 1
    if i == 0:
        new = [FORBID, ""] + lines
    elif i < len(lines) and lines[i].strip() != "":
        new = lines[:i] + ["", FORBID, ""] + lines[i:]
    else:
        new = lines[:i] + ["", FORBID] + lines[i:]
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(nl.join(new))
    return True
def main(argv):
    rel = lambda p: os.path.relpath(p, SRC).replace("\\", "/")
    tree = Tree()
    forbid, keep, every = [], [], []
    tree.plan(LIB, forbid, keep, every)
    problems = []
    lib = read(LIB)
    for attr in ROOT_ATTRS:
        if attr not in lib:
            problems.append(f"src/lib.rs: missing {attr}")
    for path, name in tree.missing:
        problems.append(f"{rel(path)}: module file of `mod {name};` not found")
    if "--apply" in argv:
        n = sum(insert(p) for p in forbid)
        print(f"inserted {FORBID} in {n} files", file=sys.stderr)
    for p in forbid:
        if FORBID not in read(p):
            problems.append(f"{rel(p)}: no unsafe code in its subtree but no {FORBID}")
    if "--list" in argv:
        for p in keep:
            print("UNSAFE", rel(p))
        for p in forbid:
            print("FORBID", rel(p))
    allowed = sum(read(p).count("allow(unsafe_code)") for p in set(every))
    for line in problems:
        print("error:", line, file=sys.stderr)
    print(f"{len(keep)} files with unsafe code, {allowed} #[allow(unsafe_code)] items, "
          f"{len(forbid)} module files forbid it, {len(problems)} problems")
    return 1 if problems else 0
if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
