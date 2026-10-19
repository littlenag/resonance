# Concepts

Resonance keeps a **graph**: a set of things (nodes) and typed links between them. Your agent builds and reads it through the MCP server; you see it in `resonance tui` and through your agent. This page explains each kind of node and link, and the ideas that tie them together.

## The graph and your files

Resonance never writes to your source files, and it parses none of them. It keeps a copy of every file, so that it can follow the regions your agent declared as files change, whichever tool changes them. Your spec, though, lives in the graph: once your agent has brought a requirement in, the graph is its source of truth, and the document it came from is kept as a linked, readable companion (see [Your spec documents](spec-documents.md)).

The graph is stored in `.resonance/` at the root of the repository. `resonance init` adds `/.resonance/` to `.git/info/exclude`, a local ignore file that git never shares, so the graph is never committed and `git status` is unchanged, until you choose to [share it](sharing.md).

This is **offline mode**: the graph is private to you, on your machine. Nothing is shared with teammates, and nothing in the repository reveals that you use Resonance. Graph changes you or your agent make on a branch stay with that branch: when you switch branches, they follow, and when commits are rebased or amended, they follow those too.

## Spec items

Requirements, decisions, designs and constraints are **spec items**. Each has a **key** (`FR-042`, `ADR-0007`, `DES-3`), a status, and its own fields. Spec items come from your documents, brought in by your agent, from your conversations with your agent, and from `resonance tui`. They are never deleted: retiring one keeps its history and its links.

| Item | What it says | Typical fields |
| --- | --- | --- |
| **Requirement** | What the software must do. Keys starting with `NFR-` are non-functional. | text, kind, priority, status (`draft`, `active`, `superseded`, `retired`) |
| **Decision** | A choice and its reasons. | title, decision, context, consequences, status (`proposed`, `accepted`, `superseded`, `deprecated`, `retired`) |
| **Design** | How something is built. Code *realizes* it. | title, text, status |
| **Constraint** | A limit on how something is built. It *constrains* what it applies to. | title, text, scope, status |

An item can be **superseded** by a newer one of the same kind; links to the old one then become stale, so that they are checked against the new one.

## Files, code units and tests

Every file Resonance reads is a node, identified by its path. Ignored files (see [Configuration](configuration.md#which-files-are-read)) are not part of the graph. Files containing NUL bytes or larger than `ingest.maxBlobBytes` are recorded by hash and size only.

A **code unit** is a named region of a file that your agent declared: a function, a type, a block, a configuration stanza, or a paragraph of a spec document. Each has a name, an **address** (`<path>::<name>`, such as `inventory.py::record`), a free-text kind, and an **anchor**: the region's location plus a copy of its text and a few lines of context. Code units nest: annotations and decisions on an outer unit (or on the file) also apply to the units inside it.

A **test** is a code unit with a **run command**: the exact shell command that runs that test alone. Resonance doesn't discover tests; whoever declares one supplies its command. Exit code 0 means the test passed.

### How Resonance follows code

After every change to a file, Resonance re-locates every unit in it, trying these steps in order:

1. **Follow the edit.** If at least half of the unit's lines survive (a lightly edited line counts), the unit follows them, including lines inserted inside it.
2. **Find the same text in the same file**, for example after a function moved further down. Surrounding lines decide between several matches.
3. **Find the same text in another file**, when the region moved there.
4. **Find similar text in the same file** (at least 80% similar, with agreeing surroundings). A match below 90% goes on the repair list for someone to confirm.
5. **Orphan.** If nothing matches, the unit is orphaned, and so are its links and annotations. Nothing is deleted, and everything goes on the repair list with its last known location and text.

Renamed and moved files take their units with them. Orphaning is reversible: if the exact text comes back (switching back to a branch, undoing a deletion), the unit and its links are restored. When one piece of code becomes several, or several become one, your agent records a **split** or a **merge**, and the links and annotations go to the right place.

## Links

| Link | From | To | Read as |
| --- | --- | --- | --- |
| `satisfies` | code unit, test or file | requirement | this code implements that requirement |
| `verifies` | test | requirement or code unit | this test checks that requirement or code |
| `constrains` | decision or constraint | requirement, code unit, test, file or decision | this limits that thing |
| `realizes` | code unit or file | design | this code builds that design |
| `derivedFrom` | requirement or design | requirement | this refines that requirement |
| `derivedFrom` | spec item | code unit or file | this item came from that region of a document |
| `supersedes` | spec item | spec item of the same kind | this replaces that |
| `annotates` | annotation | any node | this note is about that thing |

Seen from the other end, links read backward: *satisfied by*, *verified by*, *constrained by*. Each link records who made it (a person or an agent), when, and for an agent's links, how confident the agent was.

### Link statuses

| Status | Meaning |
| --- | --- |
| `asserted` | Someone said the link holds. New links start here. |
| `verified` | A test run backs it up: the test passed, and neither end has changed since. Only `verifies` links become verified. |
| `stale` | Something it depended on changed: a spec item's text, either end of a verified link, or either side of an item's link to its document. Someone should check it. |
| `orphaned` | One end of it disappeared. It keeps the last known location. |

Editing code under an `asserted` link leaves it `asserted`, so routine edits don't create noise.

### Verification and evidence

When your agent runs a requirement's tests, each run is recorded as **evidence**: pass, fail or error, the exit code, the checked-out commit, the time and the end of the output. A pass marks the test's `verifies` links `verified`; a fail returns them to `asserted` with the failing evidence attached; an error (the command could not start, ran longer than `test.timeoutSeconds`, or printed `test.zeroTestsPattern`, meaning no test actually ran) is never a pass.

## Annotations

An **annotation** is a note attached to a code unit, a test, a file or a spec item. It lives only in the graph, never in your files.

| Kind | Use it for | How agents treat it |
| --- | --- | --- |
| `note` | Background and explanation. | Information. |
| `warning` | Something fragile or surprising. | A constraint on changes. |
| `instruction` | A rule for anyone changing this code. | A constraint on changes. |
| `call-out` | Code that matters for a particular task, optionally named. | Pulled into the agent's context for that task. |

Annotations follow their code through edits, renames and moves, and are orphaned (and listed for repair) when it disappears. Ask your agent to add them ("annotate `record`: keep it idempotent"), or to call out code for a task.

## The repair list

When something changes in a way Resonance can't settle on its own, it goes on the **repair list**: orphaned units, links and annotations; uncertain re-anchors; stale links, including items that drifted from their documents; and retired requirements that still have links. Resolving these is a judgment call, so Resonance leaves it to your agent, which asks you when unsure. `resonance status` shows the list; ask your agent to work through it.

## The daemon

A background process, one per repository, watches your files and git, and keeps the graph current. You never need to start it: the first request does. Every answer reflects the files on disk at the moment it was asked, including edits made a moment before by any editor or agent. See [The daemon](daemon.md).
