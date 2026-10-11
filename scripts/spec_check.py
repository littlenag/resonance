#!/usr/bin/env python3
"""Deterministic consistency checks for the Resonance specification.

Checks, over spec/section-*.md, spec/mvp.md and spec/backlog.md:

  ids        No requirement ID (FR, NFR, OQ) is defined twice, and every
             referenced ID is defined somewhere in the full spec.
  coverage   Every FR and NFR defined in the full spec appears in mvp.md or
             backlog.md; every "(partial)" item in mvp.md has a "(remainder)"
             line in backlog.md, and every "(remainder)" refers to an MVP item.
  sections   Every section reference (for example "§8.6.1") points at a
             section heading that exists.
  spelling   No UK spellings in any Markdown file in the repository.

Usage: spec_check.py [ROOT] [--only CHECK[,CHECK...]]
Exits 0 when clean, 1 when any check finds a problem.
Standard library only; supports Python 3.9.
"""

import argparse
import glob
import os
import re
import sys

ID = r"(?:FR|NFR|OQ)-\d+[a-z]?"
DEFINITION = re.compile(r"^\s*- \*\*((?:FR|NFR)-\d+[a-z]?)\*\*|^\| \*\*((?:NFR|OQ)-\d+)\*\*", re.M)
REFERENCE = re.compile(r"\b(" + ID + r")\b")
SECTION_REF = re.compile(r"§(\d+(?:\.\d+)*)")
HEADING = re.compile(r"^#{2,6} (\d+(?:\.\d+)*)[ .]", re.M)
MVP_PARTIAL = re.compile(r"\b((?:FR|NFR)-\d+[a-z]?)(?:\*\*)? \(partial\)|\(((?:FR|NFR)-\d+[a-z]?), partial\)")
BACKLOG_ITEM = re.compile(r"^- \*\*((?:FR|NFR)-\d+[a-z]?)\*\*( \(remainder\))?", re.M)
MVP_ITEM = re.compile(r"\*\*((?:FR|NFR)-\d+[a-z]?)")

# Each pattern is a UK form; matching is case-insensitive on whole words.
UK_SPELLINGS = [
    r"\w*(?:behaviour|colour|favour|honour|labour|neighbour|flavour)\w*",
    r"licence[sd]?", r"defence", r"offence", r"centre[sd]?", r"catalogue[sd]?",
    r"judgement[s]?", r"acknowledgement[s]?", r"artefact[s]?", r"programme[s]?",
    r"whilst", r"amongst", r"learnt", r"spelt",
    r"\w*(?:labelled|labelling|modelled|modelling|travelled|travelling|cancelled|cancelling|signalled|signalling)",
    r"\w*(?:organis|recognis|normalis|serialis|initialis|optimis|minimis|maximis|prioritis|summaris|categoris"
    r"|customis|finalis|standardis|synchronis|materialis|authoris|centralis|visualis|analys)(?:e|es|ed|ing|ation|ations|er|ers)",
]
UK = re.compile(r"\b(?:" + "|".join(UK_SPELLINGS) + r")\b", re.I)


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def rel(root, path):
    return os.path.relpath(path, root)


def check_ids(root, sections, mvp, backlog):
    problems = []
    seen = {}
    for path in sections:
        for m in DEFINITION.finditer(read(path)):
            rid = m.group(1) or m.group(2)
            if rid in seen:
                problems.append("duplicate definition of %s in %s (also in %s)" % (rid, rel(root, path), seen[rid]))
            else:
                seen[rid] = rel(root, path)
    for path in sections + [mvp, backlog]:
        for rid in sorted(set(REFERENCE.findall(read(path)))):
            if rid not in seen:
                problems.append("%s references undefined %s" % (rel(root, path), rid))
    return problems, seen


def check_coverage(root, defined, mvp, backlog):
    problems = []
    mvp_text, backlog_text = read(mvp), read(backlog)
    partials = {a or b for a, b in MVP_PARTIAL.findall(mvp_text)}
    # An MVP item is a bolded ID, or any ID marked partial (for example "**FR-132 (partial) / FR-210 (partial)**").
    in_mvp = set(MVP_ITEM.findall(mvp_text)) | partials
    backlog_ids, remainders = set(), set()
    for m in BACKLOG_ITEM.finditer(backlog_text):
        backlog_ids.add(m.group(1))
        if m.group(2):
            remainders.add(m.group(1))
    for rid in sorted(r for r in defined if not r.startswith("OQ-")):
        if rid not in in_mvp and rid not in backlog_ids:
            problems.append("%s is in neither mvp.md nor backlog.md" % rid)
    for rid in sorted(partials - remainders):
        problems.append("%s is partial in mvp.md but has no (remainder) line in backlog.md" % rid)
    for rid in sorted(remainders - in_mvp):
        problems.append("%s has a (remainder) line in backlog.md but is not in mvp.md" % rid)
    for rid in sorted((in_mvp & backlog_ids) - remainders):
        if rid not in partials:
            problems.append("%s is in both mvp.md and backlog.md without being partial/remainder" % rid)
    return problems


def check_sections(root, sections, mvp, backlog):
    problems = []
    spec_headings = set()
    for path in sections:
        spec_headings.update(h for h in HEADING.findall(read(path)))
        m = re.match(r"section-(\d+)-", os.path.basename(path))
        if m:
            spec_headings.add(m.group(1))
    for path in sections + [mvp, backlog]:
        text = read(path)
        local = set(HEADING.findall(text)) if path in (mvp, backlog) else set()
        for ref in sorted(set(SECTION_REF.findall(text))):
            if ref not in spec_headings and ref not in local:
                problems.append("%s refers to §%s, which does not exist" % (rel(root, path), ref))
    return problems


def check_spelling(root):
    problems = []
    for path in sorted(glob.glob(os.path.join(root, "**", "*.md"), recursive=True)):
        if "/target/" in path or "/.git/" in path:
            continue
        for lineno, line in enumerate(read(path).splitlines(), 1):
            for m in UK.finditer(line):
                problems.append("%s:%d: UK spelling '%s'" % (rel(root, path), lineno, m.group(0)))
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("root", nargs="?", default=".")
    parser.add_argument("--only", default="ids,coverage,sections,spelling")
    args = parser.parse_args()
    root = os.path.abspath(args.root)
    only = set(args.only.split(","))

    sections = sorted(glob.glob(os.path.join(root, "spec", "section-*.md")))
    mvp = os.path.join(root, "spec", "mvp.md")
    backlog = os.path.join(root, "spec", "backlog.md")

    results = []
    id_problems, defined = check_ids(root, sections, mvp, backlog)
    if "ids" in only:
        results.append(("ids", id_problems))
    if "coverage" in only:
        results.append(("coverage", check_coverage(root, defined, mvp, backlog)))
    if "sections" in only:
        results.append(("sections", check_sections(root, sections, mvp, backlog)))
    if "spelling" in only:
        results.append(("spelling", check_spelling(root)))

    failed = False
    for name, problems in results:
        if problems:
            failed = True
            print("%s: %d problem(s)" % (name, len(problems)))
            for p in problems:
                print("  " + p)
        else:
            print("%s: ok" % name)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
