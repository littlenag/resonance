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
| `3` | `check` (or a git hook) found a consistency error. |

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
resonance init [--agent | --no-agent] [--offline]
```

Sets up Resonance in the current repository: creates `.resonance/` (`config.toml`, and the graph in `.resonance/local/`), adds `/.resonance/` to `.git/info/exclude`, and reads every file. It changes no tracked file. Running it again changes nothing.

In a repository whose graph is shared, `init` **joins** it instead: it rebuilds the graph from the committed snapshot and deltas, and opts you in. `--offline` tries Resonance privately over the shared graph instead. See [Sharing the graph](sharing.md).

In a terminal, it offers to install the [agent kit](agents.md). `--agent` installs it without asking; `--no-agent` doesn't offer.

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

### rebuild

```
resonance rebuild
```

Re-folds the graph from its snapshot and deltas, then brings it up to date with the disk. Use it after a `C6` hold (see [Recovery](recovery.md)).

### restore

```
resonance restore <file>
```

Restores authored data from an export written by `export`, `eject` or `leave --export`. Operations for things that already exist are skipped.

### backup

```
resonance backup [--list]
resonance backup --restore [--from DIR]
```

Backs up `.resonance/` to `~/.resonance/backups/`, lists backups, or restores a missing `.resonance/` from the newest backup (or from `DIR`).

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

Checks this repository (the graph's integrity and schema version, the git exclusion, and the agent kit) and the health of every Resonance daemon on this machine. See [The daemon](daemon.md#health-of-every-daemon-doctor).

---

## Sharing and git

See [Sharing the graph](sharing.md).

### share

```
resonance share [--dry-run] [--yes] [--keep-local <annotation>…]
```

Shares the graph with the team (stage 1 to stage 2): shows everything that would be committed, then writes the snapshot and the shared files under `.resonance/`, opts you in, and installs the git hooks and drivers. It commits nothing. `--keep-local` keeps annotations private.

### leave

```
resonance leave [--export FILE]
```

Stops being a Resonance user in this clone: removes the hook blocks, the agent kit and your opt-in. The open delta must be committed, or written to `FILE` first.

### unshare

```
resonance unshare [--yes] [--out FILE]
```

Takes the repository back to stage 1: exports the graph, leaves the removal of the committed `.resonance/` files for you to commit, and restores the local exclusion.

### compact

```
resonance compact [--commit] [--anywhere]
```

Folds the sealed deltas into a new snapshot. Only on the trunk, unless `--anywhere`. `--commit` commits the compaction on its own.

### check

```
resonance check --staged
resonance check --range <a>..<b>
```

Consistency checks, with findings `R001` to `R010`. Exits with 3 when any finding is an error. The `pre-commit` and `pre-push` hooks run these.

### review

```
resonance review <a>..<b> [--markdown]
```

The graph changes in a range, for pull request descriptions.

### show-delta

```
resonance show-delta <file>
```

A delta file for people. Git uses it to show deltas in diffs.

---

## Recovery

See [Recovery](recovery.md).

### recover

```
resonance recover
resonance recover undo <id>
resonance recover dismiss <id> [--note TEXT]
resonance recover attach <delta>
resonance recover drop <delta> [--yes]
resonance recover repair-snapshot
```

Lists divergences, the detached pool and recovery records; puts back what a resolution displaced; marks a divergence handled; attaches or drops a detached offline delta; or repairs a conflicted snapshot after a merge.

### value

```
resonance value
```

What Resonance did for you, locally, including the agent kit's effectiveness.

---

## Spec items and the change log

### tui

```
resonance tui
```

A terminal UI to list, view, edit, add and retire spec items, and to see the change log. See [The TUI and the change log](tui-and-change-log.md).

### changes

```
resonance changes [--since SEQ]
resonance changes show <seq>
resonance changes done <seq>… [--note TEXT]
```

The change log: people's changes to spec items that no agent has handled yet.

### spec list

```
resonance spec list [--type TYPE] [--status STATUS] [--text TEXT]
```

Requirements, decisions, designs and constraints, with their keys, statuses and titles.

### spec show

```
resonance spec show <ref>
```

One spec item, with every field.

---

## Requirements and decisions

### import

```
resonance import <paths…> [--format FORMAT]
```

Imports intent from documents and keeps it in sync from then on: Markdown requirement lists and tables, Spec Kit `spec.md` files, Kiro `requirements.md` and `design.md`, EARS statements, and ADR directories. `--format` (`markdown`, `speckit`, `kiro`, `kiro-design`, `ears` or `adr`) names the format when it can't be told from the path. See [Requirements and decisions](requirements-and-decisions.md).

### req add

```
resonance req add <KEY> --text TEXT [--kind functional|non-functional] [--priority P] [--status draft|active]
```

Creates a requirement. The kind defaults from the key: `NFR-` keys are non-functional. An existing key is refused.

### req edit

```
resonance req edit <ref> [--text TEXT] [--kind K] [--priority P] [--status S]
```

Changes a requirement created with `req add`. If its text changes, its links become stale. Imported requirements are edited in their documents.

### req supersede

```
resonance req supersede <ref> --by <ref>
```

Records that one requirement replaces another: a `supersedes` link, and the old one becomes `superseded`, so its links become stale.

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

Changes any field of a decision, including its status (for example `superseded` or `deprecated`).

### design, constraint

```
resonance design add [--key KEY] --title TITLE [--text TEXT] [--status draft|active]
resonance constraint add [--key KEY] [--title TITLE] --text TEXT [--scope SCOPE] [--status draft|active]
resonance design|constraint edit <ref> [--title T] [--text X] [--scope S] [--status S]
resonance design|constraint retire <ref>
resonance design|constraint supersede <ref> --by <ref>
```

Designs describe how something is built; constraints limit how (`--scope` says where one applies). Code `realizes` a design; a constraint `constrains` what it applies to.

### ext add

```
resonance ext add <namespace:Name> --name NAME [--text TEXT] [--prop KEY=VALUE]…
```

Creates a node of an extension type, such as `acme:Service`. Link to it with a namespaced relation, such as `acme:runsIn`.

### intent project

```
resonance intent project <dir> [--dry-run] [--yes]
```

Writes the graph-native requirements as Markdown under `<dir>`. The document then owns them: edits there flow back into the graph.

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

Creates a link. `<type>` is `satisfies`, `verifies`, `constrains`, `realizes`, `derivedFrom` or `supersedes`, or a namespaced extension relation. `--confidence` is between 0 and 1. See [Links and verification](links-and-verification.md).

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
resonance agent install [--dry-run] [--yes] [--shared] [--generic] [--no-mcp]
```

Installs the agent kit: the MCP server in Claude Code's local scope, a skill, a harness hook that checks the agent's own git commands, and an instructions section. `--shared` (stage 2) puts the section in the shared `CLAUDE.md` or `AGENTS.md`; `--generic` prints a section and the MCP command for other agents. See [Working with agents](agents.md).

### agent status, agent update

```
resonance agent status
resonance agent update
```

The installed kit, whether it's stale, and the orientation; or refresh it for this version, stage and participation.

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
