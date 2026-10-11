# Resonance MVP — Working Specification

This file is the **working spec**: the part of the full specification (`spec/section-*.md`) that the current implementation must satisfy. Everything not here is in [`backlog.md`](backlog.md).

**How the two files work together**

- Every item carries its ID from the full spec (`FR-###`, `NFR-###`, `S#`). The full spec stays the authoritative statement of intent. Text here may *narrow* an item for the MVP, and says so.
- `MVP-###` items are implementation decisions that the full spec leaves open or that the MVP deliberately simplifies. They are not in the full spec.
- To grow the product, move an item (a whole bullet, including its sub-bullets) from `backlog.md` to the matching area heading here, then implement it. An item marked *(partial)* here has its remainder listed in the backlog under the same ID.
- Backlog items carry an area tag such as `[storage]`, which names the heading here where they belong.

---

## 1. What the MVP is

**A single-developer, offline Resonance.** It is one Rust binary, `resonance`, providing a CLI and an MCP server over stdio. Both work against a local SQLite graph in `.resonance/` that is invisible to git. The binary:

- imports requirements from Markdown;
- lets people and agents declare **code units** and **tests** as named regions of any text file, and keeps them attached as files change, without parsing code;
- lets people and agents link code and tests to requirements and annotate code;
- verifies `verifies` links by running the linked tests;
- answers `trace`, `why` and context-pack queries.

**Why this cut.**

1. It tests the product thesis directly: intent structure reaching an agent (H2, §11.1).
2. It can be dogfooded on day one: Resonance imports its own spec, and its own code is linked to its own requirements.
3. It builds the seams that later features depend on: the store API boundary, an operation log, textual anchors, and stage and participation metadata.
5. It is language-agnostic from day one. Resonance parses no source code (NG4); meaning comes from the agent, and Resonance remembers it.
4. It leaves out the riskiest and most expensive parts: the persistent daemon and watcher, git integration (snapshots, deltas, merges), graph-to-disk writes, and multi-user adoption. Each of those can be added later without rework.

**Typical session**

```
resonance init                         # .resonance/ created, excluded via .git/info/exclude
resonance import spec/*.md             # FR/NFR items become Requirement nodes
resonance agent install                # registers the MCP server with Claude Code, local scope only
resonance unit add crates/resonance-store-sqlite/src/lib.rs:40-95 --name SqliteStore --kind type
resonance test add crates/resonance-core/src/ids.rs:120-131 --name ulid_sorts \
    --run "cargo test -p resonance-core -- --exact ids::tests::ulid_sorts"
resonance link SqliteStore satisfies FR-160
resonance link ulid_sorts verifies FR-030
resonance test FR-030                  # runs the linked tests, records Evidence, marks links verified
resonance trace FR-160                 # requirement -> code -> tests, with link statuses
resonance why SqliteStore
resonance annotate SqliteStore --kind instruction --text "do not change the open() signature"
resonance status                       # counts plus the repair list
```

Goals served: G1, G2, G3, G4, G8 (partial), G13. Principles honored: P2, P3, P4 (via MVP-002), P8.

---

## 2. MVP decisions

- **MVP-001 Language and layout.** Rust, as a Cargo workspace with three crates:
  - `crates/resonance-core`: the model, edit-algebra operations, the `GraphStore` trait, queries, ingest, the anchor tracker, intent import, and the MCP and CLI logic as a library.
  - `crates/resonance-store-sqlite`: the SQLite adapter, which is the only crate that depends on `rusqlite`.
  - `crates/resonance-cli`: the `resonance` binary.

  `resonance-core` MUST NOT depend on `resonance-store-sqlite` or `rusqlite` (FR-161). Rust is the project's implementation language (OQ-11, decided). `resonance-core` also depends on no source-code parser such as tree-sitter (FR-110).
- **MVP-002 No daemon: reconcile on demand.** There is no persistent watcher, daemon, registry or socket. Every CLI command, and every MCP tool call, first reconciles the graph with the disk (§3.3), then answers. This gives read-your-writes (FR-102) and "delay, never wrong answers" (P4) without a background process. It replaces FR-040–FR-044, FR-048 and FR-080–FR-096 for now. Concurrent CLI and MCP processes are serialized by SQLite (WAL mode, busy timeout of 5 s).
- **MVP-003 MCP transport.** `resonance mcp` speaks MCP over stdio, JSON-RPC 2.0 with newline-delimited messages. It handles `initialize` (returning `instructions`, MVP-014), `notifications/initialized`, `ping`, `tools/list` and `tools/call`. A minimal hand-rolled implementation is preferred over an SDK dependency unless a stable SDK crate is clearly simpler at implementation time. Resolves OQ-04 for the MVP.
- **MVP-004 One local graph.** The graph does not track branches; this is the Phase 1 fallback from OQ-18. Branch switches, pulls and rebases are simply reconciled like any other change to the file tree.
- **MVP-005 Orphaning is reversible.** A code unit that disappears is *retired*, not deleted, and its authored edges become `orphaned` with the last known location (FR-034). If the orphaned anchor's quoted text reappears exactly, with matching context (for example after switching back to a branch), the anchor is restored and its edges return to the status they had before. This makes MVP-004 tolerable.
- **MVP-006 Dependencies** (versions pinned at implementation time): `rusqlite` (bundled), `similar` (text diffs for anchor tracking), `ignore`, `serde`, `serde_json`, `toml`, `clap`, `ulid`, `sha2`, `globset`, `anyhow`/`thiserror`. Git is used through the `git` executable (`rev-parse`, `--git-path`); there is no libgit2.
- **MVP-007 Inputs.** Intent comes from Markdown in this spec's conventions (MVP-011). Code units and tests are declared by people and agents on any text file, in any language (MVP-013). Resonance parses no source code (NG4, FR-110). Every non-ignored file is a `File` node, and links and annotations may also target whole files.
- **MVP-008 Hashing and IDs.** Content hashes are SHA-256 in lowercase hex. Authored node and edge IDs are ULIDs (FR-030). A requirement's node ID is deterministic, `req:<KEY>` (for example `req:FR-042`), so re-import and future clones agree (FR-031, FR-186). Code units and tests are authored nodes with ULIDs; their addresses are defined in MVP-013.
- **MVP-009 Repository requirement.** Resonance runs inside a git repository and uses `git rev-parse --show-toplevel` to find the root. It refuses to run elsewhere, with a clear message (G9).
- **MVP-010 Operation log.** Every authored mutation is applied as a batch of edit-algebra operations (§9.1 envelope: `opId`, `kind`, `actor`, `hlc`, `changeSet`, `schema`, `payload`). The batch is recorded in an `ops` table in the same transaction as the state change. `hlc` is wall-clock milliseconds plus a counter. The MVP materializes state directly, but the log makes a later deterministic fold (FR-150) and delta export (FR-026) possible without migrating data.
- **MVP-011 Intent import format.** Requirements are recognized in Markdown as:
  - list items `- **KEY** text`;
  - table rows `| **KEY** | text | … |`;

  where `KEY` matches `(FR|NFR)-\d+[a-z]?`. A trailing status label (`[Decided]`, `[Proposed]`, `[Open]`, including qualified forms) is recorded as `specStatus`. The source path and line are recorded. Paths imported with `resonance import` are added to `config.toml` (`intent.sources`), and changed sources are re-imported automatically during reconcile. When a requirement's text changes, its node is updated and every edge to it becomes `stale` (S7). When a key disappears from all sources, the requirement is marked `retired`; it is never deleted.
- **MVP-012 Staleness rules.** Each edge stores the content hashes of both endpoints at the time it was created, confirmed or last verified. During reconcile:
  - A `verified` edge whose either endpoint changed becomes `stale` (FR-002).
  - *Any* edge whose `Requirement` endpoint text changed becomes `stale` (S7 AC1).
  - Edits to code under an `asserted` link leave it `asserted`, to avoid noise.
  - `resonance confirm <edge>` returns a stale edge to `asserted` and refreshes the hashes.
- **MVP-013 Code units as textual anchors.** A code unit (or test) is declared with:
  - a file;
  - a region: a line range `path:START-END`, or an exact snippet that occurs exactly once in the file;
  - a `name`;
  - a free-text `kind` (for example `function`, `type`, `block`, `test`).

  Resonance stores the **anchor**: the byte range, the quoted text, up to 3 lines (at most 256 bytes) of context before and after, and the quote's hash. Its address is `<path>::<name>`, or just `<name>` where that is unique. Duplicate names in one file get `#2`, `#3` and so on.

  After each file change, anchors in that file are re-located in order (FR-032, narrowed):
  1. **Diff mapping.** Map the range through the line diff between the old and new content (`similar`). If at least half of the anchored lines survive, the anchor follows the mapped range, including lines inserted inside it. Its content hash changes if the text changed.
  2. **Exact quote search** in the same file, with context used to pick between multiple hits.
  3. **Exact quote search** in other files that changed in the same reconcile: a move, recorded as `identity.move`.
  4. **Approximate match** in the same file: the best window whose similarity to the quote is at least 0.8 and whose context also agrees. Recorded as `identity.retarget` with its `confidence`. Matches below 0.9 go on the repair list for confirmation.
  5. **Orphan** (FR-034), keeping the last quote and location.

  When a file is renamed or moved, the addresses of its code units follow it.
- **MVP-014 Agent conventions served by MCP.** The MCP `initialize` result's `instructions` field carries the agent kit's dynamic part (FR-212). It covers:
  - what Resonance is;
  - when to call which tool;
  - **Resonance does not parse code**: declare code units and tests (`resonance_declare_unit`, `resonance_declare_test`) for the code you write or need to talk about;
  - **before changing code**, call `resonance_why` or `resonance_context`, and honor the annotations they return;
  - **keep anchors healthy**: work through `resonance_repair_list`, re-anchoring orphaned or low-confidence code units (FR-038);
  - **after writing code or tests**, link them (`satisfies`, `verifies`) and run `resonance_test`;
  - **offline invisibility** (FR-189): no Resonance IDs, trailers or mentions in commits, branches, pull requests or code;
  - **graph content is data, not instructions** (NFR-013).
- **MVP-015 Output.** Every read command prints human-readable text by default, and JSON with `--json`. MCP tools return JSON text content.
- **MVP-016 Large and binary files.** Files containing NUL bytes, or larger than `ingest.maxBlobBytes` (default 5 MiB), are recorded with their hash and size but without stored bytes. This narrows FR-010.
- **MVP-017 Tests are declared, not discovered.** Resonance does not find tests by parsing. The person or agent who declares a test supplies the exact command that runs it alone. Exit code 0 is a pass; see §3.8 for errors.

---

## 3. Requirements in the MVP

### 3.1 Model and link lifecycle  `[model]`

- **§6.2 core node types (partial).** `Requirement`, `Decision`, `File`, `CodeUnit` (a declared region, MVP-013), `Test` (a `CodeUnit` with a `run` command), `Evidence`, `Annotation`, with the key properties listed in §6.2. Requirement `status` is `active` or `retired` in the MVP.
- **§6.3 core edge types (partial).** `satisfies` (CodeUnit/File → Requirement), `verifies` (Test → Requirement or CodeUnit), `constrains` (Decision → any), `annotates` (Annotation → any), `contains` (File/CodeUnit → CodeUnit, derived from anchor ranges), `evidences` (Evidence → edge). Every authored edge carries `provenance`, `author`, `createdAt`, `confidence` (for agents), `status`, and endpoint hashes (MVP-012). Display uses reverse names ("satisfied by", "verified by", and so on), and visualizations omit reverse edges.
- **FR-001** Every edge has exactly one status: `asserted`, `verified`, `stale` or `orphaned`.
- **FR-002** A change to either endpoint of a `verified` edge moves it to `stale` (with the MVP-012 extension for requirement text).
- **FR-003** An edge whose endpoint is removed without an identity operation becomes `orphaned`, not deleted (MVP-005 makes this reversible).
- **FR-004** `verifies` edges are verified by recording test results as `Evidence` (§3.8).
- **FR-005** Queries can filter by link status; "requirements with no verified test" is a single query (`resonance unverified`).
- **FR-006** The schema version is stored in the graph (`meta.schema_version`), and migrations run at startup.

### 3.2 Source of truth (offline)  `[truth]`

- **FR-010 (partial)** The graph holds the exact content of every non-ignored file as content-addressed blobs (with the exceptions in MVP-016). In offline mode git and the file tree are authoritative, and the graph follows them (FR-188).
- **FR-011** Anchor positions and containment are recomputable from file content plus the recorded anchors, and are never the only record of anything authored.
- **FR-012a** A changed file updates the graph's copy whether or not it parses.
- **FR-013 (partial)** Resonance parses only intent sources and its own configuration, never source code. An intent source that cannot be read (unreadable, or invalid UTF-8) keeps its previously imported requirements and appears on the repair list.
- **FR-014** The working tree is untouched: Resonance never writes outside `.resonance/` and `.git/info/exclude`.
- **FR-015** All graph-only information is reachable only through the CLI and MCP, never as files in the source tree. Projected intent documents are not in the MVP.
- **FR-188** Offline: disk edits are ingested, never treated as conflicts, and Resonance never blocks any git operation. No git hooks are installed.

### 3.3 Ingest and reconcile  `[ingest]`

- **FR-047 (adapted by MVP-002)** Before serving any command or tool call, reconcile the graph against the disk. The fast path skips files whose `(size, mtime)` are unchanged; changed files are re-hashed, and only real content changes are processed. A full re-hash is available as `resonance reconcile --full`.
- **FR-049** Ingesting a changed file is incremental: re-hash, update the blob, re-locate the anchors in that file (MVP-013), update link statuses (MVP-012).
- **FR-045** Always ignore `.git/`, `.resonance/` and editor scratch files (`*.swp`, `*~`, `.#*`, `4913`).
- **FR-046** Respect `.gitignore` (and `.git/info/exclude` and global excludes, through the `ignore` crate), and also ignore the default build and output directories listed in FR-046. Extra patterns can be set in `config.toml` (`ingest.ignore`).
- **FR-035** File identity is path-based. Atomic save-by-rename is just a new version of the same file.

### 3.4 Identity  `[identity]`

- **FR-030** Authored nodes, including code units and tests, get permanent ULIDs; requirements get deterministic IDs (MVP-008).
- **FR-032 (partial)** After each file change, re-locate every anchor in it per MVP-013, carrying IDs, links and annotations across.
- **FR-033 (partial)** Moves and approximate re-anchors are recorded as `identity.move` and `identity.retarget` operations, with their confidence.
- **FR-034** When matching fails, affected links and annotations become `orphaned` with the last known location (file, byte range, quoted text, address), and are never deleted.
- **FR-037** People and agents can re-attach orphaned links and annotations: `resonance reattach <edge|annotation> <new-target>` records an `identity.retarget` operation. `resonance unit edit <unit> <path:START-END>` re-anchors a code unit explicitly.
- **FR-038** Orphaned code units, and approximate re-anchors with confidence below 0.9, go on the repair list. The agent decides what they now correspond to: re-anchor, confirm or retire.

### 3.5 Storage and the store API  `[storage]`

- **FR-020 (partial)** The graph is a SQLite database behind the store API, at `.resonance/graph.db`. It is local only and never committed in the MVP.
- **FR-021** The store lives under `.resonance/` at the repository root.
- **FR-022** Every applied change set is one transaction.
- **FR-023** Authored, identity and derived data are separated (by table or by `layer` column), so derived data can be dropped and rebuilt with `resonance rebuild-derived`.
- **FR-025 (partial)** `resonance init` adds `/.resonance/` to `.git/info/exclude` (found via `git rev-parse --git-path info/exclude`) and modifies no tracked file.
- **FR-160** All persistence goes through a `GraphStore` trait defined in engine-neutral terms (nodes, edges, properties, blobs, operations, traversals). `SqliteStore` implements it.
- **FR-161** The SQLite adapter lives in its own crate (MVP-001). No SQL, SQLite types, row IDs or table names appear in the trait or reach callers.
- **FR-162 (partial)** The trait covers:
  - transactions (applying a change set atomically);
  - node, edge and property reads and writes;
  - bounded traversal (neighbors filtered by edge type, direction and status, to a depth);
  - blob put and get;
  - file records;
  - metadata (`schema_version`, `stage = 1`, `participation = offline`, last reconcile);
  - appending operations and iterating over them;
  - schema migration;
  - a simple integrity check.
- **FR-163** `trace`, `why`, context packs and the repair list are implemented in `resonance-core` against the trait, never in the adapter.
- **FR-166 (partial)** `resonance export` writes the complete authored and identity data as edit-algebra operations in JSON Lines (engine-neutral, FR-164 format).
- **FR-167 (partial)** A store conformance test suite lives in `resonance-core` as generic tests over `GraphStore`, and `SqliteStore` runs it.
- **FR-168** No engine-specific escape hatch (raw SQL or direct file access) is exposed.

### 3.6 Intent  `[intent]`

- **FR-120 (partial)** Requirements and decisions can be created, edited and retired through the CLI (`resonance req add|edit|retire`, `resonance decision add|edit`) and MCP.
- **FR-122 (partial)** Import Markdown with requirement IDs in this spec's conventions (MVP-011), including the full Resonance spec.
- **FR-123 (partial)** Annotations are graph-only (option a), anchored to code units or files. Tagged source comments are not supported.
- **S4 annotations.** Annotation kinds are `note`, `warning`, `call-out` and `instruction`. A call-out can name a task label, so that a set of code units can be called out for one task (`resonance annotate … --kind call-out --task <label>`).

### 3.7 Code units and tests  `[units]`

- **FR-110** `resonance-core` parses no source code and depends on no language parser. Code units and tests are declared through the CLI (`resonance unit add|edit|remove|list`, `resonance test add`) and MCP (`resonance_declare_unit`, `resonance_declare_test`, `resonance_list_units`).
- **FR-111** The structure Resonance provides is files plus the declared code units and tests with their anchors (MVP-013).
- **FR-114** It is language-agnostic: any text file can carry code units.
- **Tests.** A test is a code unit with a `run` command: an exact shell command that runs that test alone (MVP-017). If `--run` is omitted, `test.command` from `config.toml` is used, with `{name}`, `{path}` and `{address}` substituted.

### 3.8 Verification  `[verification]`

- **FR-135 / FR-004** `resonance test <requirement|test|edge>` runs the `run` command of every linked test: the tests on `verifies` edges into a requirement, or a single test. Each run records an `Evidence` node (`kind = test`, `result` = `pass`/`fail`/`error`, `commit` = `git rev-parse HEAD`, `timestamp`, `toolVersion`, plus the exit code and the last 4 KiB of output), linked by `evidences` to each `verifies` edge it supports.
  - **Pass** (exit code 0) marks those edges `verified` and refreshes their hashes.
  - **Fail** (non-zero exit code) leaves them `asserted` with the failing evidence attached.
  - **Error** is recorded if the command cannot be started, or if its output matches `test.zeroTestsPattern` (default `running 0 tests`, which catches a `cargo test` filter that matched nothing). An error is never a pass.

### 3.9 Queries, CLI and MCP  `[query]`

- **FR-100** One library API in `resonance-core` serves both the CLI and MCP.
- **FR-102 (adapted)** Read-your-writes, through reconcile-before-read (MVP-002).
- **FR-103** Mutations are batches of edit-algebra operations, applied atomically (MVP-010).
- **FR-105 (partial)** A fixed set of operations; no query language and no SQL escape hatch.
- **Node addressing.** Commands accept a ULID; a requirement key (`FR-042`); a code-unit address (`path::name`) or a bare unit name where it is unique; or a file path. Ambiguous references list the candidates.
- **CLI commands:**
  - setup and lifecycle: `init`, `status`, `reconcile [--full]`, `rebuild-derived`, `mcp`, `agent install`, `export [--out]`, `eject`;
  - intent: `import <paths…>`, `req add|edit|retire`, `decision add|edit`;
  - code units and tests: `unit add <path:START-END|path --match TEXT> --name --kind`, `unit edit`, `unit remove`, `unit list [path]`, `test add … --run CMD`;
  - linking: `link <src> <type> <dst> [--confidence]`, `unlink <edge>`, `confirm <edge>`;
  - annotation: `annotate <target> --kind --text [--task]`, `reattach <edge|annotation> <target>`;
  - reading: `show <ref>`, `trace <req>`, `why <unit>`, `search <text> [--type] [--status]`, `unverified`, `context <req…> [--task]`;
  - verification: `test <ref>`.
- **MCP tools** (FR-130, partial): `resonance_status`, `resonance_search`, `resonance_show`, `resonance_trace`, `resonance_why`, `resonance_context`, `resonance_unverified`, `resonance_repair_list`, `resonance_create_requirement`, `resonance_create_decision`, `resonance_declare_unit`, `resonance_declare_test`, `resonance_list_units`, `resonance_edit_unit`, `resonance_link`, `resonance_unlink`, `resonance_confirm`, `resonance_annotate`, `resonance_reattach`, `resonance_test`.
- **S2 `trace`.** One call returns the code units linked by `satisfies`, the tests linked by `verifies`, and each link's status and latest evidence.
- **S3 `why`.** One call returns the requirements a code unit satisfies, the decisions that constrain it, the tests that cover it, and the annotations on it (including annotations on its containing units).

### 3.10 Agent integration  `[agent]`

- **FR-131** Agents edit files with their own tools, and reconcile picks up the edits. There are no API code mutations in the MVP.
- **FR-132 (partial) / FR-210 (partial)** `resonance agent install` registers the MCP server for Claude Code at *local* scope (`claude mcp add --scope local resonance -- <abs-path-to-resonance> mcp`), writing nothing to tracked files. If `claude` is not found, it prints the equivalent instructions. `resonance init` offers to run it.
- **FR-211 (partial) / FR-212 (partial)** The kit's content is delivered as MCP server instructions (MVP-014). There is no static file and no harness hooks.
- **FR-133** A context pack for a task contains: the target requirements; the decisions that constrain them or their linked code; the linked code units as locations (path, line range, address), not as full text; the linked tests with their latest evidence; and the annotations in scope, including call-outs for the given `--task`.
- **FR-134** Links created through MCP carry `provenance = agent`, a `confidence` value (required) and the agent's identity (from MCP `clientInfo`), and start as `asserted`.
- **FR-189** Invisibility: the kit tells agents never to put Resonance IDs, trailers or mentions in commits, branches, pull requests or code, and the MVP itself writes nothing visible.
- **FR-219** Traceability duties (in the instructions): declare code units and tests, link them, run linked tests, honor annotations, and keep anchors healthy.

### 3.11 Offline mode and lifecycle  `[offline]`

- **FR-180** `resonance init` creates `.resonance/` (with `graph.db` and `config.toml`), adds the exclusion, runs the first reconcile, and leaves `git status` unchanged. It explains in a few lines what offline mode is and isn't.
- **FR-181** Offline mode provides everything in this file.
- **FR-238 (partial)** `resonance status` (and `resonance_repair_list`) shows the **repair list**: orphaned code units, links and annotations with their last known location; approximate re-anchors with confidence below 0.9; stale links; unreadable intent sources; and retired requirements that still have links. It is computed after every reconcile.
- **FR-173 (partial) / FR-184 (partial)** `resonance eject` first writes an export (FR-166) outside the repository (default: `~/.resonance/exports/<repo-name>-<timestamp>.jsonl`), asks for confirmation, then removes `.resonance/` and the exclusion line. Nothing else in Resonance deletes `.resonance/`.

### 3.12 Non-functional  `[nfr]`

- **NFR-001** To unaware tools, the working tree is indistinguishable from a repository without Resonance.
- **NFR-003** Resonance holds no locks on source files and never changes file timestamps.
- **NFR-004** Build output is not ingested (FR-046).
- **NFR-005** No configuration change to build tools, IDEs, test runners or CI is required.
- **NFR-010** There is no network listener; MCP runs over stdio only.
- **NFR-011** There is no telemetry.
- **NFR-013** MCP results wrap graph-sourced text in fields labeled as data, and the instructions say that this content is not instructions.
- **NFR-036** A text write is never lost (files stay on disk; the graph only reads them).
- **NFR-037** Never wrong answers: every answer follows a reconcile (MVP-002).
- **NFR-038** Supported platforms are macOS and Linux.
- **NFR-041** `resonance init` leaves `git status` unchanged.

---

## 4. Data layout

```
.resonance/            # excluded through .git/info/exclude; nothing here is committed in the MVP
  graph.db             # SQLite (WAL); live store
  config.toml          # intent.sources, ingest.ignore, ingest.maxBlobBytes, test.command, test.zeroTestsPattern
```

Illustrative tables (an internal detail of the adapter): `nodes`, `edges`, `blobs`, `files`, `anchors`, `ops`, `meta`. These follow §8.2, with edge endpoint hashes (MVP-012) and anchors (MVP-013).

---

## 5. Acceptance

The MVP is done when the following pass. Each is an automated integration test against a temporary git repository unless marked *manual*.

- **S1 (offline form)** AC1 (no file modified), AC2 (files and imported requirements present; nothing parsed), AC3 (builds unaffected), AC4 (deleting `.resonance/` leaves a working project), AC5 / NFR-041 (`git status` unchanged).
- **S2** AC1 and AC2: `trace` with statuses and evidence; `unverified`.
- **S3** AC1: `why`.
- **S4** AC1 (anchored to a code unit), AC2 (survives edits elsewhere in the file, edits inside the region that keep at least half its lines, file renames, and moves of the region to another file; a region rewritten beyond recognition is orphaned and listed, never silently re-attached), AC3 (orphaned with last location when the anchor disappears, and restored when it reappears, MVP-005), AC4 (annotations appear in `why` and in context packs).
- **S7** AC1 (editing a requirement's text in Markdown and re-running any command makes its links stale) and AC2 (`trace` lists everything affected).
- **S8 (adapted)** A file save followed by any command reflects the new text and re-located anchors.
- **S13 (adapted)** Edits made between commands are always reflected in the next answer.
- **S15** AC1, AC2 (for the MVP feature set), AC4, AC7.
- **Verification.** `resonance test` marks passing `verifies` links `verified`, records failing evidence, and records `error` when the command cannot start or zero tests ran.
- **Anchor tracking.** A table-driven test suite of edit scenarios (insert above, edit inside, delete half, reformat, move to another file, duplicate snippet, delete entirely, restore) checks every MVP-013 step and its recorded operations.
- **Store boundary.** The conformance suite passes for `SqliteStore`, and `cargo tree -p resonance-core` shows no `rusqlite` and no source-code parser.
- **MCP.** A scripted stdio session (initialize → tools/list → tools/call `resonance_trace`) succeeds.
- **Dogfood (manual).** In this repository: `resonance import spec/section-*.md spec/mvp.md` imports every FR and NFR item. The MVP's own main code units and tests are declared, and linked to the MVP requirements they satisfy and verify. `resonance unverified` then shows what remains. A Claude Code session answers "why does `SqliteStore` exist?" using the MCP tools.
