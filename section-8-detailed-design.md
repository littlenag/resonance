## 8. Detailed design

### 8.1 Source-of-truth semantics

"Graph as source of truth" means the graph store is authoritative for everything, including file content, while the working tree is a replica that tools can use directly.

- **FR-010** The graph MUST hold the exact content of every tracked file, stored as content-addressed blobs. File content in the graph is authoritative for bytes. [Proposed]
- **FR-011** Structure derived from file content (code units, references) MUST be recomputable from that content and MUST NOT be the only record of anything a person or agent authored. [Proposed]
- **FR-012a** A file change on disk MUST update the graph's copy of that file whether or not the file parses. [Proposed]
- **FR-012b** File changes occurring when the demon is not running will result in a "conflict" that must be surfaced to the user. The conflicting file must be replaced with content from the graph, with the file on disk renamed, and a message to the user asking for directions to merge, discard, or take other action.
- **FR-013** When a file does not parse, its derived structure MUST be marked stale, and links and annotations on it MUST be kept. This should be an exception circumstance as there is no particular parsing of user files that is required. Unparseable git files must result in a message to the user. The `doctor` command should revert unparseable core files (like git configs, etc) to their known good graph counterparts. [Proposed]
- **FR-014** The working tree MUST be a standard software project for the project's language and build system. [Decided]
- **FR-015** All graph-only information (intent, links, annotations, evidence, identity history) MUST be reachable only through the API, never as extra files mixed into the source tree. Projected intent documents (§8.10) are the one deliberate exception, and are opt-in. This is solely because tooling has yet to be built that would move these intent documents entirely into the graph. Nevertheless, real projects will have a split of graph-native and file-native intent documents. [Decided, with the exception Proposed]

Rationale: storing file text in the graph and treating it as authoritative for bytes avoids the lossless round-trip problem of printing files from syntax trees (comments, whitespace, formatting). If a future version makes syntax trees primary, it should use a lossless tree that keeps trivia (as Roslyn and rust-analyzer's `rowan` do), so that rendering a file is concatenation. (No programming language in wide-spread usage is yet graph-native; resonance is a key step in enabling that future.) [Proposed]

### 8.2 Storage

- **FR-020** The graph store MUST be a structured binary store kept in the repository alongside regular files. SQLite is the assumed starting choice. [Decided: structured blob in the repo; SQLite named as the likely form]
- **FR-021** The store MUST live under `.resonance/` at the repository root. [Decided]
- **FR-022** The store MUST be crash-safe: every applied change is a transaction. [Decided]
- **FR-023** The store MUST separate authored, identity and derived data so that derived data can be dropped and rebuilt without loss. [Decided]
- **FR-024** Derived data SHOULD be cached keyed by the content hash of its inputs, so that switching back to a previously seen tree state is fast. [Proposed]
- **FR-025** Local-only runtime files (endpoint file, lock file, sockets, caches) MUST be excluded from git, and `resonance init` MUST add them to `.gitignore`. [Decided]
- **FR-026** Changing `graph.db` on each commit will wreak havoc with git. Therefore, we must eventually allow a linear list of "delta" patch databases to be generated that patch the base `graph.db` into the current state.  This would allow git operations to push just the delta db. A compaction step would occur every N (configurable) deltas. [Proposed]

Illustrative layout [Proposed]:

```
.resonance/
  graph.db          # SQLite: authored + identity + derived tables (see §8.6 for what is committed)
  ops/              # committed operation log (if option A in §8.6 is adopted)
  cache/            # derived-data cache keyed by tree hash (gitignored)
  daemon.lock       # single-instance lock (gitignored)
  daemon.json       # endpoint, pid, versions (gitignored)
  config.toml       # project configuration (committed)
```

Illustrative core tables [Proposed]:

| Table | Columns (sketch) |
| --- | --- |
| `nodes` | `id`, `type`, `props` (JSON), `layer`, `created_op`, `retired_op` |
| `edges` | `id`, `type`, `src`, `dst`, `props`, `status`, `provenance`, `confidence`, `created_op`, `removed_op` |
| `blobs` | `hash`, `bytes`, `size` |
| `files` | `path`, `blob`, `generation`, `last_ingested_at`, `parse_state` |
| `spans` | `code_unit`, `file`, `start_byte`, `end_byte` |
| `ops` | `op_id`, `kind`, `payload`, `actor`, `hlc`, `change_set`, `commit` |
| `identity` | `id`, `lineage` (previous IDs), `op` |
| `meta` | `schema_version`, `repo_root`, `last_reconciled_commit`, `ingest_watermark` |

SQLite with recursive common table expressions is expected to handle the traversal depths Resonance needs (one to a few hops). Whether an embedded graph engine is warranted later is [Open] (OQ-03).

### 8.3 Identity and reconciliation

This component determines whether annotations and links survive real editing. It SHOULD be built and tested first, against real editing histories. [Proposed]

- **FR-030** Authored nodes (requirements, decisions, annotations, and so on) MUST receive a permanent, globally unique ID when created (for example UUIDv7 or ULID). [Proposed]
- **FR-031** Every clone of a repository MUST agree on the IDs of nodes that authored data refers to. Either derived IDs are deterministic (a function of language, path, kind and qualified name, plus a disambiguator), or the minting of an ID is itself a committed operation. [Proposed]
- **FR-032** After each successful parse, the daemon MUST match the new code units against the previous ones and carry IDs, links and annotations across. Matching order: same kind, qualified name and parent; then content similarity; then tree-diff matching (GumTree-style). [Proposed]
- **FR-033** When the daemon decides that a code unit was renamed, moved, split or merged, and authored data depends on it, it MUST record that decision as an identity operation (§9.4), so that other clones apply the same judgement rather than recomputing it differently. [Proposed]
- **FR-034** When matching fails, affected links and annotations MUST become `orphaned` with a record of the last known location. They MUST NOT be deleted. [Proposed]
- **FR-035** File identity MUST be path-based. An editor that saves by writing a temporary file and renaming it over the original MUST be treated as a new version of the same file node. [Proposed]
- **FR-036** Real file moves MUST be detected by content similarity, as git does, and recorded as identity operations. [Proposed]
- **FR-037** People and agents MUST be able to correct a reconciliation decision (for example, re-attach an orphaned annotation) through the API. [Proposed]

### 8.4 Watcher and ingest

- **FR-040** Each repository MUST have a persistent watcher process for its working tree. [Decided]
- **FR-041** The watcher MUST use native file-event APIs: FSEvents on macOS and inotify (or fanotify) on Linux. Windows support is [Open] (OQ-07). [Proposed]
- **FR-042** The watcher MUST tolerate batched, coalesced, delayed and out-of-order events. FSEvents reports at directory granularity and merges changes, so the watcher MUST re-examine affected paths (hash comparison) rather than trust event contents. [Proposed]
- **FR-043** The daemon MUST maintain an **ingest watermark**: the point up to which disk changes have been applied to the graph. [Proposed]
- **FR-044** The daemon MUST suppress events caused by its own writes, by recording the path, content hash and generation of every file it writes and ignoring a matching event. [Proposed]
- **FR-045** The watcher MUST ignore, by default: `.git/` internals (handled by the git monitor, §8.6), `.resonance/` runtime files, and editor scratch files (for example `*.swp`, `*~`, `.#*`, vim's `4913` probe file). [Proposed]
- **FR-046** Generated and build-output directories MUST NOT be ingested as graph content. Defaults include `target/`, `.bsp/`, `.bloop/`, `.metals/`, `build/`, `dist/`, `out/`, `.gradle/`, `node_modules/`, `.venv/`, `__pycache__/`. The watcher SHOULD also respect `.gitignore`. Mache's experience (a watcher on `target/` leaking roughly 129,000 file descriptors) shows why this matters. [Proposed]
- **FR-047** On startup, the daemon MUST reconcile the graph against the disk before serving queries, using content hashes. It MAY use FSEvents' replay-since-event-ID as an optimization, but MUST fall back to a full hash scan because that history is advisory. [Proposed]
- **FR-048** A burst of changes that matches a git operation MUST be handed to the git monitor instead of being processed as individual edits (§8.6). [Proposed]
- **FR-049** Ingest of a changed file MUST be incremental: re-hash, update the blob, re-extract that file, reconcile identity, update link statuses. [Proposed]

### 8.5 Projection writer (graph → disk)

- **FR-050** When a graph mutation changes file content, the daemon MUST write the file with an atomic replace (write a temporary file in the same directory, then rename), preserving permissions and line endings. [Proposed]
- **FR-051** Writes MUST produce ordinary file-system events, so IDEs, build tools and test watchers notice them with no special integration. [Decided in substance: G5]
- **FR-052** Before writing, the daemon MUST check that the file on disk still matches the version the mutation was based on (by hash or generation). If it does not, the daemon MUST ingest the disk version first and then re-evaluate the mutation. [Proposed]
- **FR-053** Conflict policy: an edit made on disk by a person takes precedence. A graph mutation that no longer applies cleanly MUST be rejected with a conflict error or rebased by the caller, never silently overwrite the disk. [Proposed]
- **FR-054** Multi-file mutations SHOULD be written as a batch, to keep event bursts short and to avoid build tools seeing a half-applied change for long. [Proposed]

### 8.6 Git integration

Git integration was identified as the next significant problem. This section records the decided constraints, the proposed approach and the alternatives.

**Decided constraints**

- **FR-060** The graph store's internal representation MUST be optimized for the system's access patterns and need not be textual. [Decided]
- **FR-061** All changes MUST be expressible in an edit algebra of textual edits and node-centric edits that git can carry and merge (§9). [Decided]

**Proposed approach**

- **FR-062** The **derived layer MUST NOT be committed**. It is rebuilt from source files (or fetched from a cache keyed by tree hash). [Proposed]
- **FR-063** The **authored and identity layers MUST be versioned** in git. [Proposed]
- **FR-064** File content is versioned by git as ordinary files. Text edits MUST NOT be duplicated as committed operations, except where a text edit changes identity (FR-033). [Proposed]
- **FR-065** Authored and identity changes MUST merge without manual resolution in the common case, and genuine conflicts MUST become `Conflict` nodes (S11). [Proposed]
- **FR-066** Graph changes in a pull request SHOULD be reviewable on standard hosting platforms without custom tooling. [Proposed]

**How authored data is committed — options** [Open] (OQ-01)

| Option | Description | Strengths | Weaknesses |
| --- | --- | --- | --- |
| **A. Operation-log files** (recommended) | Each change set writes a new, uniquely named file of operations, such as `.resonance/ops/<ulid>.ops`. Files are only ever added, so git merges are a union and never conflict. The graph is a deterministic fold over all operations. | Works with every host and git's merge button; readable in PRs; the edit algebra becomes the unit of merging. | Needs periodic deterministic compaction; many small files. |
| **B. Custom git refs** | Operations stored as git objects under a separate ref namespace, as git-bug and Gerrit NoteDb do. | Keeps the working tree clean; native git storage. | Refs are not pushed or fetched by default; invisible to hosting UIs; must be tied to code commits explicitly. |
| **C. Commit the SQLite file** | The database file itself is committed, with a custom merge driver and a textconv diff driver in `.gitattributes`. | Simplest to start; matches "SQLite in the repo" literally. | Drivers must be configured per clone; server-side merges bypass them; PR diffs show a binary blob; repository growth. |

Recommended path: start with **C** only for single-developer bootstrapping if needed, and move to **A** before any shared use. SQLite remains the materialized store in every option. [Proposed]

**Git operation handling**

- **FR-067** The git monitor MUST watch `.git/HEAD`, refs, `index.lock`, `MERGE_HEAD`, and rebase state directories, and MUST suspend per-event ingest while a git operation is in progress. [Proposed]
- **FR-068** After a git operation completes, the daemon MUST rebuild state for the new commit: load or rebuild the derived layer for the new tree, fold the authored operations reachable from that commit, then apply uncommitted working-tree changes. The graph state is a function of **commit plus uncommitted changes**. [Proposed]
- **FR-069** Resonance SHOULD provide optional git hooks: `pre-commit` to flush pending operations into `.resonance/ops/` so they are committed with the code, and `post-checkout` / `post-merge` to notify the daemon promptly. Hooks MUST be optional; the git monitor alone must be sufficient. [Proposed]
- **FR-070** Each git worktree MUST be treated as a separate repository root with its own daemon. Whether worktrees share a derived-data cache is [Open] (OQ-08). [Proposed]

### 8.7 Process model: daemons, discovery and health

**Per-repository daemon**

- **FR-080** The watcher, ingest, projection writer, git monitor and query API for a repository MUST run under one owner (P1), because disk ingest and API mutations must be serialized against each other and self-write suppression requires knowing what the daemon wrote. [Proposed]
- **FR-081** Queries SHOULD be served by the same owner, so that they can honor read-your-writes (§8.8). Direct read-only access to the SQLite file MAY be offered for tools that accept stale answers. [Proposed]
- **FR-082** A daemon MUST take an exclusive lock on `.resonance/daemon.lock` at start. The lock, not the registry, guarantees one daemon per repository. [Proposed]
- **FR-083** A daemon MUST write `.resonance/daemon.json` with its endpoint, process ID, Resonance version, schema version and start time, so clients can find it without the registry. [Proposed]
- **FR-084** Clients MUST start a daemon on demand when none is running. Daemons SHOULD exit after a configurable idle period. [Proposed]
- **FR-085** The process layout MUST allow the per-repository component to run either as its own process or hosted alongside others in a shared process later, without design changes. (Relevant if the implementation runs on the JVM, where one process per repository costs memory and start-up time.) [Proposed]

**Transport**

- **FR-086** Daemons MUST listen on Unix domain sockets by default, placed in a short per-user runtime directory (for example `~/.resonance/run/<hash-of-root>.sock`) because macOS limits socket paths to about 104 characters. [Proposed]
- **FR-087** If a TCP listener is ever enabled, it MUST bind to loopback only and require a per-daemon token, because an unauthenticated localhost service that accepts mutations is exposed to browser-based attacks such as DNS rebinding. [Proposed]

**Registry**

- **FR-088** A system-wide registry daemon MUST provide only (a) service discovery ("which endpoint serves this directory?") and (b) health monitoring of per-repository daemons. [Decided]
- **FR-089** The registry MUST NOT sit between a client and a repository daemon on any request. [Proposed]
- **FR-090** Daemons MUST register with a lease and renew it by heartbeat; entries that miss renewal expire. [Proposed]
- **FR-091** Registry entries MUST record: canonical repository root (symlinks resolved; each worktree separate), process ID, endpoint, Resonance version, schema version and start time, so version skew after upgrades is detectable. [Proposed]
- **FR-092** After a restart, the registry MUST rebuild its view by scanning known endpoint files and checking which locks are held. [Proposed]
- **FR-093** The registry SHOULD run under the platform's user service manager (launchd on macOS, `systemd --user` on Linux). [Proposed]
- **FR-094** In the first version the registry monitors only; it does not restart daemons. Supervision is a later decision. [Proposed]

**Health**

- **FR-095** Daemons MUST distinguish **liveness** (process responding) from **readiness** (graph current enough to serve), and report: state (starting, reconciling, ready, git-operation, degraded), ingest lag, pending event count, last successful ingest, and last error. Mache's experience of a daemon that answered its handshake while its graph build was stuck shows why readiness must be separate. [Proposed]
- **FR-096** `resonance doctor` MUST show all known daemons with their health and flag version skew. [Proposed]

### 8.8 Query and mutation API

- **FR-100** All graph-only capabilities MUST be available through one API, used by the CLI, the MCP adapter and IDE plugins alike. [Decided in substance: G4]
- **FR-101** The API transport SHOULD be JSON-RPC over the daemon's Unix socket. gRPC is an alternative. [Open] (OQ-04)
- **FR-102** Reads MUST offer read-your-writes: a query that follows a file save or a mutation MUST reflect it. The daemon achieves this by waiting for the ingest watermark to pass the relevant change, or by re-hashing the files a query touches. A client MAY request `consistency: cached` to skip the wait. [Proposed]
- **FR-103** Mutations MUST be submitted as batches of edit-algebra operations (§9) and applied atomically. [Proposed]
- **FR-104** The API MUST support subscriptions to change streams (for IDE plugins and agents). [Proposed]

Operation groups [Proposed]:

| Group | Examples |
| --- | --- |
| **Navigation** | `getNode`, `neighbors(node, edgeTypes, depth)`, `trace(requirement)` (forward), `why(codeUnit)` (backward), `path(a, b)` |
| **Search** | By text, by type, by key (`FR-042`), by status, by author or provenance |
| **Intent authoring** | Create, edit, supersede and retire requirements, decisions, designs and constraints |
| **Linking** | Add, remove and re-target edges; confirm or reject agent-suggested links |
| **Annotation** | Annotate, call out a set of code units for a task, list annotations for a scope |
| **Verification** | Record evidence; run the tests linked to a requirement; list unverified, stale or orphaned links |
| **Impact** | What is affected by changing this requirement, decision or code unit |
| **Context** | Build a context pack for an agent task (§8.11) |
| **Code mutation** | Replace a code unit's text, create files, move files (through the projection writer, §8.5) |
| **Diagnostics** | Health, ingest lag, reconciliation failures, conflicts |

- **FR-105** A general query language is [Open] (OQ-05). Proposed default: a fixed set of operations like those above, plus a read-only SQL escape hatch over documented views. Cypher-like or Datalog-like languages can be added once real query needs are known.

### 8.9 Code-structure extraction (secondary layer)

Consistent with §2.5, this layer is deliberately minimal until Phase 0 says otherwise.

- **FR-110** Extractors MUST be pluggable per language. [Proposed]
- **FR-111** The minimum extracted structure is: files, modules or packages, types, functions and methods, and tests, with their spans. This is enough to anchor links and annotations. [Proposed]
- **FR-112** Reference edges (calls, uses, implements) are OPTIONAL and SHOULD come from compiler-grade semantic data where available (for example SemanticDB for Scala, or SCIP / language-server data), because questions such as "who calls this through an interface" or "what breaks if this signature changes" need name and type resolution, which syntax-only parsing cannot provide reliably. Tree-sitter MAY be used as a syntax-only fallback. [Proposed]
- **FR-113** Further investment in structural queries MUST be justified by Phase 0 results (§11). [Decided in substance]
- **FR-114** The first supported languages are [Open] (OQ-06): likely Scala, plus the main language of the codebase used for evaluation.

### 8.10 Intent layer authoring and import

- **FR-120** Requirements, decisions, designs and constraints MUST be creatable and editable through the API. [Decided in substance]
- **FR-121** Intent documents MAY be projected to disk as Markdown files with stable IDs (for example under `docs/intent/` or `specs/`), editable in any editor and re-ingested by the watcher like any other file. This gives intent-level developers a familiar editing surface and gives git reviewers readable diffs. It is opt-in, and it is the one exception to FR-015. [Proposed]
- **FR-122** Resonance SHOULD import existing specification formats during onboarding (S1): Markdown with requirement IDs (such as this document), GitHub Spec Kit output, Kiro `requirements.md` / `design.md` / `tasks.md`, EARS-style requirement statements, and ADR directories. [Proposed]
- **FR-123** How code-fluent developers annotate code is [Open] (OQ-10). Options: (a) graph-only annotations anchored to code units, leaving source untouched; (b) tagged source comments (for example `// @satisfies FR-042`) ingested as edges; (c) both, with (a) as the default. [Proposed default: c]

### 8.11 Agent integration

- **FR-130** An MCP adapter MUST expose the API's navigation, search, linking, annotation, verification, impact and context operations as agent tools. [Proposed]
- **FR-131** Agents MAY continue to edit files directly with their built-in tools; the watcher ingests those edits (P3). Agents MAY instead use API code mutations. Neither path is required. [Proposed]
- **FR-132** `resonance init` SHOULD add a short section to the project's agent instructions file (`CLAUDE.md` / `AGENTS.md`) explaining when to use the graph: intent, traces, annotations, impact and verification. [Proposed]
- **FR-133** A **context pack** for a task MUST include: the target requirements and their ancestors, related decisions and constraints, linked code units (as locations, not full text by default), linked tests, and annotations in scope, including call-outs made for this task. [Proposed]
- **FR-134** Links created by agents MUST carry agent provenance and a confidence value, and MUST start as `asserted`. [Proposed]
- **FR-135** Agents SHOULD be able to record evidence by running linked tests, turning `asserted` links into `verified` ones. [Proposed]

### 8.12 IDE integration (later phase)

- **FR-140** The system MUST work fully with no IDE plugin, because the working tree is ordinary files. [Decided in substance: G5]
- **FR-141** The first IDE plugin SHOULD target IntelliJ. [Proposed]
- **FR-142** The plugin SHOULD provide: gutter markers linking code to requirements and tests; navigation between requirement, code and test; an annotation and call-out UI; warnings for stale and orphaned links; and a view of requirements with no verified test. [Proposed]

### 8.13 Compatibility with build systems, IDEs and source control

- **NFR-001** The working tree MUST be indistinguishable to unaware tools from a project that does not use Resonance, apart from the `.resonance/` directory. [Decided]
- **NFR-002** Changes made through the API MUST appear to unaware tools as ordinary file writes (FR-051). [Decided in substance]
- **NFR-003** Resonance MUST NOT hold locks on source files and MUST NOT change file timestamps except by writing content. [Proposed]
- **NFR-004** Build-output and generated directories MUST NOT be ingested (FR-046). [Proposed]
- **NFR-005** Resonance MUST NOT require any configuration change to build tools, IDEs, test runners or CI. [Proposed]

### 8.14 Security and privacy

- **NFR-010** No network listener by default; Unix sockets with user-only permissions (FR-086, FR-087). [Proposed]
- **NFR-011** No telemetry by default. [Proposed]
- **NFR-012** Committed graph data (requirements, annotations, decisions) is visible to anyone with repository access. Resonance SHOULD make it easy to review what is committed, and MAY support a local-only annotation class that is never committed. [Proposed]
- **NFR-013** Content retrieved from the graph and passed to agents is data, not instructions; agent integrations SHOULD mark it as such. [Proposed]

### 8.15 Observability

- **NFR-020** Structured logs per daemon. [Proposed]
- **NFR-021** Metrics: ingest lag, event rate, self-write suppression hits, reconciliation matches and failures, orphan count, stale-link count, query latency. [Proposed]
- **NFR-022** `resonance doctor` summarizes health for one repository or all of them (FR-096). [Proposed]
