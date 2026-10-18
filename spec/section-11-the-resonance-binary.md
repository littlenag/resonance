## 11. The `resonance` binary

Everything Resonance does on a developer's machine is delivered by **one executable, `resonance`**. It is a small operations CLI, a per-repository daemon, an MCP server, the system-wide registry, the quiet participant's helper, the git plumbing that hooks and drivers call, and the CI checker. These are subcommands of the same program, built from one Cargo workspace (MVP-001), so there is one thing to install, version, pin and trust. Separate deliverables (the IntelliJ plugin, CI wrappers, graph adapters) come later and sit on top of this binary (§13). [Decided]

Resonance's primary user is the agent, which works through the MCP server; for developers Resonance runs in the background (G16). The binary's command surface reflects that: graph work is MCP tools, and people get the spec TUI plus a few commands to set up, run and repair Resonance (FR-259).

This section is the reference for the binary: what it is responsible for, how it runs, and its complete command and tool surface. Commands are specified in detail where their requirements live; this section collects them and records the conventions they share. The **Available** column says whether a command is in the MVP (`spec/mvp.md`) or which backlog increment (`spec/backlog.md`) adds it.

### 11.1 Responsibilities

| Responsibility | What the binary does | Spec |
| --- | --- | --- |
| **Graph owner** | Holds the repository's graph through the store API; applies every change as an edit-algebra change set; runs schema migrations. | §8.2, §8.2.1, §9 |
| **Ingest and anchor tracking** | Keeps the graph in step with the file tree; re-locates code-unit anchors after each change. | §8.3, §8.4 |
| **Intent authoring** | Validates and applies the spec items agents bring in from documents (batch, with a dry run), and links them back to their source regions. It parses no documents. | §8.10 |
| **Query and mutation API** | Navigation, search, trace, why, impact, context packs, linking, annotation, verification. | §8.8 |
| **Verification** | Runs linked tests and records evidence. | §6.5, FR-135 |
| **MCP server** | Exposes the API and the agent kit's conventions to agents. | §8.11, §8.20 |
| **Agent kit installer** | Installs the agent kit as part of `init`, refreshes it when `init` runs again, and reports on it in `status` and `doctor`; carries its content. | §8.20 |
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
| **CLI** | A person (setup, operation, repair) or an agent's shell (`check`, `status`, `recover`) | One command | A client of the daemon, which it starts on demand (FR-084). No graph work: that is MCP (FR-259). |
| **TUI** (`resonance tui`) | A person | One session | The spec TUI (§8.22), a client of the daemon. |
| **MCP server** (`resonance mcp`) | The agent harness | One agent session | Speaks MCP over stdio; a client of the daemon. Carries all graph work. |
| **Daemon** (`resonance daemon run`) | The CLI, TUI or MCP server, on demand | Until idle (FR-084) | One per repository root (FR-082). People start, stop and restart it with `resonance daemon start\|stop\|restart` when needed. |
| **Registry** (`resonance registry run`) | launchd or `systemd --user` | Always on | Discovery and health only (FR-088). |
| **Quiet** (`resonance quiet …`) | The quiet bootstrap script, or a quiet participant's agent | One command | Restricted capabilities, enforced in code: no daemon, no projection, no compaction, no stage changes, writes only to `.resonance/deltas/` and its own working area (FR-234, FR-257). |
| **Plumbing** (`resonance hook …`, `resonance agent-hook …`, `resonance git-driver …`) | Git, or the agent harness | One invocation | Machine-facing entry points. Not intended for people to call. |

- **FR-252** When a daemon is available, CLI and MCP commands MUST be served by it (P1). If none is running, the client starts one and waits (P4). A command MUST never answer from a graph that has not been reconciled. In the MVP, which has no daemon, each command reconciles before answering (MVP-002). [Proposed]

### 11.3 Command reference

The **Available** column gives *MVP*, or a backlog increment (B8–B19), or both when the MVP provides a narrower form that a later increment completes.

- **FR-259** The CLI is an operations surface: setting up a repository, starting, stopping and restarting the daemon, status and health, consistency checks, and repair. Graph work (bringing in intent from documents, declaring code units and tests, linking, annotating, tracing, searching, verifying, reading and clearing the change log, reviewing) MUST be offered through the API, as MCP tools for agents and in the spec TUI for people, and MUST NOT be offered as CLI commands. A person who wants graph work done asks their agent. [Decided]

**Setup and lifecycle**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `init` | Mirror the repository into the graph, install the git hooks for the stage, and install the agent kit. Offline by default; in a shared repository, join the committed graph as a Resonance user. Running it again refreshes hooks and kit. Reads no requirements (S1). | FR-172, FR-180, FR-210 | MVP |
| `status` | Stage, participation, counts, unresolved divergences, the repair list, pending change-log entries, and the agent kit's state. | FR-171, FR-238 | MVP |
| `doctor` | Health of daemons and the registry, adoption health, integrity checks, version skew. | FR-096, FR-207 | MVP; B8 |
| `share` / `unshare` | Move between offline (stage 1) and shared (stage 2). | FR-185, FR-172 | MVP |
| `leave` | Stop being a Resonance user, without loss. | FR-172, FR-173 | MVP |
| `enforce on\|off` | Move between shared (stage 2) and enforced (stage 3). | FR-172 | B11 |
| `eject` | Export, then remove Resonance from the repository. | FR-173 | MVP |
| `quiet setup` | Opt in to quiet participation locally. | FR-232 | B9 |
| `version [--json]` | Binary version, supported schema and delta formats, MCP protocol version. | FR-256 | MVP |
| `tui` | Terminal UI over spec items: list, view, edit, add, retire; the change log. | FR-270–FR-279 | MVP |

**Daemon**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `daemon [status]` | The repository's daemon and its health. | §8.7 | MVP |
| `daemon start\|stop\|restart` | Start, stop or restart it. Clients also start it on demand. | FR-084 | MVP |

**Repair**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `recover` | List unresolved divergences, the detached pool and recovery records. | FR-196 | MVP |
| `recover undo\|dismiss\|attach\|drop\|repair-snapshot` | Undo an automatic resolution, mark a divergence handled, attach or drop a detached delta, repair a conflicted snapshot. | FR-192, FR-157 | MVP |
| `reconcile [--full]` | Bring the graph in step with the disk now. | FR-047 | MVP |
| `rebuild` | Rebuild authored state by folding the operation log. | FR-150 | MVP |
| `rebuild-derived` | Drop and rebuild derived data. | FR-023 | MVP |
| `export [--out]` / `restore <file>` | Engine-neutral export of authored and identity data, and restoring from one. | FR-166 | MVP |
| `backup` | Local backup outside the repository; restore when `.resonance/` has gone missing. | FR-184 | MVP |

**Git and CI**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `check --staged` | Pre-commit consistency check, with suggested fixes. | FR-215, FR-221 | MVP |
| `check --range <base>..HEAD` | Pre-PR and CI consistency check. | FR-176, FR-216 | MVP |

**Parallel graphs**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `graph load <name> <file>` | Load a parallel or disconnected graph from the import format. | FR-243 | B13 |
| `graph list` / `graph drop <name>` | Manage loaded graphs. | FR-243 | B13 |

**Services**

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `mcp` | MCP server over stdio, started by the agent harness. | FR-130, MVP-003 | MVP |
| `registry run\|install\|uninstall` | Run the registry, or install and remove its launchd or systemd user service. | FR-088–FR-094 | MVP |
| `uninstall --user` | Remove user-level installs: registry service, agent registrations, caches. | FR-266 | B18 |

**Plumbing** (called by git, agent harnesses and Resonance itself, not by people)

| Command | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `hook <git-hook-name> [args…]` | Entry point for every git hook Resonance installs; chained after existing hooks. | FR-069, FR-177 | MVP |
| `agent-hook <event>` | Entry point for agent-harness hooks that gate the agent's git commands. | FR-222 | MVP |
| `git-driver merge <base> <ours> <theirs>` | Merge driver for `graph.db`: mechanical union-and-refold. | FR-157 | MVP |
| `git-driver textconv <file>` | Readable rendering of a delta file, as git's textconv driver. | FR-066 | MVP |
| `daemon run` | The daemon itself, in the foreground. | §8.7 | MVP |

**Graph work: MCP tools** (agents; there are no CLI equivalents, FR-259)

| Tools | Purpose | Spec | Available |
| --- | --- | --- | --- |
| `resonance_conventions` | The conventions for this working tree, the kit's state and the orientation. | FR-212, FR-225 | MVP |
| `resonance_status`, `resonance_repair_list` | Counts, divergences and the repair list. | FR-171, FR-238 | MVP |
| `resonance_import_items` | Bring spec items in from a document, linked to their regions, with a dry run for the preview. | FR-122, FR-124 | MVP |
| `resonance_list_items`, `resonance_create_item`, `resonance_edit_item`, `resonance_retire_item`, `resonance_supersede` | Spec items of every type. | FR-120 | MVP |
| `resonance_create_requirement`, `resonance_create_decision`, `resonance_create_extension` | Shortcuts and extension types. | FR-120, §6.6 | MVP |
| `resonance_declare_unit`, `resonance_declare_test`, `resonance_edit_unit`, `resonance_remove_unit`, `resonance_split_unit`, `resonance_merge_unit`, `resonance_list_units` | Code units and tests (textual anchors). | FR-110, FR-033, MVP-013, MVP-017 | MVP |
| `resonance_link`, `resonance_unlink`, `resonance_confirm`, `resonance_reattach`, `resonance_annotate` | Links and annotations. | §6.3, FR-037, FR-134, S4 | MVP |
| `resonance_show`, `resonance_search`, `resonance_trace`, `resonance_why`, `resonance_unverified`, `resonance_context` | Reading. | §8.8, S2, S3, FR-005, FR-133 | MVP |
| `resonance_impact` | What a change to this node affects. | §8.8 | B12 |
| `resonance_test` | Run linked tests; record evidence. | FR-135 | MVP |
| `resonance_changes`, `resonance_changes_show`, `resonance_changes_done` | Read and clear the change log. | FR-274–FR-276 | MVP |
| `resonance_check`, `resonance_review` | Consistency checks; the graph changes in a range, for pull requests. | FR-221, FR-066 | MVP |
| `resonance_compact` | Fold sealed deltas into a new snapshot (on the trunk; only when the developer asks, FR-218). | FR-154–FR-156 | MVP |
| `resonance_recover`, `resonance_value` | Divergences and recovery; what Resonance did for the developer. | FR-196, FR-240 | MVP |

### 11.4 Conventions shared by all commands

- **FR-253** Every command MUST follow these conventions: [Proposed]
  - **Repository.** `-C <path>` runs as if started in that directory. The repository root comes from git (MVP-009).
  - **Output.** Human-readable by default; `--json` for machine-readable output with a stable schema (MVP-015). Messages follow §8.19.
  - **Safety.** Anything that changes tracked files, hooks, git configuration or user-level installs previews by default and needs `--yes` to proceed non-interactively (FR-172, FR-204). `--dry-run` is accepted everywhere a change could happen.
  - **Exit codes.** `0` success; `1` error; `2` usage error; `3` a check found problems (for `check` and hooks); `4` waiting on an *action required* decision (FR-201).
  - **Configuration precedence**, highest first: command-line flags; environment variables (`RESONANCE_*`); `.resonance/config.toml`; user configuration (`~/.config/resonance/config.toml`); built-in defaults.
- **FR-254** The command and tool surface is a public, versioned interface. CLI commands, MCP tool names and schemas, and JSON output schemas are versioned together with the binary (semantic versioning, FR-264). The MCP tools are the primary surface and are designed for agents; they are not a mirror of the CLI, which is a separate, small operations surface (FR-259). Where both offer the same operation (`check`, `status`, `recover`), they share its name and its JSON. Plumbing commands may change between minor versions, because only Resonance installs their callers. [Proposed]
- **FR-255** Everything the binary installs or writes from templates ships inside it: the agent kit content, git hook shims, `.resonance/README.md`, `.resonance/.gitattributes`, service definitions, schema migrations and the import-format schema. The binary never downloads anything at run time. The only download in the whole system is the quiet bootstrap fetching the binary itself (FR-231). [Proposed]
- **FR-256** `resonance version --json` MUST report the binary version, the store schema versions it can read and write, the delta and snapshot format versions, the parallel-graph import format version, and the MCP protocol version. `doctor`, the registry (version skew, FR-091) and quiet pinning (FR-231) rely on it. [Proposed]
- **FR-257** Quiet mode is the same binary with a restricted capability set, enforced in code rather than by instructions. Commands outside quiet mode's capabilities fail with an explanation. That keeps one build, one release and one checksum for everyone (FR-231). [Proposed]
- **FR-258** The binary MUST refuse to write a store whose schema is newer than it understands, and say which version is needed. It MAY read an older store and migrate it forward, after a backup, when it next writes (FR-006). [Proposed]
