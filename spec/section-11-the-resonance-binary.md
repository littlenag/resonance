## 11. The `resonance` binary

Everything Resonance does on a developer's machine is delivered by **one executable, `resonance`**. It is a CLI, a per-repository daemon, an MCP server, the system-wide registry, the quiet participant's helper, the git plumbing that hooks and drivers call, and the CI checker. These are subcommands of the same program, built from one Cargo workspace (MVP-001), so there is one thing to install, version, pin and trust. Separate deliverables (the IntelliJ plugin, CI wrappers, graph adapters) come later and sit on top of this binary (§13). [Decided]

This section is the reference for the binary: what it is responsible for, how it runs, and its complete command surface. Commands are specified in detail where their requirements live; this section collects them and records the conventions they share. The **Available** column says whether a command is in the MVP (`spec/mvp.md`) or which backlog increment (`spec/backlog.md`) adds it.

### 11.1 Responsibilities

| Responsibility | What the binary does | Spec |
| --- | --- | --- |
| **Graph owner** | Holds the repository's graph through the store API; applies every change as an edit-algebra change set; runs schema migrations. | §8.2, §8.2.1, §9 |
| **Ingest and anchor tracking** | Keeps the graph in step with the file tree; re-locates code-unit anchors after each change. | §8.3, §8.4 |
| **Intent import** | Turns requirement documents into `Requirement` nodes, and keeps them in sync with their sources. | §8.10 |
| **Query and mutation API** | Navigation, search, trace, why, impact, context packs, linking, annotation, verification. | §8.8 |
| **Verification** | Runs linked tests and records evidence. | §6.5, FR-135 |
| **MCP server** | Exposes the API and the agent kit's conventions to agents. | §8.11, §8.20 |
| **Agent kit installer** | Installs, updates and reports on the agent kit; carries its content. | §8.20 |
| **Daemon** | Watches the working tree, maintains the ingest watermark, serves the API over a socket. | §8.4, §8.7 |
| **Registry** | System-wide discovery and health for daemons. | §8.7 |
| **Git integration** | Git monitor; open and sealed deltas; compaction; hooks; merge and diff drivers. | §8.6, §8.6.1, §9.6 |
| **Sync and recovery** | Classifies divergence from git evidence; recovery journal; undo. | §8.18 |
| **Adoption lifecycle** | `init`, `share`, `leave`, `enforce`, `eject`; offline mode; repair lists. | §8.16, §8.17 |
| **Quiet mode** | Restricted bookkeeping for developers who have not opted in. | §8.21 |
| **CI checker** | Consistency checks and review summaries, runnable in any CI system. | FR-066, FR-176 |
| **Parallel graphs** | Loads, lists, drops and queries parallel and disconnected graphs, enforcing that they cannot modify the primary graph. | §6.7, §8.9 |
| **Projection writer** | Writes graph-originated changes to the working tree. | §8.5 |

### 11.2 Execution modes

- **FR-250** All of Resonance's local functionality MUST ship as the single executable `resonance`. That covers the CLI, daemon, MCP server, registry, quiet mode, git plumbing, CI checks and the agent kit. No second binary is required on a developer's or CI machine. [Decided]
- **FR-251** The binary runs in one of the modes below, chosen by subcommand. The modes share one implementation of every operation, so the same question gets the same answer whichever mode serves it. [Proposed]

| Mode | Started by | Lifetime | Notes |
| --- | --- | --- | --- |
| **CLI** | A person, a script or an agent's shell | One command | In the MVP each command reconciles and answers directly (MVP-002). Once the daemon exists, the CLI is a client of it and starts it on demand (FR-084). |
| **MCP server** (`resonance mcp`) | The agent harness | One agent session | Speaks MCP over stdio. Direct in the MVP; a client of the daemon later. |
| **Daemon** (`resonance daemon`) | The CLI or the MCP server, on demand | Until idle (FR-084) | One per repository root (FR-082). Never started by users directly, except for debugging. |
| **Registry** (`resonance registry run`) | launchd or `systemd --user` | Always on | Discovery and health only (FR-088). |
| **Quiet** (`resonance quiet …`) | The quiet bootstrap script, or a quiet participant's agent | One command | Restricted capabilities, enforced in code: no daemon, no projection, no compaction, no stage changes, writes only to `.resonance/deltas/` and its own working area (FR-234, FR-257). |
| **Plumbing** (`resonance hook …`, `resonance agent-hook …`, `resonance git-driver …`) | Git, or the agent harness | One invocation | Machine-facing entry points. Not intended for people to call. |

- **FR-252** When a daemon is available, CLI and MCP commands MUST be served by it (P1). If none is running, the client starts one and waits (P4). A command MUST never answer from a graph that has not been reconciled. In the MVP, which has no daemon, each command reconciles before answering (MVP-002). [Proposed]

### 11.3 Command reference

The **Available** column gives *MVP*, or a backlog increment (B8–B19), or both when the MVP provides a narrower form that a later increment completes.

**Setup and lifecycle**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `init` | Offline: create `.resonance/`, exclude it locally, run the first reconcile, and offer agent onboarding. In a shared repository: join the committed graph as a Resonance user. | FR-172, FR-180 | MVP |
| `status` | Counts, repair list, and later the stage, participation, unresolved divergences and governed paths. | FR-171, FR-238 | MVP |
| `doctor` | Health of daemons and the registry, adoption health, integrity checks, version skew. | FR-096, FR-207 | MVP; B8 |
| `reconcile [--full]` | Bring the graph in step with the disk now. | FR-047 | MVP |
| `rebuild-derived` | Drop and rebuild derived data. | FR-023 | MVP |
| `rebuild` | Rebuild authored state by folding the operation log. | FR-150 | MVP |
| `export [--out]` | Engine-neutral export of authored and identity data. | FR-166 | MVP |
| `restore <file>` | Restore authored and identity data from an engine-neutral export. | FR-166 | MVP |
| `backup` | Local backup outside the repository; restore when `.resonance/` has gone missing. | FR-184 | MVP |
| `value` | Local report of what Resonance did for the developer, and the upgrade path. | FR-240 | MVP |
| `share` / `unshare` | Move between offline (stage 1) and shared (stage 2). | FR-185, FR-172 | MVP |
| `leave` | Stop being a Resonance user, without loss. | FR-172, FR-173 | MVP |
| `enforce on\|off` | Move between shared (stage 2) and enforced (stage 3). | FR-172 | B11 |
| `eject` | Export, then remove Resonance from the repository. | FR-173 | MVP |
| `agent install` | Install the agent kit for the developer's harness (local scope in offline mode). | FR-132, FR-210 | MVP |
| `agent update` / `agent status` | Refresh the kit; report a stale or missing kit. | FR-214 | MVP |
| `quiet setup` | Opt in to quiet participation locally. | FR-232 | B9 |
| `version [--json]` | Binary version, supported schema and delta formats, MCP protocol version. | FR-256 | MVP |
| `tui` | Terminal UI over spec items: list, view, edit, add, retire; the change log. | FR-270–FR-279 | MVP |

**Intent**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `import <paths…>` | Import requirement documents and keep them in sync. | FR-122, MVP-011 | MVP |
| `req add\|edit\|retire` | Author requirements. | FR-120 | MVP |
| `decision add\|edit` | Author decisions. | FR-120 | MVP |
| `design …`, `constraint …`, `req supersede` | Further intent types and lifecycle. | §6.2, FR-120 | MVP |

**Code units and tests**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `unit add\|edit\|remove\|list` | Declare, re-anchor, remove and list code units (textual anchors). | FR-110, MVP-013 | MVP |
| `test add … --run CMD` | Declare a test and how to run it alone. | MVP-017 | MVP |
| `unit split\|merge` | Declare splits and merges. | FR-033 | MVP |

**Links and annotations**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `link <src> <type> <dst>` / `unlink` | Create and remove edges. | §6.3, FR-134 | MVP |
| `confirm <edge>` | Return a stale edge to `asserted`. | MVP-012 | MVP |
| `annotate <target> --kind --text [--task]` | Annotate or call out code. | S4, FR-123 | MVP |
| `reattach <edge\|annotation> <target>` | Re-anchor after orphaning. | FR-037 | MVP |

**Reading**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `show <ref>` | One node with its edges. | §8.8 | MVP |
| `trace <req>` | Requirement → code → tests, with statuses and evidence. | S2 | MVP |
| `why <unit>` | Code → requirements, decisions, tests, annotations. | S3 | MVP |
| `search <text> [--type] [--status]` | Search by text, type, key or status. | §8.8 | MVP |
| `unverified` | Requirements with no verified test. | FR-005 | MVP |
| `context <req…> [--task]` | Context pack for an agent task. | FR-133 | MVP |
| `impact <ref>` | What a change to this node affects. | §8.8 | B12 |

**Verification**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `test <req\|test\|edge>` | Run linked tests; record evidence. | FR-135 | MVP |

**Change log**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `changes [--since <seq>]` | List pending change-log entries, optionally after a remembered sequence number. | FR-274–FR-276 | MVP |
| `changes show <seq>` | The full before and after of one entry. | FR-275 | MVP |
| `changes done <seq>… [--note TEXT]` | Remove handled entries. | FR-276 | MVP |

**Recovery**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `recover` | List unresolved divergences and apply resolutions. | FR-196 | MVP |
| `recover undo <id>` | Undo an automatic resolution. | FR-192 | MVP |

**Git, review and CI**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `check --staged` | Pre-commit consistency check, with suggested fixes. | FR-215, FR-221 | MVP |
| `check --range <base>..HEAD` | Pre-PR and CI consistency check. | FR-176, FR-216 | MVP |
| `review <range>` | Readable summary of graph changes for a pull request. | FR-066 | MVP |
| `compact [--commit]` | Fold sealed deltas into a new snapshot (trunk only by default). | FR-154–FR-156 | MVP |
| `show-delta <file>` | Readable rendering of a delta file; also used as git's textconv driver. | FR-066 | MVP |

**Parallel graphs**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `graph load <name> <file>` | Load a parallel or disconnected graph from the import format. | FR-243 | B13 |
| `graph list` / `graph drop <name>` | Manage loaded graphs. | FR-243 | B13 |

**Services**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `mcp` | MCP server over stdio. | FR-130, MVP-003 | MVP |
| `daemon [--foreground]` | Per-repository daemon. | §8.7 | MVP |
| `registry run\|install\|uninstall` | Run the registry, or install and remove its launchd or systemd user service. | FR-088–FR-094 | MVP |
| `uninstall --user` | Remove user-level installs: registry service, agent registrations, caches. | FR-266 | B18 |

**Plumbing** (called by git and agent harnesses, not by people)

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `hook <git-hook-name> [args…]` | Entry point for every git hook Resonance installs; chained after existing hooks. | FR-069, FR-177 | MVP |
| `agent-hook <event>` | Entry point for agent-harness hooks that gate the agent's git commands. | FR-222 | MVP |
| `git-driver merge <base> <ours> <theirs>` | Merge driver for `graph.db`: mechanical union-and-refold. | FR-157 | MVP |

### 11.4 Conventions shared by all commands

- **FR-253** Every command MUST follow these conventions: [Proposed]
  - **Repository.** `-C <path>` runs as if started in that directory. The repository root comes from git (MVP-009).
  - **Output.** Human-readable by default; `--json` for machine-readable output with a stable schema (MVP-015). Messages follow §8.19.
  - **Safety.** Anything that changes tracked files, hooks, git configuration or user-level installs previews by default and needs `--yes` to proceed non-interactively (FR-172, FR-204). `--dry-run` is accepted everywhere a change could happen.
  - **Exit codes.** `0` success; `1` error; `2` usage error; `3` a check found problems (for `check` and hooks); `4` waiting on an *action required* decision (FR-201).
  - **Configuration precedence**, highest first: command-line flags; environment variables (`RESONANCE_*`); `.resonance/config.toml`; user configuration (`~/.config/resonance/config.toml`); built-in defaults.
- **FR-254** The command surface is a public, versioned interface. CLI commands, JSON output schemas and MCP tool names are versioned together with the binary (semantic versioning, FR-264). MCP tools mirror CLI commands (`resonance_<command>`) wherever an equivalent exists, so that agents and people use the same vocabulary. Plumbing commands may change between minor versions, because only Resonance installs their callers. [Proposed]
- **FR-255** Everything the binary installs or writes from templates ships inside it: the agent kit content, git hook shims, `.resonance/README.md`, `.resonance/.gitattributes`, service definitions, schema migrations and the import-format schema. The binary never downloads anything at run time. The only download in the whole system is the quiet bootstrap fetching the binary itself (FR-231). [Proposed]
- **FR-256** `resonance version --json` MUST report the binary version, the store schema versions it can read and write, the delta and snapshot format versions, the parallel-graph import format version, and the MCP protocol version. `doctor`, the registry (version skew, FR-091) and quiet pinning (FR-231) rely on it. [Proposed]
- **FR-257** Quiet mode is the same binary with a restricted capability set, enforced in code rather than by instructions. Commands outside quiet mode's capabilities fail with an explanation. That keeps one build, one release and one checksum for everyone (FR-231). [Proposed]
- **FR-258** The binary MUST refuse to write a store whose schema is newer than it understands, and say which version is needed. It MAY read an older store and migrate it forward, after a backup, when it next writes (FR-006). [Proposed]
