# Concepts

Resonance keeps a **graph**: a set of things (nodes) and typed links between them. This page explains each kind of node and link, and the ideas that tie them together.

## The graph and your files

Your files on disk and your git history are always the source of truth. The graph follows them: whenever a file changes, Resonance updates its copy, works out where your declared code now is, and updates link statuses. Resonance never writes to your source files.

The graph is stored in `.resonance/` at the root of the repository. `resonance init` adds `/.resonance/` to `.git/info/exclude`, a local ignore file that git never shares, so the graph is never committed and `git status` is unchanged, until you choose to [share it](sharing.md).

This is **offline mode**: the graph is private to you, on your machine. Nothing is shared with teammates, and nothing in the repository reveals that you use Resonance.

Resonance keeps one graph per repository, whichever branch is checked out. When you switch branches, pull or rebase, Resonance treats the result like any other change to your files.

## Nodes

### Requirements

A requirement is a statement of what the software must do, identified by a **key** such as `FR-001` or `NFR-002`. Requirements usually come from Markdown documents that you import. Resonance keeps them in sync as the documents change. You can also author them directly with `resonance req add`.

A requirement is `active` or `retired`. Requirements are never deleted: one that disappears from its document is retired, so its history and links are kept.

Keys starting with `NFR-` are non-functional requirements; all others are functional.

See [Requirements and decisions](requirements-and-decisions.md).

### Decisions

A decision records a choice and its reasons: a title, what was decided, and optionally its context and consequences. A decision can **constrain** requirements, code units, tests, files or other decisions, and then appears whenever someone asks why that code exists.

### Designs and constraints

A design says how something is built, and code **realizes** it. A constraint limits how, and **constrains** the things it applies to. Like requirements and decisions, they can be retired and superseded, never deleted.

### Files

Every file Resonance reads is a node, identified by its path. Links and annotations can target whole files as well as code units. Ignored files (see [Configuration](configuration.md#which-files-are-read)) are not part of the graph.

### Code units

A **code unit** is a named region of a file that a person or an agent declared: a function, a type, a block, a configuration stanza or anything else worth talking about. Resonance does not parse code, so it only knows about the code units you declare.

Each code unit has:

- a **name**, such as `record`;
- an **address**, `<path>::<name>`, such as `inventory.py::record`;
- a free-text **kind**, such as `function`, `type` or `block`;
- an **anchor**, the region's location plus a copy of its text and a few lines of context.

When the file changes, Resonance uses the anchor to find where the region went. A region that cannot be found is **orphaned**, which is reversible: if the text comes back, the unit is restored. See [Code units and tests](code-units-and-tests.md).

Code units nest. A unit whose region lies inside another unit (or inside a file) is *contained* by it, and annotations and decisions on the outer unit also apply to the inner one.

### Tests

A **test** is a code unit with a **run command**: the exact shell command that runs that test alone. Resonance doesn't discover tests; whoever declares a test supplies its command. Exit code 0 means the test passed.

### Evidence

Each time Resonance runs a test, it records the result as **evidence**: pass, fail or error, the exit code, the commit that was checked out, the time, and the end of the output. Evidence is attached to the links the test supports.

### Annotations

An **annotation** is a note attached to a code unit, a file or another node. Its kind is one of `note`, `warning`, `call-out` (optionally for a named task) or `instruction`. Agents treat warnings and instructions as constraints on their changes. See [Annotations](annotations.md).

## Links

| Link | From | To | Read as |
| --- | --- | --- | --- |
| `satisfies` | code unit, test or file | requirement | this code implements that requirement |
| `verifies` | test | requirement or code unit | this test checks that requirement or code |
| `constrains` | decision or constraint | requirement, code unit, test, file or decision | this limits that thing |
| `realizes` | code unit or file | design | this code builds that design |
| `derivedFrom` | requirement | requirement | this requirement refines that one |
| `supersedes` | spec item | spec item of the same kind | this replaces that; links to the old one become stale |
| `annotates` | annotation | any node | this note is about that thing |

You create `satisfies`, `verifies` and `constrains` links with `resonance link`; `annotates` links are created by `resonance annotate`. When displayed from the other end, links use reverse names: *satisfied by*, *verified by*, *constrained by*.

Each link records who made it (`provenance`: a person or an agent), the author, when, and, for links an agent makes, how confident the agent was.

### Link statuses

Every link has exactly one status:

| Status | Meaning |
| --- | --- |
| `asserted` | Someone said the link holds. New links start here. |
| `verified` | A test run backs it up: the test passed, and neither end has changed since. Only `verifies` links become verified. |
| `stale` | Something it depended on changed: the requirement's text changed, or either end of a verified link changed. Someone should check it. |
| `orphaned` | One end of it disappeared, for example a code unit whose code was deleted. It keeps the last known location. |

Editing code under an `asserted` link leaves it `asserted`, so routine edits don't create noise. See [Links and verification](links-and-verification.md).

## References

Wherever a command takes a node or a link, you can name it by:

- a requirement key: `FR-001`;
- a code unit address: `inventory.py::record` (or `inventory.py::record#2` for the second unit with that name in the file);
- a bare unit or test name, when it is unique: `record`;
- a file path, relative to the repository root or to your current directory: `inventory.py`;
- a decision title, ignoring case: `'Store items in memory'`;
- an ID, or a unique prefix of one of at least six characters, like a short git hash: `01M4PNTS5X`.

If a reference matches more than one thing, Resonance lists the candidates and asks you to be more specific.

## The repair list

When code or requirements change in ways Resonance can't settle on its own, the affected items go on the **repair list**: orphaned units, links and annotations, uncertain re-anchors, stale links, unreadable requirement documents, and retired requirements that still have links. `resonance status` shows it, with a suggested command for each item. Resolving these items is a judgment call, so Resonance leaves it to you or your agent. See [The repair list](repair-list.md).

## The daemon

Commands are served by a background process, one per repository, that watches your files and keeps the graph current. You never need to start it: the first command does. Every answer reflects the files on disk at the moment you asked, including edits made a moment before by any editor or agent. See [The daemon](daemon.md).
