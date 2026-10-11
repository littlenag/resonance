# CLI reference

```
resonance [GLOBAL OPTIONS] <COMMAND> [ARGS]
```

`resonance help <command>` and `resonance <command> --help` show the same information in your terminal.

## Global options

| Option | Meaning |
| --- | --- |
| `-C <path>` | Run as if Resonance was started in `<path>`. |
| `--json` | Print machine-readable JSON instead of text. |
| `--cached` | Answer from the graph as it is, without waiting for recent file changes to be applied. |
| `--no-daemon` | Run in this process instead of through the daemon (also `RESONANCE_NO_DAEMON=1`). |
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version. |

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | An error, including a reference that matched nothing or several things, and any test run that didn't pass. |
| `2` | A usage error: a missing or invalid argument. |

## References

Arguments called `<ref>` below accept any of:

- a requirement key: `FR-001`;
- a unit or test address: `src/store.rs::open`, or `src/store.rs::open#2` for the second unit with that name;
- a bare unit or test name, when unique: `open`;
- a file path: `src/store.rs`;
- a decision title, ignoring case: `"Store items in memory"`;
- an ID, or a unique prefix of at least six characters: `01M4PNTS5X`.

Links are referred to by ID or ID prefix. See [Concepts](concepts.md#references).

## Regions

Where a command takes a `<region>`:

| Form | Meaning |
| --- | --- |
| `path:START-END` | Lines START to END, counted from 1, both included. |
| `path:LINE` | One line. |
| `path` with `--match TEXT` | An exact snippet that occurs exactly once in the file. |

---

## Setup and life cycle

### init

```
resonance init [--agent | --no-agent]
```

Sets up Resonance in the current repository: creates `.resonance/` (the graph and `config.toml`), adds `/.resonance/` to `.git/info/exclude`, and reads every file. It changes no tracked file. Running it again changes nothing.

In a terminal, it offers to register the MCP server with Claude Code. `--agent` registers it without asking; `--no-agent` doesn't offer.

### status

```
resonance status
```

Counts of everything in the graph, the imported documents, the time of the last update, and the [repair list](repair-list.md).

### reconcile

```
resonance reconcile [--full]
```

Brings the graph up to date with the disk now. Every command already does this, so you rarely need it. `--full` re-reads every file, ignoring sizes and modification times.

### rebuild-derived

```
resonance rebuild-derived
```

Drops the data Resonance computes from your files (which units contain which, and the cached file contents) and rebuilds it. Nothing you or your agent authored is touched.

### export

```
resonance export [--out FILE]
```

Writes everything you and your agents authored (requirements, decisions, code units, tests, links, annotations and test evidence) to standard output or to `FILE`. The output is JSON Lines: a header line, then one operation per line that recreates the current state. File contents aren't included, since they're in git.

### eject

```
resonance eject [--dry-run] [--yes] [--out FILE]
```

Removes Resonance from the repository:

1. exports the graph to `FILE`, by default `~/.resonance/exports/<repository>-<time>.jsonl`;
2. stops the daemon;
3. removes `.resonance/`;
4. removes the `/.resonance/` line from `.git/info/exclude`.

It asks before removing anything; `--yes` skips the question, and `--dry-run` only shows the plan.

### version

```
resonance version [--json]
```

The binary's version, the graph's schema version, and the MCP protocol versions supported.

### doctor

```
resonance doctor
```

Checks this repository (the graph's integrity and schema version, and the git exclusion) and the health of every Resonance daemon on this machine. See [The daemon](daemon.md#health-of-every-daemon-doctor).

---

## Requirements and decisions

### import

```
resonance import <paths…>
```

Imports requirements from Markdown documents and keeps them in sync from then on. See [Requirements and decisions](requirements-and-decisions.md).

### req add

```
resonance req add <KEY> --text TEXT [--kind functional|non-functional]
```

Creates a requirement. The kind defaults from the key: `NFR-` keys are non-functional.

### req edit

```
resonance req edit <ref> --text TEXT
```

Changes the text of a requirement created with `req add`. Its links become stale. Imported requirements are edited in their documents.

### req retire

```
resonance req retire <ref>
```

Retires a requirement created with `req add`. Requirements are never deleted.

### decision add

```
resonance decision add --title TITLE --decision TEXT [--context TEXT] [--consequences TEXT]
```

Records a decision, with the status `accepted`.

### decision edit

```
resonance decision edit <ref> [--title T] [--decision D] [--context C] [--consequences X] [--status S]
```

Changes any field of a decision, including its status (for example `superseded` or `rejected`).

---

## Code units and tests

### unit add

```
resonance unit add <region> [--match TEXT] --name NAME [--kind KIND]
```

Declares a code unit. The kind is free text and defaults to `block`. See [Code units and tests](code-units-and-tests.md).

### test add

```
resonance test add <region> [--match TEXT] --name NAME [--kind KIND] [--run COMMAND]
```

Declares a test. `--run` is the exact shell command that runs this test alone, from the repository root. If it's omitted, `test.command` from the configuration is used. The kind defaults to `test`.

### unit edit

```
resonance unit edit <ref> [<region>] [--match TEXT] [--name NAME] [--kind KIND] [--run COMMAND]
```

Changes a code unit or test:

- with a region (or `--match`), re-anchors it there, which also restores an orphaned unit and its links;
- `--name`, `--kind` and `--run` (tests only) change those fields;
- with no options, confirms that its current anchor is correct, which clears a low-confidence anchor from the repair list.

### unit remove

```
resonance unit remove <ref>
```

Retires a code unit or test. Its links and annotations become orphaned.

### unit list

```
resonance unit list [path]
```

Lists code units and tests with their locations and states, optionally under one file or directory.

### unit split

```
resonance unit split <ref> --into <ref>…
```

Declares that a unit became several. Declare the new units first. The old unit's links and annotations are copied to each new unit, and the old unit is retired.

### unit merge

```
resonance unit merge <ref>… --into <ref>
```

Declares that several units became one. Their links and annotations are copied to the remaining unit, and they are retired.

---

## Links and annotations

### link

```
resonance link <source> <type> <target> [--confidence N]
```

Creates a link. `<type>` is `satisfies`, `verifies` or `constrains`. `--confidence` is between 0 and 1. See [Links and verification](links-and-verification.md).

### unlink

```
resonance unlink <link>
```

Removes a link. It's kept for history but no longer appears in queries.

### confirm

```
resonance confirm <link>
```

Returns a stale link to `asserted` after you've checked that it still holds, and records the current content of both ends as its new baseline.

### annotate

```
resonance annotate <ref> --kind note|warning|call-out|instruction --text TEXT [--task LABEL]
```

Attaches an annotation. `--task` is for call-outs only. See [Annotations](annotations.md).

### reattach

```
resonance reattach <link-or-annotation> <ref>
```

Moves an orphaned (or wrongly attached) link or annotation to a new target. A re-attached link starts again as `asserted`.

---

## Reading

### show

```
resonance show <ref>
```

One node or link in full, with its links and annotations.

### trace

```
resonance trace <requirement>
```

The code and tests behind a requirement, with link statuses, the latest test results and the decisions that constrain it.

### why

```
resonance why <unit-test-or-file>
```

The requirements, decisions, tests and annotations behind a piece of code, including those on the units and file that contain it.

### search

```
resonance search [TEXT] [--type TYPE] [--status STATUS] [--limit N]
```

Finds nodes by text (case-insensitive), type (`Requirement`, `Decision`, `CodeUnit`, `Test`, `Annotation`, `File`, `Evidence`) and status. A node status is `active`, `retired`, `attached`, `orphaned` or `removed`. A link status (`asserted`, `verified`, `stale`, `orphaned`) finds nodes with at least one link in that status. Returns at most 50 results unless `--limit` says otherwise.

### unverified

```
resonance unverified
```

Active requirements with no verified test.

### context

```
resonance context <requirement>… [--task LABEL]
```

A context pack for a task: the requirements, the decisions that constrain them or their code, code locations, tests with their latest results, annotations in scope, and the call-outs for `LABEL`. See [Queries](queries.md#context-everything-for-a-task).

---

## Verification

### test

```
resonance test <ref>
```

Runs tests and records the results as evidence. `<ref>` is a requirement or code unit (runs every test that verifies it), a test (runs that test), or a `verifies` link (runs its test). A pass makes the test's `verifies` links `verified`. Exits with status 1 if any run didn't pass.

---

## Agents and services

### agent install

```
resonance agent install [--dry-run] [--yes]
```

Registers the Resonance MCP server with Claude Code for this project, in Claude Code's local scope. See [Working with agents](agents.md).

### mcp

```
resonance mcp
```

Runs the MCP server over standard input and output. Agents start this; you don't run it by hand.

### daemon

```
resonance daemon [--foreground | --stop]
```

With no options, starts the repository's daemon if needed and shows its health. `--stop` stops it. `--foreground` runs it in this process until it's idle or stopped. See [The daemon](daemon.md).

### registry

```
resonance registry run
resonance registry install [--dry-run] [--yes]
resonance registry uninstall [--dry-run] [--yes]
```

Runs the optional registry of daemons, or installs or removes it as a user service (launchd on macOS, `systemd --user` on Linux). `install` and `uninstall` show what they'll do and ask first. See [The registry](daemon.md#the-registry-optional).
