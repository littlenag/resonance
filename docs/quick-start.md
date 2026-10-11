# Quick start

This guide takes about fifteen minutes. You install Resonance, set it up in a small repository, record a few requirements, connect them to code and tests, and see what Resonance tells you as the code changes. At the end you connect Claude Code so your agent can do the same.

You need:

- macOS or Linux;
- `git`;
- the Rust toolchain (to build Resonance from source);
- `python3`, only for the example project below.

## 1. Install

Build and install the `resonance` binary from the source repository:

```
git clone https://github.com/littlenag/resonance.git
cd resonance
cargo install --locked --path crates/resonance-cli
```

Check that it is on your `PATH`:

```
$ resonance version
resonance 0.1.0
store schema version: 1
MCP protocol version: 2025-11-25 (also accepts 2025-06-18, 2025-03-26, 2024-11-05)
```

Installing the binary changes nothing else on your machine.

## 2. Create an example project

Any git repository works. To follow along exactly, create this small Python project:

```
mkdir inventory && cd inventory
git init
mkdir docs tests
touch tests/__init__.py
```

`docs/requirements.md`:

```markdown
# Inventory requirements

- **FR-001** Recording an item stores its name and quantity.
- **FR-002** Negative quantities are rejected.
- **FR-003** Item names are trimmed and lower-cased.
```

`inventory.py`:

```python
"""A tiny in-memory inventory."""


def normalize(name):
    return name.strip().lower()


class Inventory:
    def __init__(self):
        self.items = {}

    def record(self, name, quantity):
        if quantity < 0:
            raise ValueError("quantity must not be negative")
        self.items[normalize(name)] = quantity

    def total(self):
        return sum(self.items.values())
```

`tests/test_inventory.py`:

```python
import unittest

from inventory import Inventory


class InventoryTest(unittest.TestCase):
    def test_record(self):
        inv = Inventory()
        inv.record("  Apples ", 3)
        self.assertEqual(inv.items, {"apples": 3})

    def test_rejects_negative(self):
        with self.assertRaises(ValueError):
            Inventory().record("pears", -1)


if __name__ == "__main__":
    unittest.main()
```

Commit it:

```
git add -A && git commit -m "Inventory"
```

## 3. Set up Resonance

```
$ resonance init
Initialized Resonance in /home/you/inventory/.resonance (offline mode).
Added /.resonance/ to .git/info/exclude; `git status` is unchanged.
...
Files: 4   Requirements: 0 active, 0 retired   Decisions: 0
Code units: 0   Tests: 0   Annotations: 0   Test runs recorded: 0
Links: 0 asserted, 0 verified, 0 stale, 0 orphaned
```

`init` creates `.resonance/` beside your code, adds it to `.git/info/exclude` so git never sees it, and reads every file. It may ask whether to register Resonance with Claude Code; answer no for now (step 10 covers it).

Check that git sees nothing:

```
$ git status --short
$
```

## 4. Import the requirements

```
$ resonance import docs/requirements.md
Now importing from: docs/requirements.md
3 requirements defined in 1 source.
Requirements added: FR-001, FR-002, FR-003
Changes to these files are picked up automatically by every command.
```

From now on, edits to `docs/requirements.md` are picked up automatically. You never need to import it again.

## 5. Declare code units and tests

Resonance does not parse your code. You tell it which regions matter by declaring them as **code units**, by line range or by an exact snippet:

```
$ resonance unit add inventory.py:12-15 --name record --kind function
Code unit inventory.py::record (function)
  at inventory.py:12-15 [attached]
  id: 01M4PNTRWSVMAKQ51XXSP1N6ZR

$ resonance unit add inventory.py --match 'if quantity < 0:' --name negative_check --kind block
Code unit inventory.py::negative_check (block)
  at inventory.py:13 [attached]

$ resonance unit add inventory.py:4-5 --name normalize --kind function
```

A **test** is a code unit plus the exact command that runs that test alone:

```
$ resonance test add tests/test_inventory.py:7-10 --name test_record \
    --run 'python3 -m unittest tests.test_inventory.InventoryTest.test_record'

$ resonance test add tests/test_inventory.py:12-14 --name test_rejects_negative \
    --run 'python3 -m unittest tests.test_inventory.InventoryTest.test_rejects_negative'
```

List what you declared:

```
$ resonance unit list
inventory.py:4-5                 inventory.py::normalize                  unit  function   attached
inventory.py:12-15               inventory.py::record                     unit  function   attached
inventory.py:13                  inventory.py::negative_check             unit  block      attached
tests/test_inventory.py:7-10     tests/test_inventory.py::test_record     test  test       attached
tests/test_inventory.py:12-14    tests/test_inventory.py::test_rejects_negative test  test       attached
```

IDs in your output will differ from the ones shown here. You rarely need them: a unique name such as `record` is enough.

## 6. Link code and tests to requirements

Code **satisfies** a requirement; a test **verifies** one:

```
$ resonance link record satisfies FR-001
Linked: inventory.py::record [asserted] satisfies FR-001 (Requirement)  link 01M4PNTS5X

$ resonance link negative_check satisfies FR-002
$ resonance link normalize satisfies FR-003
$ resonance link test_record verifies FR-001
$ resonance link test_record verifies FR-003
$ resonance link test_rejects_negative verifies FR-002
```

Every new link is `asserted`: someone said it holds, but nothing has checked it yet.

## 7. Verify with tests

`resonance unverified` lists requirements that no passing test backs up:

```
$ resonance unverified
3 requirements with no verified test:
  FR-001    Recording an item stores its name and quantity.  [tests/test_inventory.py::test_record (asserted); 1 satisfying]
  FR-002    Negative quantities are rejected.  [tests/test_inventory.py::test_rejects_negative (asserted); 1 satisfying]
  FR-003    Item names are trimmed and lower-cased.  [tests/test_inventory.py::test_record (asserted); 1 satisfying]
```

Run the tests linked to a requirement. A pass marks their links `verified` and records the result as evidence:

```
$ resonance test FR-001
PASS  tests/test_inventory.py::test_record (174 ms)
      $ python3 -m unittest tests.test_inventory.InventoryTest.test_record
      verifies FR-001 -> verified
      verifies FR-003 -> verified
1 passed, 0 failed, 0 errors.

$ resonance test FR-002
$ resonance unverified
Every active requirement has a verified test.
```

## 8. Ask questions

**Trace** a requirement to the code and tests behind it:

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

Leave notes on code with **annotations**, and record **decisions**:

```
$ resonance annotate record --kind instruction --text 'keep record() idempotent: callers retry on timeout'
$ resonance decision add --title 'Store items in memory' --decision 'Keep items in a dict; no database yet'
$ resonance link 'Store items in memory' constrains record
```

Ask **why** a piece of code exists:

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

`resonance context FR-001 FR-002` gathers everything about a set of requirements into one **context pack**: the requirements, decisions, code locations, tests with their latest results, and annotations.

## 9. Change things and watch Resonance keep up

Resonance follows your files as they change, whatever editor or tool changes them. In your editor, add an import and a logger below the docstring at the top of `inventory.py`, for example:

```python
"""A tiny in-memory inventory."""

import logging

log = logging.getLogger(__name__)


def normalize(name):
```

Save, and ask again:

```
$ resonance why record
Code unit inventory.py::record (function)
  at inventory.py:16-19 [attached]
...
```

The unit moved with its code. (Your line numbers depend on how many lines you added.) Rename the file and it follows again:

```
$ git mv inventory.py stock.py
$ resonance unit list
stock.py:8-9                     stock.py::normalize                      unit  function   attached
stock.py:16-19                   stock.py::record                         unit  function   attached
...
```

Now change a requirement. In `docs/requirements.md`, edit FR-003 to read "Item names are trimmed, lower-cased and never empty." Its links become `stale`, because what they promised has changed:

```
$ resonance trace FR-003
FR-003 [active, functional]
  Item names are trimmed, lower-cased and never empty.
  source: docs/requirements.md:5
Satisfied by:
  [stale] stock.py::normalize at stock.py:8-9  link 01M4PNTS8X
Verified by:
  [stale] tests/test_inventory.py::test_record at tests/test_inventory.py:7-10; ...  link 01M4PNTSCN
```

`resonance status` shows the counts and the **repair list**, which holds everything that needs a person's or an agent's judgment, each item with a suggested command:

```
$ resonance status
...
Repair list (2):
  [stale link] stock.py::normalize satisfies FR-003: an endpoint changed
      | def normalize(name):
      |     return name.strip().lower()
      -> check that it still holds and run `resonance confirm 01M4PNTS8X0DTFQ0SPMV9KC8AN`, or `resonance unlink 01M4PNTS8X0DTFQ0SPMV9KC8AN`
  ...
```

Delete code that a unit points to and the unit becomes `orphaned`, with its last known location kept on the repair list. Put the code back (for example by switching back to a branch) and the unit is restored. See [The repair list](repair-list.md).

When you're done experimenting, rename the file back with `git mv stock.py inventory.py`, since the example's tests import `inventory`.

## 10. Connect Claude Code

```
$ resonance agent install
```

This registers Resonance's MCP server with Claude Code for this project only (Claude Code's *local* scope). It shows the command it will run and asks first; nothing in your repository changes. Start Claude Code in the repository and your agent can trace requirements, ask why code exists, declare and link what it writes, and run linked tests. See [Working with agents](agents.md).

## Where to go next

- [Concepts](concepts.md): the ideas behind what you just did.
- [Code units and tests](code-units-and-tests.md): how anchors follow edits, and what to do when they can't.
- [CLI reference](cli-reference.md): every command and option.
- [Removing Resonance](troubleshooting.md#removing-resonance-from-a-repository): `resonance eject` exports your graph, then removes every trace from the repository.
