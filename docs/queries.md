# Queries

Every query reflects the files on disk at the moment you ask. Add `--json` to any command for machine-readable output.

## trace: from a requirement to code and tests

```
$ resonance trace FR-001
FR-001 [active, functional]
  Recording an item stores its name and quantity.
  source: docs/requirements.md:3
Satisfied by:
  [asserted] inventory.py::record at inventory.py:12-15  link 01M4PNTS5X
Verified by:
  [verified] tests/test_inventory.py::test_record at tests/test_inventory.py:7-10; last run pass at 2026-10-11T23:47:42.074Z, commit e3216d3b9f  link 01M4PNTSB3
```

`trace` shows the code units and files that satisfy a requirement, the tests that verify it, each link's status, the latest test result, and the decisions that constrain the requirement.

## why: from code to its reasons

```
$ resonance why record
Code unit inventory.py::record (function)
  at inventory.py:12-15 [attached]
  inside: inventory.py
Satisfies:
  [asserted] FR-001: Recording an item stores its name and quantity.  link 01M4PNTS5X
Constrained by:
  [asserted] Store items in memory: Keep items in a dict; no database yet
Covered by tests:
  (none)
Annotations:
  instruction: keep record() idempotent: callers retry on timeout
```

`why` takes a code unit, a test or a file. It shows:

- the requirements it satisfies;
- the decisions that constrain it, or any unit or file containing it;
- the tests that verify this unit directly (tests that verify its requirements appear in `trace`);
- for a test, what it verifies;
- the annotations on it and on everything that contains it.

Run `why` before changing code you didn't write.

## context: everything for a task

```
resonance context FR-001 FR-002
resonance context FR-003 --task unicode-names
```

A **context pack** gathers what someone (usually an agent) needs before working on a set of requirements:

- the requirements, with their text and source;
- the decisions that constrain them or their code;
- the code that satisfies them, as locations (path, lines and address) rather than full text;
- the tests that verify them, with their run commands and latest results;
- the annotations in scope;
- with `--task`, every call-out made for that task label.

## unverified: gaps in testing

```
resonance unverified
```

Lists every active requirement that has no verified test, with the tests linked so far and how many units satisfy it.

## search: find anything

```
resonance search memory                      # text, case-insensitive
resonance search --type CodeUnit             # every code unit
resonance search --type Requirement --status retired
resonance search --status stale              # everything with a stale link
resonance search inventory --type Test --limit 10
```

| Option | Values |
| --- | --- |
| text | Any text, matched case-insensitively against names, keys, titles, text and paths. Leave it out to match everything. |
| `--type` | `Requirement`, `Decision`, `CodeUnit`, `Test`, `Annotation`, `File` or `Evidence` |
| `--status` | A node status (`active`, `retired`, `attached`, `orphaned`, `removed`), or a link status (`asserted`, `verified`, `stale`, `orphaned`), which finds nodes with at least one link in that status. |
| `--limit` | Maximum results (default 50). |

## show: one item in full

```
resonance show FR-002
resonance show record
resonance show 01M4PNTS5X      # a link
```

`show` prints every property of a node or link, plus its links and annotations. Use it to look at a link's evidence, an anchor's details or a requirement's source.

## status: health at a glance

```
resonance status
```

Shows the counts of everything in the graph, the imported documents, when the graph was last brought up to date, and the [repair list](repair-list.md).

## unit list: what's declared

```
resonance unit list [path]
```

Lists code units and tests with their locations, optionally under one file or directory.
