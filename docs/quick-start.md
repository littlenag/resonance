# Quick start

This guide takes about ten minutes. You install Resonance, set it up in a small repository, and then work through your agent: it brings your requirements in, connects them to code and tests, and shows you how what was built traces back to what you asked for.

You need:

- macOS or Linux;
- `git`;
- the Rust toolchain (to build Resonance from source);
- `python3`, only for the example project below;
- [Claude Code](https://claude.com/claude-code), or another agent that speaks MCP.

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
hook post-checkout: created
hook post-merge: created
hook post-rewrite: created
Registered the Resonance MCP server with Claude Code (local scope: this project, this developer).
Agent kit 0.1.0 (stage 1, offline):
  skill: ~/.claude/skills/resonance/SKILL.md (created)
  harness hook and RESONANCE_ACTOR: .claude/settings.local.json (created)
  instructions section: CLAUDE.local.md (created)
```

`init` does four things, and changes no tracked file:

1. mirrors the repository into a graph in `.resonance/`, excluded from git through `.git/info/exclude`;
2. installs a few non-blocking git hooks, so Resonance hears about checkouts, merges and rebases at once;
3. onboards your agent: the MCP server (in Claude Code's local scope, for this project and you only), a skill, a hook that checks your agent's own git commands, and a short `CLAUDE.local.md` section;
4. explains what offline mode can and can't do.

It reads no requirements. That is your agent's job.

For an agent other than Claude Code, run `resonance init --agent generic`: it prints the MCP command line and a section to add to your agent's instructions. Running `init` again refreshes the hooks and the kit, for example after upgrading Resonance.

## 4. Meet your agent

Start Claude Code in the repository, and ask:

> Give me the Resonance orientation.

The agent confirms it can reach Resonance, tells you the mode (offline, stage 1), and says in a few lines what it will now do differently: read why code exists before changing it, record what it builds, keep Resonance invisible to your teammates, and keep the graph healthy.

## 5. Bring your requirements in

> Bring docs/requirements.md into Resonance.

The agent reads the document and shows you a preview before writing anything: the three requirements it found, their keys and text, and any reformatting it suggests. Here the document already has one requirement per list item with its key in bold, so there is nothing to reformat. Once you agree, it brings them in. Each requirement is linked to the lines it came from.

From then on **the graph is the source of truth**, and the document is its readable companion. If you edit the document, or edit a requirement in the TUI, your agent notices that the two have drifted apart and brings them back in step, asking you which way when it matters. See [Your spec documents](spec-documents.md).

## 6. Connect code and tests

> Connect inventory.py and the tests to the requirements, and run the tests.

The agent declares the code units it's talking about (`record`, `normalize`), declares each test with the exact command that runs it alone, links them (`satisfies`, `verifies`) and runs the tests. Passing tests mark their links `verified`. Then ask:

> What implements FR-002, and is it tested?

> Why does normalize exist?

The agent answers from the graph, so you can see exactly how it connected your intent to the code. Resonance never parsed any of it: the agent named the regions, and Resonance keeps them attached as the code changes.

## 7. Leave notes for the next change

> Annotate record: keep it idempotent, callers retry on timeout.

Annotations live only in the graph, never in your files. Before your agent changes `record`, it reads them, and treats an `instruction` or `warning` as a constraint. See [Concepts](concepts.md#annotations).

## 8. Look at the spec yourself

```
$ resonance tui
```

lists your requirements and decisions; `enter` shows one with its links and latest test results, `e` edits it and `a` adds one. Your edits land in a short change log that your agent reads and handles before its next commit. See [The TUI and the change log](tui-and-change-log.md).

## 9. Check on it

```
$ resonance status
```

shows counts, the **repair list** (what needs judgment after code changed, for example a function rewritten beyond recognition), pending change-log entries and the state of the agent kit. Your agent works through the repair list; you can ask it to:

> Work through the Resonance repair list.

When git and the graph disagree after a pull or a rebase, `resonance recover` shows what happened and lets you undo any automatic resolution. See [Recovery](recovery.md).

## Where to go next

- [Concepts](concepts.md): the ideas behind what you just did.
- [Working with your agent](agents.md): what your agent is told, what to ask it, and the gate on its git commands.
- [Sharing the graph](sharing.md): when your team is ready, commit the graph with the code.
- [Removing Resonance](troubleshooting.md#removing-resonance-from-a-repository): `resonance eject` exports your graph, then removes every trace from the repository.
