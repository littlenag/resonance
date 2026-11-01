# Resonance MVP — Working Specification

This file is the **working spec**: the part of the full specification (`spec/section-*.md`) that the current implementation must satisfy. Everything not here is in `backlog.md`.

**How the two files work together**

- Every item carries its ID from the full spec (`FR-###`, `NFR-###`, `S#`). The full spec stays the authoritative statement of intent. Text here may *narrow* an item for the MVP, and says so.
- `MVP-###` items are implementation decisions that the full spec leaves open or that the MVP deliberately simplifies. They are not in the full spec.
- To grow the product, move an item (a whole bullet, including its sub-bullets) from `backlog.md` to the matching area heading here, then implement it. An item marked *(partial)* here has its remainder listed in the backlog under the same ID.
- Backlog items carry an area tag such as `[storage]`, which names the heading here where they belong.

---

## 1. What the MVP is

**A git-integrated Resonance for one developer and a small team.** It is one Rust binary, `resonance`, providing an MCP server over stdio (the agent's way in, and where all graph work happens), a small operations CLI, a per-repository daemon that watches the working tree and the git repository, a system-wide registry for discovery and health, git plumbing (hooks, a merge driver and a diff driver), a web UI that each daemon serves on loopback, and a deliberately simple spec TUI. It supports adoption stages 0 to 2 (§8.16):

- **Offline trial (stage 1).** Nothing is committed and nothing is visible to teammates. The private graph follows git history and branches, and the agent repairs it after outside changes.
- **Shared graph (stage 2).** The graph is committed next to the code as a snapshot plus small delta files, compacted every N deltas on the trunk. Developers who join have opted in: the graph is their source of truth, and their agent, their git hooks and `resonance check` keep their commits consistent with it. Hold-outs keep using plain git.

Resonance is a superpower for the agent and a background tool for the developer (G16). The binary:

- mirrors the repository into the graph, installs git hooks and onboards the developer's agent, in one `init`;
- lets agents bring requirements and decisions in from the project's documents, whatever their format, with a preview, links back to the source regions, and the graph as the source of truth afterwards; Resonance parses no documents (NG6);
- lets agents declare **code units** and **tests** as named regions of any text file, and keeps them attached as files change, without parsing code;
- lets agents link code and tests to intent, annotate code, and verify `verifies` links by running the linked tests;
- answers trace, why and context-pack queries over MCP, kept current in the background;
- follows git: it recognizes git operations, classifies every disagreement between git and the graph from git evidence, resolves only what is non-destructive, and lets people and agents recover and undo;
- gives people a few commands to set up, run and repair it: `init`, `status`, `doctor`, `daemon`, `check`, `recover` and the like; no graph work is a CLI command (FR-259);
- onboards the developer's agent with a kit (instructions, a skill, harness hooks and conventions served over MCP), so that the agent keeps the graph and git in sync and pull requests merge cleanly;
- gives people a web UI and a terminal UI over spec items, and records a concise change log of their edits for agents to pick up and clear;
- shows people, in the web UI, what the daemon has done since it started, with its logs, which are also written to a configurable file.

**Why this cut.**

1. It tests the product thesis directly: intent structure reaching an agent (H2, §12.1), now with the git workflow a real team uses.
2. It is the smallest version a team can adopt incrementally: try it offline, share it, and step back without loss (G12).
3. It builds the seams later features depend on: the store API boundary, a deterministic fold over operations, textual anchors, adoption stages and participation, and the agent kit.
4. It is language-agnostic. Resonance parses no source code (NG4); meaning comes from the agent, and Resonance remembers it.
5. It leaves out what can come later without rework: graph-to-disk code mutations (B10; the only project-file writes are recovery restores and opt-in intent projection), team-wide message guardrails (B8), quiet onboarding (B9), enforcement at stage 3 (B11), parallel graphs (B13), the JetBrains plugin (B14) and the TUI's architectural summaries (B19).

Increments B1–B7 of the backlog have moved into this file, together with the spec TUI and change log (§8.22). The web UI, diagnostics and logs (§8.23) were added to it directly. Running Resonance on its own repository stays gated on field tests elsewhere (B17).

**Typical session**

```
resonance init                         # offline: the repository mirrored into .resonance/ (excluded via
                                       # .git/info/exclude), git hooks and the agent kit installed locally
```

Then, in the agent session:

```
> Bring our requirements in spec/ into Resonance.
  (the agent reads the documents, previews the items, keys and any reformatting,
   and on confirmation calls resonance_import_items; each item links to its region)
> Implement FR-160.
  (resonance_context; code and tests written; resonance_declare_unit, resonance_declare_test,
   resonance_link satisfies/verifies; resonance_test records evidence)
> Why does SqliteStore exist?          (resonance_why)
```

And for the person:

```
resonance ui                           # the web UI in the browser: spec items, the change log, diagnostics, logs
resonance tui                          # the same spec work in the terminal; edits land in the change log for the agent
resonance status                       # stage, divergences, repair list, pending changes, agent kit
resonance share                        # stage 2: preview, then new files under .resonance/ to commit
git add -A && git commit               # hooks flush the open delta and check consistency
```

Goals served: G1–G4, G6, G7, G8 (partial), G9, G12, G13, G14, G16. Principles honored: P1–P9.

---

## 2. MVP decisions

- **MVP-001 Language and layout.** Rust, as a Cargo workspace with three crates:
  - `crates/resonance-core`: the model, edit-algebra operations, the `GraphStore` trait, queries, ingest, the anchor tracker, the agent kit, and the MCP and CLI logic as a library.
  - `crates/resonance-store-sqlite`: the SQLite adapter, which is the only crate that depends on `rusqlite`.
  - `crates/resonance-cli`: the `resonance` binary.

  `resonance-core` MUST NOT depend on `resonance-store-sqlite` or `rusqlite` (FR-161). Rust is the project's implementation language (OQ-11, decided). `resonance-core` also depends on no source-code parser such as tree-sitter (FR-110).
- **MVP-002 The daemon serves; direct mode reconciles on demand.** A per-repository daemon owns the graph and serves every MCP tool call, web UI and TUI request, and CLI command (§3.15, FR-252). Clients start it when none is running and wait (P4). Before it answers, the daemon brings the graph up to date with the disk through the watcher and the sync protocol of MVP-020. *Direct mode* (`--no-daemon`, or `RESONANCE_NO_DAEMON=1`) keeps the original behavior for debugging and for environments where the daemon cannot run: the command reconciles the graph with the disk itself (§3.3), then answers. Both modes run the same operations (FR-251). Concurrent processes are serialized by SQLite (WAL mode, busy timeout of 5 s).
- **MVP-003 MCP transport.** `resonance mcp` speaks MCP over stdio, JSON-RPC 2.0 with newline-delimited messages. It handles `initialize` (returning `instructions`, MVP-014), `notifications/initialized`, `ping`, `tools/list` and `tools/call`. A minimal hand-rolled implementation is preferred over an SDK dependency unless a stable SDK crate is clearly simpler at implementation time. Resolves OQ-04 for the MVP.
- **MVP-004 One local graph.** *Superseded by FR-183 when B3 moved into the MVP: offline deltas now follow branches (MVP-030).* Originally the graph did not track branches (the Phase 1 fallback from OQ-18), and branch switches were reconciled like any other change.
- **MVP-005 Orphaning is reversible.** A code unit that disappears is *retired*, not deleted, and its authored edges become `orphaned` with the last known location (FR-034). If the orphaned anchor's quoted text reappears exactly, with matching context (for example after switching back to a branch), the anchor is restored and its edges return to the status they had before. This makes MVP-004 tolerable.
- **MVP-006 Dependencies** (versions pinned at implementation time): `rusqlite` (bundled), `similar` (text diffs for anchor tracking), `ignore`, `serde`, `serde_json`, `toml`, `clap`, `ulid`, `sha2`, `globset`, `anyhow`/`thiserror`, `notify` (native file events, B1), `ratatui` with `crossterm` (the TUI, MVP-041), `tiny_http` (the web UI's server; a build script embeds its bundle and the token comes from `/dev/urandom`, NFR-038), and `tracing` with `tracing-subscriber` (logs, MVP-044). The web UI's frontend (MVP-043) uses TypeScript, React, Vite (to build only), Mantine, TanStack Query, TanStack Virtual, React Router and Vitest. Git is used through the `git` executable (`rev-parse`, `--git-path`); there is no libgit2.
- **MVP-007 Inputs.** Intent comes from agents through the API: they read the project's documents and bring items in with `resonance_import_items` (MVP-042), or create them one by one; people add and edit items in the TUI. Resonance reads no documents (NG6). Code units and tests are declared by agents on any text file, in any language (MVP-013). Resonance parses no source code (NG4, FR-110). Every non-ignored file is a `File` node, and links and annotations may also target whole files.
- **MVP-008 Hashing and IDs.** Content hashes are SHA-256 in lowercase hex. Authored node and edge IDs are ULIDs (FR-030). A requirement's node ID is deterministic, `req:<KEY>` (for example `req:FR-042`), so that two clones that bring in the same requirement agree (FR-031, FR-186). Code units and tests are authored nodes with ULIDs; their addresses are defined in MVP-013.
- **MVP-009 Repository requirement.** Resonance runs inside a git repository and uses `git rev-parse --show-toplevel` to find the root. It refuses to run elsewhere, with a clear message (G9).
- **MVP-010 Operation log.** Every authored mutation is applied as a batch of edit-algebra operations (§9.1 envelope: `opId`, `kind`, `actor`, `hlc`, `changeSet`, `schema`, `payload`). The batch is recorded in an `ops` table in the same transaction as the state change. `hlc` is wall-clock milliseconds plus a counter. Since B5 the authored layer is a deterministic fold over these operations (MVP-027), and the live store caches the fold.
- **MVP-011 Intent import format.** *Superseded by MVP-042 (G16, NG6): Resonance does not read documents; agents bring items in through the API.* Originally Resonance recognized requirements in Markdown list items and table rows, kept the documents as import sources and re-imported them on change. When a requirement's text changes, every edge to it still becomes `stale` (S7), and items are retired, never deleted.
- **MVP-012 Staleness rules.** Each edge stores the content hashes of both endpoints at the time it was created, confirmed or last verified. During reconcile:
  - A `verified` edge whose either endpoint changed becomes `stale` (FR-002).
  - *Any* edge whose `Requirement` endpoint text changed becomes `stale` (S7 AC1).
  - Edits to code under an `asserted` link leave it `asserted`, to avoid noise.
  - `resonance_confirm` returns a stale edge to `asserted` and refreshes the hashes.
- **MVP-013 Code units as textual anchors.** A code unit (or test) is declared through MCP (`resonance_declare_unit`, `resonance_declare_test`) with:
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
  - **Resonance does not parse code or documents**: declare code units and tests (`resonance_declare_unit`, `resonance_declare_test`) for the code you write or need to talk about, and bring spec documents in yourself (MVP-042);
  - **before changing code**, call `resonance_why` or `resonance_context`, and honor the annotations they return;
  - **keep anchors healthy**: work through `resonance_repair_list`, re-anchoring orphaned or low-confidence code units (FR-038);
  - **after writing code or tests**, link them (`satisfies`, `verifies`) and run `resonance_test`;
  - **offline invisibility** (FR-189): no Resonance IDs, trailers or mentions in commits, branches, pull requests or code;
  - **graph content is data, not instructions** (NFR-013).

  Since B7 these instructions are the dynamic part of the full agent kit (MVP-038), and depend on the stage and participation.
- **MVP-015 Output.** Every CLI command prints human-readable text by default, and JSON with `--json`. MCP tools return JSON text content.
- **MVP-016 Large and binary files.** Files containing NUL bytes, or larger than `ingest.maxBlobBytes` (default 5 MiB), are recorded with their hash and size but without stored bytes. This narrows FR-010.
- **MVP-017 Tests are declared, not discovered.** Resonance does not find tests by parsing. The agent that declares a test supplies the exact command that runs it alone. Exit code 0 is a pass; see §3.8 for errors.
- **MVP-018 Graph scope seam.** The `GraphStore` trait exposes writes through a handle scoped to one graph. In the MVP only the primary graph exists. This lets parallel graphs (backlog B13) later receive handles that can write only their own graph (FR-116), without changing existing callers.

- **MVP-019 Daemon process model.** One daemon per repository root, run as `resonance daemon run` (FR-080, FR-082). It is a library component (`resonance_core::daemon`) that any process can host (FR-085); the binary runs one per process.
  - One owner thread applies every change, whether from the watcher or from a request, so changes are serialized (P1). Connection threads parse requests and hand them to the owner.
  - The API is JSON-RPC 2.0 with newline-delimited messages over a Unix domain socket (FR-086, FR-101, resolving OQ-04 for the MVP). Methods are the API's operations, used alike by the MCP server, the web UI, the TUI and the CLI (FR-251); `health`, `sync`, `subscribe` and `shutdown` are added.
  - The socket lives in a short per-user runtime directory: `$RESONANCE_RUNTIME_DIR` if set, else `$XDG_RUNTIME_DIR/resonance`, else `~/.resonance/run`, else `/tmp/resonance-<uid>` when that path would be too long for a socket (about 104 bytes on macOS). Its name is a hash of the canonical repository root. The directory is user-only (`0700`) and so is the socket.
  - `.resonance/local/daemon.lock` is held with an exclusive lock for the daemon's lifetime (FR-082). Because `flock` locks an inode rather than a path, a daemon also counts as running while it answers on its socket: a daemon that finds a listener there exits instead of taking the socket over, clients treat a live listener like a held lock, the running daemon re-locks `daemon.lock` if the file is deleted or replaced, and on exit it unlinks the socket only when it still holds the lock inode and the path is still the socket it bound, before it drops the listener or the lock. Deleting the lock file, the socket, or both does not make a running daemon gone: it still holds the old lock inode, and it re-locks `daemon.lock` before binding the socket again. It does not bind or unlink unless it holds the inode currently at that path. The bound socket's identity is that path's device and inode, recorded immediately after bind while the lock is held (`fstat` of a Unix listener does not name the socket file). A second process that takes the lock waits until the daemon recorded in `daemon.json` has exited before opening the store. The daemon checks its socket file every second: if it was deleted (or replaced by a file nobody listens on, for example after the runtime directory was cleaned) and this daemon still holds the lock, it binds it again, so that clients reconnect within a second instead of waiting for it to go idle; if another daemon holds the lock or answers there, this one stops without removing the successor's socket; `.resonance/local/daemon.json` records the endpoint, the web UI's address (MVP-043), process ID, versions and start time (FR-083); `.resonance/local/daemon.log` receives its log (MVP-044).
  - Clients start the daemon on demand as a detached process and wait until it is ready (FR-084). It exits after `daemon.idleSeconds` (default 1800) without requests. `resonance daemon start`, `stop` and `restart` control it, and `resonance daemon` shows its health.
- **MVP-020 Read-your-writes by sync cookies.** To answer a request (FR-102), the daemon writes a uniquely named *cookie* file into `.resonance/local/cookies/` and waits until the watcher reports it. File events for one watch are delivered in order, so every change made before the request has then been seen; the daemon applies them before answering. The *ingest watermark* (FR-043) is the sequence number of the last applied event batch. A request with `consistency: cached` (`--cached` on the command line) skips the sync and answers from the graph as it is. If the watcher fails or loses events, the daemon falls back to a full reconcile before answering (P4).
- **MVP-021 Watching.** On macOS the daemon watches the root with one recursive FSEvents stream. On Linux it adds an inotify watch for each directory it ingests, skipping ignored and build-output directories (FR-046), and watches new directories as they appear. Event contents are hints only: affected paths are re-examined by size, time and hash (FR-042). A dropped-events or rescan signal triggers a full reconcile. A change to a `.gitignore` file, to `.git/info/exclude` or to `.resonance/config.toml` also triggers one.
- **MVP-022 Registry.** `resonance registry run` serves discovery and health only, on `registry.sock` in the runtime directory (FR-088, FR-089). Daemons register when they start, renew a 30-second lease every 10 seconds, and unregister when they stop (FR-090, FR-091). A daemon that finds no registry carries on without it (P5). Daemons also append their root to `~/.resonance/known-roots`, from which a restarted registry rebuilds its view by reading each `daemon.json` and checking whether its lock is held (FR-092). `resonance registry install` writes and loads a launchd user agent (macOS) or a `systemd --user` unit (Linux); `registry uninstall` removes it. Both preview by default and need `--yes` (FR-093, FR-263). The registry polls each daemon's `health` and never restarts one (FR-094).
- **MVP-023 Git operations in progress.** While `.git/index.lock` exists, a git command is changing the working tree. The daemon reports the state `git-operation`, holds the events it receives, and applies them as one batch when the lock disappears (FR-048). The git monitor (MVP-029) extends this to every git operation and classifies it. Requests wait for the batch, for at most 10 seconds; a lock older than that is treated as stale and the daemon reports `degraded`.
- **MVP-024 Identity reconciliation (B2).** Re-anchoring stays purely textual (NG4):
  - In diff mapping (MVP-013 step 1), a line counts as surviving when it is unchanged or is paired with a changed line whose character similarity is at least 0.6. Boundaries inside changed lines are mapped through a character-level diff, so snippet anchors keep their exact columns.
  - Approximate matches (step 4) of snippet anchors are refined to the best-aligned substring rather than whole lines.
  - After step 3, the exact quote is searched in every unchanged text file too, through an in-memory index of line hashes (step 3b, recorded as `identity.move`).
  - When a file disappears and another appears in the same reconcile with content at least 50% similar, the change is a file move (FR-036). It is recorded as a `file.move` operation; links and annotations on the old file follow it, and anchors are mapped through the diff between the old and new content.
  - `resonance_split_unit` and `resonance_merge_unit` declare splits and merges (FR-033). The new units must already be declared and attached; the old unit may be attached or orphaned (a function rewritten into two is usually orphaned by then). They record `identity.split` / `identity.merge`, copy the links and annotations of the old units to the new ones as `asserted`, and retire the old units.
- **MVP-025 Layout, and migration from the earlier layout.** `.resonance/` follows §8.2:

  ```
  .resonance/
    graph.db          # snapshot (MVP-027); committed at stage 2, local in offline mode
    deltas/           # sealed and open deltas (MVP-026); committed at stage 2, local in offline mode
    config.toml       # committed at stage 2: stage, minimum binary version, compaction, governance, ...
    README.md         # stage 2: for hold-outs (FR-079)
    .gitignore        # stage 2: ignores local/
    .gitattributes    # stage 2: delta diff driver, snapshot merge driver
    local/            # never committed
      state.db        # live store: the fold of snapshot and deltas, plus derived data and local state
      private/        # private deltas: offline layer in a shared repository (FR-187), local-only annotations (NFR-012)
      journal/        # every operation applied or written (FR-194)
      recovery/       # versions displaced by resolutions (FR-192)
      cache/          # derived data keyed by tree hash (FR-024)
      cookies/  daemon.lock  daemon.json  daemon.log  downtime.log  agent-kit.json  ui.json
  ```

  In offline mode the whole directory is excluded through `.git/info/exclude`, and `README.md`, `.gitignore` and `.gitattributes` are not created. A repository initialized with the earlier MVP layout (live store at `.resonance/graph.db`, daemon files directly under `.resonance/`) is migrated the first time a newer binary opens it: the old store is backed up (FR-184), its operation log is folded into `local/state.db`, a snapshot is written to `graph.db`, and the daemon files move under `local/`. The migration is reported once.
- **MVP-026 Canonical serialization and the delta format** (resolves OQ-02 for the MVP). A delta is a UTF-8 JSON Lines file named `<ulid>.delta` (FR-076). Line 1 is the header (FR-075: delta ID, format version, schema version, writer and Resonance version, epoch, starting branch); each following line is one operation in the §9.1 envelope; the last line is the file manifest (path → previous and new content hash). Serialization is canonical (FR-153): object keys sorted by code point, no insignificant whitespace, numbers in their shortest round-trip form (a confidence stays a decimal between 0 and 1), and strings escaped only where JSON requires it. The same operation therefore always has the same bytes. The format is engine-neutral (FR-164); `resonance git-driver textconv` renders a delta for people.
- **MVP-027 Fold and snapshot.** The authored and identity layers are a deterministic fold (FR-150) over the snapshot's operations, the delta operations reachable at HEAD, the open delta and, for the developer only, the private layer. Order (FR-151): deltas by the topological order of the commits that introduced them (one `git log --topo-order --reverse` pass over `.resonance/deltas/`; for offline deltas, the commits they were sealed on, MVP-030), uncommitted deltas last, then `hlc`, then `opId`. Operations are deduplicated by `opId` (FR-193). The snapshot is a SQLite file holding every operation it folded, in fold order (FR-157); the authored and identity data are their fold, materialized in the live store rather than repeated in the snapshot. It is written deterministically: a fresh database, page size 4096, rollback journal, rows inserted in fold order, then `VACUUM INTO` a temporary file that is renamed into place. Its `snapshot_meta` table records the format identifier (`resonance-snapshot-sqlite`), format version, schema version, epoch (the SHA-256 of the sorted IDs of the folded deltas) and those IDs (FR-074, FR-165). Two clones folding the same operations produce byte-identical snapshots (FR-152). `local/state.db` is the live store and is never the snapshot (FR-169). `resonance rebuild` re-folds from scratch.
- **MVP-028 Derived-data cache** (FR-024). *Not built in the MVP: settling after a re-fold rebuilds file records from stored blobs without reading the disk, which has been fast enough; the cache returns if field tests (B17) show otherwise.* After a rebuild for a commit, the derived tables (file records, anchor positions, containment) are saved in `local/cache/` under the commit's tree hash and the hash of the anchor set, and reused when a later checkout returns to the same state. The cache is a performance aid only; deleting it is always safe.
- **MVP-029 Git monitor** (FR-067, FR-068). The daemon also watches, inside the git directory (`git rev-parse --absolute-git-dir`): `HEAD`, `ORIG_HEAD`, `MERGE_HEAD`, `packed-refs`, `refs/heads/` and the reflogs (`logs/HEAD`, `logs/refs/heads/`), as well as the delta files. Per-event ingest is suspended while `index.lock` exists (MVP-023); a multi-step operation such as a rebase is caught up when `HEAD` settles. When the reflog has advanced since the last catch-up, including a round trip that returns to the same commit, the operation is identified from the reflog entries since the reflog position recorded at that catch-up (`checkout`, `commit`, `commit (amend)`, `merge`, `pull`, `rebase`, `reset`, `cherry-pick`, `revert`), and the daemon catches up for the new commit in the order of FR-068 and FR-195, producing one summary. Git hooks (MVP-035) only make this prompter. Watching `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `REBASE_HEAD`, `rebase-merge/` and `rebase-apply/` to report an operation in progress, and the common git directory of worktrees (FR-070), wait for field tests.
- **MVP-030 Branch-aware offline deltas** (FR-183, resolves OQ-18). In offline mode an open delta is sealed on the first commit made (by `commit` or `commit (amend)`) since the reflog entry Resonance last caught up at, not merely since the commit hash (a checkout can return to the same hash), found by walking the reflog forward from that entry, and is associated with that commit in `state.db`. The graph for a checkout is the snapshot, plus the sealed deltas whose commits are ancestors of `HEAD`, plus the open delta. After a rewrite (`rebase`, `commit (amend)`, a squash; from the reflog, or from the `post-rewrite` hook's input when installed), associations move to the rewritten commit with the same `git patch-id --stable`, or, for an amend, to the new `HEAD`. Deltas that cannot be re-associated go to the **detached pool**, which `status` and the repair list show; `resonance recover` can attach a pooled delta to `HEAD` or export and drop it. Nothing is dropped silently. Offline compaction folds only deltas whose commits are reachable from the trunk (MVP-034).
- **MVP-031 Divergence classification** (FR-190, FR-195, FR-198). The daemon classifies every disagreement into the classes C1–C9 of §8.18, using as evidence: the last reconciled commit, `HEAD`, the reflog, `git stash list`, the commits since the last reconcile with their changed files and delta manifests, the index, and the daemon's own downtime record (`local/downtime.log`). Each classification is a record (path, class, evidence, resolution, level, time), counted per class, listed by `status` and `resonance recover`, and resolved by authority (FR-191). Levels are *info*, *attention* and *action required*; only *action required* waits for a person. The team-wide message conventions of FR-200–FR-206 stay in B8.
- **MVP-032 Journal, recovery and the recovery writer.** Every operation applied or written is appended to `local/journal/` (FR-194). A resolution that displaces anything stores the displaced bytes and a record (class, evidence, what was kept, how to undo) under `local/recovery/<id>/`, and `resonance recover undo <id>` reverses it (FR-192). The only project-file writes in the MVP are recovery restores (FR-012b, FR-196) and opt-in intent projection (FR-121). Both use an atomic replace in the same directory, preserving permissions and line endings (FR-050), and record each write's path, content hash and generation so that the watcher ignores the events it causes (FR-044). Graph-to-disk code mutations stay in B10.
- **MVP-033 Stage, participation and the open delta.** The repository's stage and the minimum binary version are recorded in the committed `config.toml` (`[repository] stage`, `minVersion`, FR-264); a working tree's participation is recorded in local git configuration (`resonance.participation = user | offline`). A developer has opted in when the stage is 2 and their participation is `user` (FR-203). The open delta (FR-077) is `.resonance/deltas/<ulid>.delta`, rewritten after each change set through a temporary file and a rename. When `HEAD` moves to a commit whose tree contains the open delta (`git ls-tree`), the delta is sealed. If the committed bytes are an earlier version than the daemon's, the committed version is what is sealed, and the remaining operations move to a new open delta. The manifest records every file content change the daemon observed while the delta was open.
- **MVP-034 Compaction** (FR-154–FR-157, FR-159). The trunk is `compaction.trunk` from `config.toml`, else the target of `refs/remotes/origin/HEAD`, else `main`, else `master`. Private deltas under `.resonance/local/private/` are not sealed history: compaction does not count them, fold them into the snapshot, or delete them (FR-187). When `HEAD` is on the trunk and it has at least `compaction.maxDeltas` sealed deltas (default 100), the daemon compacts: it writes a new snapshot and deletes the folded deltas as working-tree changes, never as a commit (FR-156), and tells the developer. An agent runs it explicitly with `resonance_compact`, only when the developer asks (FR-218). A conflict on `graph.db` from colliding compactions is resolved by the merge driver `resonance git-driver merge`, registered in the local git configuration at onboarding, which unions the two snapshots' operations and re-folds; without the driver, `resonance recover` repairs it (C6). A `git revert` that removes a delta already folded into the snapshot puts an item on the repair list that offers the inverse operations. Folded operations are never pruned in the MVP (OQ-16).
- **MVP-035 Git hooks** (FR-069, FR-177). `init` installs the hooks for the working tree's participation, and `share`, joining and `leave` change them. Hooks go in the directory given by `git rev-parse --git-path hooks`, which honors `core.hooksPath`. For opted-in developers:
  - `pre-commit` flushes the open delta, stages it when any file in its manifest is staged, runs `resonance check --staged`, and blocks on errors with the fix (FR-203). It warns, without blocking, about pending change-log entries (FR-277).
  - `pre-push` runs `resonance check --range` over the outgoing commits, and blocks on errors.
  - `post-checkout`, `post-merge` and `post-rewrite` notify the daemon (and pass the rewrite pairs for MVP-030).

  Offline trial users get only the non-blocking `post-*` hooks (FR-188). Each hook is a block between `# >>> resonance >>>` and `# <<< resonance <<<` that runs `resonance hook <name> "$@"`. A missing hook file is created as a POSIX shell script; an existing shell script gets the block appended after its own content. If the existing hook is not a shell script, or a hook manager is detected (`.husky/`, `.pre-commit-config.yaml`, `lefthook.yml`, or a `core.hooksPath` inside the working tree), Resonance writes nothing and prints the lines to add. `leave` and `eject` remove only the marked blocks.
- **MVP-036 Checks and review** (FR-221, FR-066). `resonance check --staged` and `resonance check --range <a>..<b>` return findings, each with a code, a severity (`error`, `attention`, `info`), a message and a suggested fix, as text or `--json`; the exit code is 3 when any finding is an error. Findings: `R001` the open delta is not staged with the code it describes; `R002` a staged or committed file changed outside Resonance (no manifest entry); `R003` something under `.resonance/local/` is staged; `R004` a compaction is staged on a non-trunk branch; `R005` a sealed delta or the snapshot was modified; `R006` unresolved `Conflict` nodes; `R007` the branch will conflict in `.resonance/` when merged into its target; `R008` *action required* divergences are unresolved; `R009` change-log entries are pending (*attention*); `R010` the agent kit is missing or stale (*info*). The MCP tool `resonance_review` summarizes the graph changes in a range (intent added, changed and retired; links added, removed and changed in status; annotations; identity operations; conflicts; compactions), with a Markdown rendering for pull request descriptions. `.resonance/.gitattributes` names `resonance git-driver textconv` as the textconv driver for `*.delta` and `resonance git-driver merge` for `graph.db`; onboarding registers both in the local git configuration.
- **MVP-037 Sharing, joining, leaving.** `resonance share` (FR-185) runs a preflight, then a privacy review: every item that would be committed, by type and with counts, and the full text of every annotation, with `--keep-local <id>…` to move items to the private layer. It then compacts local history into a fresh snapshot, moves the exclusion to `.resonance/.gitignore`, writes `README.md`, `.gitattributes` and `config.toml` with stage 2, adds the agent kit's instructions section to the shared `CLAUDE.md` (or `AGENTS.md`, when that is what the repository uses; listed in the preview, FR-213), installs the hooks and refreshes the local agent kit for stage 2, and leaves ordinary new files and changes for the developer to commit. `resonance init` in a repository with committed data joins it as a Resonance user (FR-172): offline data, if any, is merged in as a new delta after the same privacy review (FR-186). `resonance init --offline` instead starts a private offline layer over the shared graph, which reads the committed data and never writes it (FR-187). `resonance unshare` exports, then leaves the removal of the committed `.resonance/` files as working-tree changes to commit, and restores the local exclusion. `resonance leave` requires the open delta to be committed or exported, removes the hook blocks, the local kit and the participation setting, and stops the daemon.
- **MVP-038 The agent kit** (FR-210–FR-214, B7). `resonance init` installs the kit (`--agent claude-code`, the default; `--agent generic`; `--agent none`), and running `init` again refreshes it. For Claude Code it installs:
  - the MCP server at local scope;
  - a user-level skill (`~/.claude/skills/resonance/SKILL.md`): the static part, which says Resonance is present, lists the rules that hold even when the daemon is unreachable (never edit `.resonance/` by hand, never bypass hooks, read the change log, run `resonance check` before committing), and says where to get the rest;
  - harness hooks in `.claude/settings.local.json`: a `PreToolUse` hook on shell commands that runs `resonance agent-hook pre-git` before `git commit`, `push`, `pull`, `merge` and `rebase` (FR-222, FR-277); a `UserPromptSubmit` hook that runs `resonance agent-hook prompt`, which adds the pending change-log entries to the agent's context with every prompt and prints nothing when the log is empty (FR-283); and an environment setting `RESONANCE_ACTOR=agent:claude-code` (MVP-040);
  - an instructions section in `CLAUDE.local.md`; from stage 2, `share` also puts a section in the shared `CLAUDE.md` or `AGENTS.md`, worded to be safe for every kind of participation (FR-213).

  Files that must stay local are added to `.git/info/exclude` when no ignore rule covers them. The dynamic part is the MCP `instructions` and a `resonance_conventions` tool, both specific to the stage, the participation and the binary's version (FR-212). The installed kit's version is recorded in `local/agent-kit.json`; `status`, `doctor` and finding `R010` report a missing or stale kit, and `init` refreshes it. For other agents, `init --agent generic` prints an `AGENTS.md` section and the MCP command line. Onboarding ends with the orientation of FR-225, which the kit asks the agent to give.

  As built: the developer's override of the agent-side gate (FR-222) is an explicit `RESONANCE_OVERRIDE=1` prefix that the agent adds to the git command only when told to. Joining (`init` in a shared repository) installs the kit like any `init`; `share` refreshes it for stage 2. `leave` and `eject` remove the harness hook, the actor setting, the local instructions section and `local/agent-kit.json`; the user-level skill stays, since it serves every repository, and so does the MCP registration, which `claude mcp remove` undoes.
- **MVP-039 Effectiveness counters** (FR-224, partial). The agent-side gate counts, locally (a commit passes on the first attempt when no denial preceded it since the agent's last allowed commit): agent-made commits checked, those that passed on the first attempt, agent-hook denials that were overridden, and divergences attributed to agent actors. `resonance_value` reports them. Nothing leaves the machine (NFR-011).
- **MVP-040 The change log** (FR-274–FR-278). Entries live in `local/state.db`, never in the snapshot or deltas. An entry is recorded after each change set that changes a spec item, when its actor is a person (the web UI or the TUI) or its origin is git (deltas arriving through a `HEAD` movement), and for agents only with `changes.includeAgents`. Fields: `seq` (an integer that increases monotonically per working tree and is never reused), `at`, `actor`, `item` (key and a title of at most 60 characters), `kind`, `fields`, `summary` (one line, at most 100 characters), `origin` (`ui`, `tui`, `mcp`, `git`) and the operation IDs. The actor is resolved as follows: an MCP client is an agent (FR-134); the web UI and the TUI are the person named by `git config user.email`; a CLI process is the agent named by `RESONANCE_ACTOR` when set (the kit sets it in the agent's environment, MVP-038), and otherwise that person. The log is read and cleared through MCP: `resonance_changes` (with `since`), `resonance_changes_show`, `resonance_changes_done`; people view and clear it in the web UI and the TUI (FR-285). `status` and `resonance_status` include the pending count. While entries are pending, every MCP tool result except the change-log tools carries a `pendingChanges` reminder, and the prompt hook of MVP-038 puts the entries themselves in front of the agent (FR-283). Spec item views (`resonance_show_item`, `resonance_list_items`, the web UI, the TUI) carry a `pendingChange` flag (sequence number, kind, fields, origin) for items with a pending entry, retired ones included; it is derived from the log on every read and never stored on the item (FR-284). A new store's first reconcile and joining a shared graph record nothing: they establish the starting state.
- **MVP-041 The TUI** (FR-270–FR-273, FR-279). `resonance tui` is built with `ratatui` and `crossterm` and is a client of the daemon, subscribing to change notifications (FR-104) so that it stays current. Screens:
  - **List:** key, type, status, title and a link summary; `/` filters by text, `t` by type, `s` by status.
  - **Detail** (`enter`): every field, links with their statuses, the latest evidence, annotations and, for an item that came from a document, the document and line (from its `derivedFrom` link, MVP-042).
  - **Edit** (`e`): short fields inline; long text in `$VISUAL` or `$EDITOR` on a temporary file. Every item is edited in place; for an item that came from a document, any change to its fields, including status and kind, makes its `derivedFrom` link stale, and the agent brings the document back in step (FR-272). Retiring an imported item does the same.
  - **Add** (`a`): type, a suggested key (FR-273) and fields.
  - **Retire** (`r`): with confirmation and the item's link count.
  - **Change log** (`l`): pending entries; `j`/`k` select one, `d` clears it, `D` clears them all after a confirmation (FR-285).
  - Items with a pending change are marked `•` in the list, and the detail says which change (FR-284).
  - `?` shows help and `q` quits.

  Its changes carry the person's identity. It is tested with `ratatui`'s test backend.

  As built: forms save with `ctrl-s` (or `F2`) and cancel with `esc`; `g` reloads. On the detail screen, `j`/`k` (and `↑`/`↓`) scroll and `n`/`p` move to the next and previous item; `pgup`/`pgdn` scroll a page. The change log scrolls the same way and `esc` leaves it. In direct mode, where there is no daemon to subscribe to, the TUI reloads every two seconds. An existing key is refused at creation (FR-273).

- **MVP-042 Agent import of spec documents** (FR-122, FR-124, S26). Resonance reads no documents; the MCP tool `resonance_import_items` applies what an agent read. Arguments: `document` (a repository path), `items` (each with `type`, `key`, `fields`, and the `region` of the document it came from: `startLine`/`endLine`, or an exact `match` that occurs once), and `dryRun`. The result is a plan, item by item: `create`, `update` (with the fields that differ), `unchanged` or an error (an unknown type or field, a key used by an item of another type, a region that does not resolve), plus any clash between items in the same call. A dry run writes nothing; otherwise nothing is written unless every item validates. Applying creates or edits each item, declares its region as a code unit of kind `spec` named after the key (re-anchoring it when it moved), and links the item to it with `derivedFrom` (confidence 1.0). Apply refreshes that link's baseline only when this call changed the item or moved its region and the item's text agrees with the region's quote; a dry run reports the link's status (`linkStatus`) and whether apply would refresh it (`refreshLink`). An empty required field is an error on an update as well as a create, and the accepted items are written as one change set. Items the call does not mention are untouched; the agent retires what disappeared, with the developer's agreement. The agent kit (MVP-038) carries the workflow: read the documents; preview the plan and any reformatting (each item clearly delimited and carrying its bold key) for the developer; reformat first, if agreed, so that regions point at the final text; then apply. A stale `derivedFrom` link to a `spec` unit means one side changed: if the item changed, the agent updates the document and confirms the link; if the document changed, it shows the developer the difference and, once confirmed, applies it to the item (the graph is the source of truth).
- **MVP-043 The web UI** (FR-286–FR-292). Each daemon serves a web UI on loopback, for as long as it runs.
  - **Server.** A new crate, `resonance-web`, holds the HTTP server and the embedded bundle; the binary starts it inside the daemon process, as an in-process client of the daemon, so `resonance-core` gains no HTTP or asset code. The server is `tiny_http`: synchronous, with a thread per connection, like the daemon's socket listener (MVP-019), and no async runtime. Its requests reach the owner thread like socket requests, as the person named by `git config user.email`, with origin `ui` (MVP-040).
  - **Address.** It binds `127.0.0.1` only. The port is `ui.port` when set and free, else the port this working tree had last time (in `local/ui.json`) when free, else one the operating system assigns. The address, without the token, goes into `daemon.json` and the registry entry. If it cannot bind, the daemon serves everything else, and its health says why. `ui.enabled = false` turns the web UI off.
  - **Token** (FR-288). `local/ui.json` (mode `0600`) holds the last port and a token of 32 random bytes, made when the file is missing; deleting the file makes a new token at the next start. `resonance ui` opens `/auth?token=…&next=<path>`: the server checks the token, sets it as an `HttpOnly`, `SameSite=Strict` cookie, and redirects to `next` (a local path only), so that the token leaves the address bar. Browsers do not separate cookies by port, so the cookie's name carries a short hash of the repository root, and each server reads only its own. Every request must carry the cookie and a `Host` of `127.0.0.1:<port>` or `localhost:<port>`; every `POST` must carry its own `Origin`. A refused request gets `403` and a page that says to run `resonance ui`. Responses carry a strict `Content-Security-Policy` (`default-src 'self'`, `frame-ancestors 'none'`), `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`, and no CORS headers.
  - **HTTP API.** `GET` serves the embedded bundle (hashed asset names, cached; `index.html` uncached and served for every UI address). `POST /api/call` takes `{"method", "args"}` and returns the same result or error as the socket API, for an allowlist: the methods the TUI uses, `file_text` (a tracked file's text, served from the graph so that nothing else on disk can be read), the diagnostics and log methods of MVP-044, and the actions of FR-295. Anything else is refused. `GET /api/events` is a Server-Sent Events stream of change notifications (FR-104), diagnostics events and log records. Every message has an ID, so a reconnecting browser resumes with `Last-Event-ID` while the buffers still hold the gap, and otherwise reloads. The stream counts as a client, so the daemon does not go idle while a page is open (FR-084).
  - **`resonance ui [<key>] [--no-open] [--json]`.** It starts the daemon if needed (FR-084), waits until it is ready, and opens the address with the token in the browser named by `$BROWSER`, else with `open` (macOS) or `xdg-open` (Linux). It prints the address either way; `--json` gives `url`, `port` and `root`. It fails, saying why, in direct mode, when the web UI is turned off, and when the listener could not start.
  - **Frontend.** The source is in `web/` at the repository root: TypeScript and React, built with Vite; Mantine for components, hotkeys, notifications, the command palette (Spotlight) and light and dark themes that follow the system; TanStack Query for server state, invalidated by the event stream; TanStack Virtual for long lists (activity and logs); and React Router. Fonts and icons are bundled. Dependencies are installed with `npm` from a committed lockfile.
  - **Build.** `just ui-build` runs `npm ci` and `vite build` into `web/dist`. `just build` and release builds run it first, and `resonance-web` embeds `web/dist` at compile time. A build without `web/dist` embeds a placeholder page that says to run `just ui-build`, so `cargo build` and `cargo test` need no Node. `just check` adds `ui-check`: type checking, lint and the frontend tests.
  - **Screens.** Every screen has a header with the repository, branch, stage, daemon state and the number of pending change-log entries.
    - **Spec** (`/spec`, `/spec/<key>`): the list, with a text search, checkbox facets (type, status, pending change, link state, source; nothing checked shows everything, values of one facet are alternatives and facets narrow each other, each value shows its count) and sortable columns, kept in the query string; a link summary and the pending mark (FR-284). Beside it, the selected item's detail: every field, links with their statuses, the latest evidence, annotations and, for an item that came from a document, the document and line. Clicking a link's target opens a drawer: for a code unit, test or document region, the whole file (from the graph, through `file_text`) with the linked lines highlighted, and a note when the region is orphaned; for a spec item, the item and a way to open it. Editing is in place: short fields inline, long text in a Markdown editor with a preview. Adding takes a type, offers the suggested key (FR-273) and refuses an existing one. Retiring asks for confirmation and shows the link count (FR-271).
    - **Change log** (`/changes`): pending entries, each with its full before and after on request (as `resonance_changes_show`); clear one, or all after a confirmation (FR-285).
    - **Diagnostics** (`/diagnostics`) and **Logs** (`/logs`): MVP-044.
  - **Keys** follow the TUI's: `/` filters, `a` adds, `e` edits, `r` retires, `l` opens the change log, `j`/`k` move, `n`/`p` go to the next and previous item, `mod+s` saves, `esc` cancels and `?` shows help; `mod+k` opens the command palette.
  - **Edits in progress** (FR-292). When the event stream reports a change to the item being edited, the form shows the newer values and asks whether to keep the person's edits or take the newer version before saving.
  - **Tests.** Rust integration tests drive the HTTP API against a temporary repository; the frontend's components are tested with Vitest and Testing Library against a stubbed API.
- **MVP-044 Diagnostics and logs** (FR-293–FR-298).
  - **Events.** The daemon records diagnostics as events in a ring buffer (`diagnostics.bufferEvents`, default 10000), each with `seq`, `at`, `kind`, a one-line `summary` and `fields`. Kinds: `request` (method, client, actor, origin, duration, outcome), `client` (connected or disconnected, with kind `mcp`, `tui`, `ui` or `cli`), `fs` (a batch of file events), `reconcile` (trigger, files, re-anchored units, duration), `git` (the operation from the reflog, the commits, deltas applied), `changeset` (actor, origin, operation count), `changelog` (recorded or cleared), `divergence` (class, path, level, resolution), `delta` (sealed or compacted), `registry` (a failed registration or renewal), `state` (a health transition), `action` (FR-295) and `ui` (the listener bound or failed). Fields hold identifiers, keys, paths, counts and durations, never item text or file content (FR-294). Counters (per method: count, errors, and median, 95th percentile and maximum duration) are kept for the whole session beside the buffer, and are not dropped with old events.
  - **Methods.** `diagnostics` returns identity, health, configuration with each value's source, counters, clients, the store summary of FR-293, the oldest retained `seq` and the number of events dropped, and events after `since`, filtered by kind. `logs` returns log records after `since`, filtered by level. Both are served from memory and never wait for the ingest watermark. The UI's export downloads both as one JSON file.
  - **Actions** (FR-295): `reconcile` (with `full`), `rebuild_derived`, `daemon_restart`, `daemon_stop` and `log_level`, the same operations as `resonance reconcile`, `rebuild-derived` and `daemon restart|stop`. The UI asks for confirmation before a full reconcile, a rebuild, a restart and a stop, since they make other clients wait or disconnect them. To restart, the daemon starts its successor as a detached process, as a client would, and exits; the successor takes the same port and token, so the page reconnects. After a stop, the page says that `resonance ui` starts the daemon again.
  - **Logging.** The daemon logs through `tracing`: what it now writes to standard error becomes records with a level and a component target (`daemon`, `watch`, `ingest`, `git`, `registry`, `store`, `ui` and so on). Records go to two places. One is the log file: `log.file` (default `.resonance/local/daemon.log`), `log.level` (default `info`), `log.format` (`text` or `json`, default `text`) and `log.maxBytes` (default 1 MiB), past which the file moves to `<file>.1` and a new one starts. The other is a ring buffer for the UI (`log.bufferLines`, default 5000). `RESONANCE_LOG` overrides the level, and also accepts per-component directives (`info,ingest=debug`); `RESONANCE_LOG_FILE` overrides the file. A relative path is resolved from the repository root and `~` is expanded. A file inside the working tree but outside `.resonance/local/` is refused, because it would be ingested and show in `git status` (NFR-001). A file that cannot be opened is refused too, and the daemon logs to the default file with a `warn` record saying why. `log_level` changes the level of both for the rest of the daemon's life. Panics and anything else written to standard error still go to `.resonance/local/daemon.log`, where clients' start-up errors point.
  - **Screens.** **Diagnostics** shows identity, health and configuration; the activity timeline (virtualized, filtered by kind and text, paused or following, with a note when old events were dropped); counters; connected clients; the store summary; the actions; and the export. **Logs** shows the live log, filtered by level, component and text, following or paused, with copy and export, and the session's level.

---

## 3. Requirements in the MVP

### 3.1 Model and link lifecycle  `[model]`

- **§6.2 core node types.** `Requirement`, `Decision`, `Design`, `Constraint`, `File`, `CodeUnit` (a declared region, MVP-013), `Test` (a `CodeUnit` with a `run` command), `Evidence`, `Annotation` and `Conflict`, with the key properties listed in §6.2. A requirement has a `kind` (functional, non-functional) and a status of `draft`, `active`, `superseded` or `retired`.
- **§6.3 core edge types (primary graph).** `satisfies` (CodeUnit/File → Requirement), `verifies` (Test → Requirement or CodeUnit), `derivedFrom` (Requirement/Design → Requirement, or any spec item → the `spec` code unit or file it came from, MVP-042), `constrains` (Decision/Constraint → any), `supersedes` (same type → same type), `realizes` (CodeUnit → Design), `annotates` (Annotation → any), `contains` (File/CodeUnit → CodeUnit, derived from anchor ranges), `evidences` (Evidence → edge). `references` lives only in parallel graphs (B13). Every authored edge carries `provenance`, `author`, `createdAt`, `confidence` (for agents), `status`, and endpoint hashes (MVP-012). Display uses reverse names ("satisfied by", "verified by", and so on), and visualizations omit reverse edges.
- **§6.4** Stable identity is anchored on intent: requirements, decisions, designs, constraints and tests carry permanent IDs, and code units are the current realization, whose links are re-confirmed when code is rewritten.
- **FR-007** Projects may define namespaced extension types for nodes and edges (for example `acme:Service`); extensions never change the meaning of core types.
- **FR-008** Unknown extension types are preserved on read and write, through export, restore, deltas and snapshots.
- **FR-001** Every edge has exactly one status: `asserted`, `verified`, `stale` or `orphaned`.
- **FR-002** A change to either endpoint of a `verified` edge moves it to `stale` (with the MVP-012 extension for requirement text).
- **FR-003** An edge whose endpoint is removed without an identity operation becomes `orphaned`, not deleted (MVP-005 makes this reversible).
- **FR-004** `verifies` edges are verified by recording test results as `Evidence` (§3.8).
- **FR-005** Queries can filter by link status; "requirements with no verified test" is a single query (`resonance_unverified`).
- **FR-006** The schema version is stored in the graph (`meta.schema_version`), and migrations run at startup.

### 3.2 Source of truth  `[truth]`

- **FR-010 (partial)** The graph holds the exact content of every non-ignored file as content-addressed blobs (with the exceptions in MVP-016). Offline, git and the file tree are authoritative and the graph follows them (FR-188). For an opted-in developer the graph is authoritative for file bytes, which shows in conflicts (FR-012b): a file whose bytes the graph does not hold is never restored from it.
- **FR-011** Anchor positions and containment are recomputable from file content plus the recorded anchors, and are never the only record of anything authored.
- **FR-012a** A changed file updates the graph's copy whether or not it parses.
- **FR-013 (partial)** Resonance parses only its own configuration and data, never source code or intent documents (NG6). A `config.toml` that cannot be parsed is reported as an error that says what is wrong with it.
- **FR-014** The working tree stays a standard project. Outside `.resonance/`, Resonance writes only: the `.git/info/exclude` line; marked blocks in git hooks (MVP-035); driver entries in the local git configuration (MVP-036); local agent configuration (MVP-038); recovery restores (MVP-032); and, with explicit confirmation (`share`'s preview), the shared agent instructions file (FR-179). Documents that agents import, reformat or project are written by the agent with its own tools, after the developer's confirmation (MVP-042).
- **FR-015** All graph-only information is reachable only through the CLI, MCP and TUI, never as files in the source tree. Projected intent documents (FR-121) are the one opt-in exception.
- **FR-188** Offline: disk edits are ingested, never treated as conflicts, and Resonance never blocks any git operation. Only the non-blocking `post-*` hooks are installed (MVP-035).

### 3.3 Ingest and reconcile  `[ingest]`

- **FR-047** On startup, the daemon reconciles the graph against the disk before serving queries, using content hashes. The fast path skips files whose `(size, mtime)` are unchanged; changed files are re-hashed, and only real content changes are processed. FSEvents' replay-since-event-ID optimization is not used; the stat scan is always the fallback it would need. In direct mode (MVP-002) every command reconciles this way first. A full re-hash is available as `resonance reconcile --full`.
- **FR-049** Ingesting a changed file is incremental: re-hash, update the blob, re-locate the anchors in that file (MVP-013), update link statuses (MVP-012).
- **FR-045** Always ignore `.git/`, `.resonance/` and editor scratch files (`*.swp`, `*~`, `.#*`, `4913`).
- **FR-046** Respect `.gitignore` (and `.git/info/exclude` and global excludes, through the `ignore` crate), and also ignore the default build and output directories listed in FR-046. Extra patterns can be set in `config.toml` (`ingest.ignore`).
- **FR-035** File identity is path-based. Atomic save-by-rename is just a new version of the same file.

### 3.4 Identity  `[identity]`

- **FR-030** Authored nodes, including code units and tests, get permanent ULIDs; requirements get deterministic IDs (MVP-008).
- **FR-031** Every clone agrees on the IDs that authored data refers to: code units are authored nodes with permanent IDs, and their creation and every re-anchoring decision are operations, committed in deltas from stage 2, so other clones apply the same judgment instead of recomputing it.
- **FR-032** After each file change, re-locate every anchor in it per MVP-013, as strengthened by MVP-024 (character-level boundaries, similarity-aware line survival, substring refinement of approximate matches, and a search of unchanged files for moved regions), carrying IDs, links and annotations across. No parsing is involved (NG4).
- **FR-033** Moves, approximate re-anchors, splits and merges are recorded as `identity.move`, `identity.retarget`, `identity.split` and `identity.merge` operations, with their confidence, and committed from stage 2. Splits and merges are declared by people and agents through `resonance_split_unit` and `resonance_merge_unit` (MVP-024).
- **FR-034** When matching fails, affected links and annotations become `orphaned` with the last known location (file, byte range, quoted text, address), and are never deleted.
- **FR-036** Real file moves are detected by content similarity, as git does, and recorded as `file.move` operations; links and annotations on the file follow it (MVP-024).
- **FR-037** Agents can re-attach orphaned links and annotations: `resonance_reattach` records an `identity.retarget` operation. `resonance_edit_unit` re-anchors a code unit explicitly (`path`, `startLine`, `endLine`, or `match`).
- **FR-038** Orphaned code units, and approximate re-anchors with confidence below 0.9, go on the repair list. The agent decides what they now correspond to: re-anchor, confirm or retire.
- **§9.4** The identity operations of §9.4 are recorded, committed and applied. Concurrent identity operations that cannot be reconciled (for example, one branch splits a unit while another renames it) produce a `Conflict` node rather than a guess.

### 3.5 Storage and the store API  `[storage]`

- **FR-020** The graph is a SQLite store behind the store API: a snapshot at `.resonance/graph.db` plus delta files, committed from stage 2 and local in offline mode; the live store is `.resonance/local/state.db` (MVP-025, MVP-027).
- **FR-021** The store lives under `.resonance/` at the repository root.
- **FR-022** Every applied change set is one transaction.
- **FR-023** Authored, identity and derived data are separated (by table or by `layer` column), so derived data can be dropped and rebuilt with `resonance rebuild-derived`.
- **FR-025** Local-only files are never committed. Offline, `resonance init` adds `/.resonance/` to `.git/info/exclude`; from stage 2, `.resonance/.gitignore` ignores `local/`. No existing tracked file is modified.
- **FR-024** Derived data is cached by tree hash, so returning to a previously seen state is fast (MVP-028).
- **FR-060** The store's internal representation is optimized for Resonance's access patterns and need not be textual.
- **FR-061** Every change is expressible in the edit algebra (§9), which git carries as deltas.
- **FR-150** The authored graph is a deterministic fold over its operations; `resonance rebuild` re-folds it (MVP-027).
- **FR-151** Fold order: the topological order of the commits that introduced the operations, then `hlc`, then `opId`.
- **FR-152** Compaction is deterministic: two clones compacting the same history produce identical snapshots.
- **FR-153** Committed operations have a canonical serialization: identical operations produce identical bytes (MVP-026).
- **FR-164** The delta format is engine-neutral: serialized edit-algebra operations, readable by any adapter.
- **FR-165** The snapshot carries a format identifier and version; a change of engine or format is a migration compaction, with no rewriting of deltas or git history.
- **FR-169** The committed snapshot is never the live store, so git can replace `graph.db` at any moment safely.
- **FR-160** All persistence goes through a `GraphStore` trait defined in engine-neutral terms (nodes, edges, properties, blobs, operations, traversals). `SqliteStore` implements it.
- **FR-161** The SQLite adapter lives in its own crate (MVP-001). No SQL, SQLite types, row IDs or table names appear in the trait or reach callers.
- **FR-162** The trait covers:
  - transactions (applying a change set atomically);
  - node, edge and property reads and writes;
  - bounded traversal (neighbors filtered by edge type, direction and status, to a depth);
  - blob put and get;
  - file records;
  - metadata (`schema_version`, stage, participation, last reconciled commit, ingest watermark);
  - a change feed (FR-104);
  - snapshot export and import, and delta application;
  - appending operations and iterating over them;
  - schema migration;
  - a simple integrity check.
- **FR-163** `trace`, `why`, context packs and the repair list are implemented in `resonance-core` against the trait, never in the adapter.
- **FR-166** `resonance export` writes the complete authored and identity data as engine-neutral edit-algebra operations in JSON Lines, and `resonance restore <file>` imports them. Export is the format of last resort for backups, `share` and `eject`.
- **FR-167** A store conformance test suite lives in `resonance-core` as generic tests over `GraphStore`. `SqliteStore` and a second, in-memory adapter (`crates/resonance-store-memory`) both pass it.
- **FR-168** No engine-specific escape hatch (raw SQL or direct file access) is exposed.

### 3.6 Intent  `[intent]`

- **FR-120** Requirements, decisions, designs and constraints can be created, edited, superseded and retired through MCP (`resonance_create_item`, `resonance_edit_item`, `resonance_retire_item`, `resonance_supersede`, and the shortcuts `resonance_create_requirement` and `resonance_create_decision`) and in the web UI and the TUI. There are no CLI commands for them (FR-259).
- **FR-121 (partial)** Projection is an agent workflow in the kit: on request, the agent writes graph-native items as a Markdown document, after a preview and the developer's confirmation (FR-179), and links each item to its region (MVP-042), after which the document is kept in step like an imported one. Continuous projection by Resonance waits for the projection writer (B10).
- **FR-122** Importing spec documents is an agent workflow (MVP-042, S26): the agent reads Markdown with requirement keys, GitHub Spec Kit `spec.md` files, Kiro `requirements.md` / `design.md` / `tasks.md`, EARS-style statements and ADR directories (as `Decision` items keyed `ADR-NNNN`) itself, previews the plan with `resonance_import_items` and `dryRun`, and applies it once the developer agrees. The kit tells the agent how these formats are usually laid out, and never to invent keys silently: items without a stable identifier are proposed with a key for the developer to accept. Resonance contains no document parser (NG6).
- **FR-123** Annotations are graph-only (option a, resolving OQ-10 for the MVP). Resonance does not scan files for tags such as `@satisfies FR-042`; an agent that finds them may turn them into links. Agents never write tags in offline mode (FR-189).
- **FR-124** An item that came from a document is linked to its region with `derivedFrom`, and the link becomes stale when either side changes; the agent then brings the two back in step, the graph being the source of truth (MVP-042).
- **S4 annotations.** Annotation kinds are `note`, `warning`, `call-out` and `instruction`. A call-out can name a task label, so that a set of code units can be called out for one task (`resonance_annotate` with `kind: call-out` and a `task`).

### 3.7 Code units and tests  `[units]`

- **FR-110** `resonance-core` parses no source code and depends on no language parser. Code units and tests are declared through MCP (`resonance_declare_unit`, `resonance_declare_test`, `resonance_edit_unit`, `resonance_remove_unit`, `resonance_list_units`).
- **FR-111** The structure Resonance provides is files plus the declared code units and tests with their anchors (MVP-013).
- **FR-114** It is language-agnostic: any text file can carry code units.
- **Tests.** A test is a code unit with a `run` command: an exact shell command that runs that test alone (MVP-017). If `run` is omitted, `test.command` from `config.toml` is used, with `{name}`, `{path}` and `{address}` substituted.

### 3.8 Verification  `[verification]`

- **FR-135 / FR-004** `resonance_test` on a requirement, code unit, test or `verifies` link runs the `run` command of every linked test: the tests on `verifies` edges into a requirement, or a single test. Each run records an `Evidence` node (`kind = test`, `result` = `pass`/`fail`/`error`, `commit` = `git rev-parse HEAD`, `timestamp`, `toolVersion`, plus the exit code and the last 4 KiB of output), linked by `evidences` to each `verifies` edge it supports.
  - **Pass** (exit code 0) marks those edges `verified` and refreshes their hashes.
  - **Fail** (non-zero exit code) leaves them `asserted` with the failing evidence attached.
  - **Error** is recorded if the command cannot be started, or if its output matches `test.zeroTestsPattern` (default `running 0 tests`, which catches a `cargo test` filter that matched nothing). An error is never a pass.

### 3.9 Queries, CLI and MCP  `[query]`

- **FR-100** One library API in `resonance-core` serves MCP, the web UI, the TUI and the CLI; graph work is offered only through MCP, the web UI and the TUI (FR-259).
- **FR-102** Read-your-writes: a query that follows a file save or a mutation reflects it. The daemon waits for the ingest watermark to pass the request's sync cookie (MVP-020); `cached` (`--cached` on the command line) skips the wait.
- **FR-103** Mutations are batches of edit-algebra operations, applied atomically (MVP-010).
- **FR-105 (partial)** A fixed set of operations; no query language and no SQL escape hatch.
- **Node addressing.** Tools accept a ULID; a requirement key (`FR-042`); a code-unit address (`path::name`) or a bare unit name where it is unique; or a file path. Ambiguous references list the candidates.
- **FR-259** The CLI is for operations only (G16):
  - setup and lifecycle: `init [--offline] [--agent claude-code|generic|none]`, `status`, `doctor`, `share`, `unshare`, `leave`, `eject`, `version [--json]`;
  - the daemon: `daemon [status]`, `daemon start|stop|restart`;
  - repair: `recover` (with `undo`, `dismiss`, `attach`, `drop`, `repair-snapshot`), `reconcile [--full]`, `rebuild`, `rebuild-derived`, `export [--out]`, `restore <file>`, `backup`;
  - git and CI: `check --staged|--range`;
  - people: `ui [<key>]`, `tui`;
  - services and plumbing: `mcp`, `registry run|install|uninstall`, `hook <name>`, `agent-hook <event>`, `git-driver merge|textconv`, `daemon run`.
- **MCP tools** (FR-130), where all graph work happens: `resonance_conventions`, `resonance_status`, `resonance_repair_list`, `resonance_import_items`, `resonance_list_items`, `resonance_create_item`, `resonance_edit_item`, `resonance_retire_item`, `resonance_supersede`, `resonance_create_requirement`, `resonance_create_decision`, `resonance_create_extension`, `resonance_declare_unit`, `resonance_declare_test`, `resonance_list_units`, `resonance_edit_unit`, `resonance_remove_unit`, `resonance_split_unit`, `resonance_merge_unit`, `resonance_link`, `resonance_unlink`, `resonance_confirm`, `resonance_annotate`, `resonance_reattach`, `resonance_show`, `resonance_search`, `resonance_trace`, `resonance_why`, `resonance_context`, `resonance_unverified`, `resonance_test`, `resonance_changes`, `resonance_changes_show`, `resonance_changes_done`, `resonance_check`, `resonance_review`, `resonance_compact`, `resonance_recover` and `resonance_value`.
- **S2 `resonance_trace`.** One call returns the code units linked by `satisfies`, the tests linked by `verifies`, and each link's status and latest evidence.
- **S3 `resonance_why`.** One call returns the requirements a code unit satisfies, the decisions that constrain it, the tests that cover it, and the annotations on it (including annotations on its containing units).

### 3.10 Agent integration  `[agent]`

- **FR-131** Agents edit files with their own tools, and reconcile picks up the edits. There are no API code mutations in the MVP.
- **FR-130** The MCP adapter exposes navigation, search, linking, annotation, verification and context operations, plus the stewardship operations agents need: conventions, status, checks, review summaries, recovery and the change log.
- **FR-132** Onboarding Resonance includes onboarding the developer's agent (MVP-038). Offline, the kit is installed only into local agent configuration; the shared instructions file is edited only from stage 2 and only with confirmation.
- **FR-210** Onboarding installs the agent kit and is not complete until it has; `resonance init` installs it (and refreshes it when run again), `share` refreshes it for stage 2, and `status` and `doctor` report a working tree whose agent is not onboarded or whose kit is out of date.
- **FR-211** The kit gives the agent foreknowledge of concepts (graph as source of truth, layers, link statuses, stage, participation and authority), conventions (FR-215, FR-216, FR-277), operations (the MCP tools, the few CLI commands an agent runs, and when to prefer them over plain file and git tools), workflows (importing spec documents and keeping them in step, commit, branch, pull request, pull, merge, rebase, compaction, recovery, and consuming the change log), and prohibitions (FR-218).
- **FR-212** The kit is layered: a short static part (the skill and instructions section) and a dynamic part served by the MCP adapter (`instructions` and `resonance_conventions`), specific to the stage, participation and binary version.
- **FR-213** Placement follows the stage. Offline the kit is local and is the *offline* variant: repair duties (FR-238, FR-239) and invisibility (FR-189) instead of commit and pull request duties. From stage 2 a short section may go in the shared instructions file, with confirmation, worded so that every agent can tell from local configuration which case applies: a Resonance user's agent has the full duties, and any other agent works normally and never edits `.resonance/`.
- **FR-214 (partial)** Claude Code is the supported harness (MCP server, skill, instructions section and harness hooks); other agents get a generic `AGENTS.md` section and the MCP server. The kit is versioned with the binary; running `resonance init` again refreshes it, and `status` and `doctor` flag a stale kit. First-class kits for other harnesses are OQ-21.
- **FR-215** Before every commit the agent waits for the daemon to catch up, makes sure no *action required* divergences are unresolved, runs `resonance check --staged` and fixes what it reports, and stops and asks the developer when a finding cannot be fixed safely.
- **FR-216** Before opening or updating a pull request the agent runs `resonance check --range <base>..HEAD`, re-verifies links the change made stale or reports them, and includes the `resonance_review` summary, in Markdown, in the pull request description.
- **FR-217** During pulls, merges and rebases the agent never hand-edits `graph.db` or delta files, resolves conflicts in `.resonance/` with `resonance recover`, and relays Resonance's *attention* and *action required* items after every git operation.
- **FR-218** Unless the developer explicitly instructs it, the agent does not bypass hooks, write into `.resonance/`, delete or rewrite sealed deltas, commit `.resonance/` in offline mode, run `share`, `unshare` or `eject`, commit a compaction (`resonance_compact`), or run a git command that could discard uncommitted graph state without first checking `resonance status` and getting confirmation.
- **FR-220** The agent relays Resonance's messages plainly and gets confirmation before anything that cannot be undone. When it declines a request because of a convention, it names the convention and offers the compliant alternative.
- **FR-221** Every convention the agent enforces has a deterministic check in Resonance (`resonance check`, `resonance_check`) with machine-readable findings and suggested fixes (MVP-036).
- **FR-222** The kit installs agent-side harness hooks that run the relevant check before the agent's `git commit`, `push`, `pull`, `merge` and `rebase`, and stop the command if it fails (MVP-038). An explicit instruction from the developer overrides a gate, and the result is still checked.
- **FR-223** Agent enforcement is the first line of defense; hooks and checks are the backstop, and classification and recovery the safety net. Nothing in Resonance's correctness depends on agents following the kit perfectly.
- **FR-224 (partial)** The kit's effectiveness is measured locally (MVP-039). Aggregated reporting waits for the metrics of B16.
- **FR-225** Onboarding ends with an orientation: the agent confirms it can reach the daemon, reports the stage and participation (offline, also what offline cannot do), and says in a few lines what it will now do differently.
- **FR-133** A context pack for a task contains: the target requirements; the decisions that constrain them or their linked code; the linked code units as locations (path, line range, address), not as full text; the linked tests with their latest evidence; and the annotations in scope, including call-outs for the given `task`.
- **FR-134** Links created through MCP carry `provenance = agent`, a `confidence` value (required) and the agent's identity (from MCP `clientInfo`), and start as `asserted`.
- **FR-189** Invisibility: the kit tells agents never to put Resonance IDs, trailers or mentions in commits, branches, pull requests or code, and the MVP itself writes nothing visible.
- **FR-219** Traceability duties (in the instructions): declare code units and tests, link them, run linked tests, honor annotations, and keep anchors healthy.

### 3.11 Offline mode and lifecycle  `[offline]`

- **FR-180** When no Resonance data is committed, `resonance init` starts offline: it creates `.resonance/`, adds the exclusion, runs the first reconcile, installs the offline agent kit and the non-blocking hooks locally, and leaves `git status` unchanged. It explains in a few lines what offline mode is and isn't, and that it works best on repositories with few outside changes.
- **FR-181** Offline mode provides every local feature in this file; what it lacks is collaboration through Resonance.
- **FR-238** `resonance status` (and `resonance_repair_list`) shows the **repair list**: orphaned code units, links and annotations with their last known location; approximate re-anchors with confidence below 0.9; stale links; stale `derivedFrom` links between items and the documents they came from (FR-124); retired requirements that still have links; stale realization data; and the detached pool (MVP-030). After every git operation that brings in outside changes, the list is refreshed and summarized; the agent works through it and records each repair as an identity operation (FR-037).
- **FR-239** Repair is lazy and budgeted: the list is ordered by relevance to the current work (files changed in the working tree, then files in the current context pack, then the rest), and agents may record the cost of a repair (`costTokens`, `costSeconds`) when resolving it, which `resonance_value` reports.
- **FR-240** `resonance_value` summarizes, locally, what Resonance did for the developer (traces and context packs served, annotations that survived changes, repairs made and their recorded cost, MVP-039) and, offline, explains what improves on joining.
- **FR-182** Offline mode uses the same store, snapshot-and-delta structure and formats as the shared stage, so sharing or joining is a move, not a conversion.
- **FR-183** Offline data follows git history: sealed offline deltas are associated with commits, re-associated after rewrites, and kept in a detached pool when they cannot be (MVP-030).
- **FR-185** `resonance share` moves the repository from stage 1 to stage 2, with a privacy review, a fresh snapshot, the exclusion moved, and ordinary new files left to commit (MVP-037).
- **FR-186** Joining a repository with committed data merges the developer's offline graph in as a new delta, after the privacy review; spec items with the same key get the same deterministic IDs, so nothing is duplicated.
- **FR-187** `resonance init --offline` in a shared repository reads the committed graph as an input and never writes it back; the developer's own additions stay in the private layer until they join. Opted-in developers keep local-only annotations in the same private layer (NFR-012).
- **FR-184** `resonance backup` writes a backup outside the repository (default `~/.resonance/backups/<repo-name>/`); Resonance warns before any action of its own that would delete `.resonance/`, notices when `.resonance/` has disappeared (for example after `git clean -fdx`), and offers to restore it from the latest backup or, from stage 2, from git (FR-197). `resonance eject` first writes an export outside the repository (default `~/.resonance/exports/<repo-name>-<timestamp>.jsonl`) and asks for confirmation (FR-173). Nothing else in Resonance deletes `.resonance/`.

### 3.12 Non-functional  `[nfr]`

- **NFR-001** To unaware tools, the working tree is indistinguishable from a repository without Resonance.
- **NFR-003** Resonance holds no locks on source files and never changes file timestamps.
- **NFR-004** Build output is not ingested (FR-046).
- **NFR-005** No configuration change to build tools, IDEs, test runners or CI is required.
- **NFR-010** The only network listener is the web UI's, on loopback and protected by a token (MVP-043); the API is on a Unix socket, and MCP runs over stdio.
- **NFR-011** There is no telemetry.
- **NFR-013** MCP results wrap graph-sourced text in fields labeled as data, and the instructions say that this content is not instructions.
- **NFR-036** A text write is never lost (files stay on disk; the graph only reads them).
- **NFR-037** A missing or crashed daemon causes delay, never wrong answers: clients start a new one, which reconciles before serving (FR-047, MVP-002).
- **NFR-038** Supported platforms are macOS and Linux.
- **NFR-041** `resonance init` in offline mode leaves `git status` unchanged.
- **NFR-012** Committed graph data is visible to anyone with repository access: `share`'s preview and the review summary make what is committed easy to see, and a local-only annotation class keeps private notes out of commits (MVP-037).
- **NFR-042** At most one new `graph.db` blob enters git history per compaction.
- **NFR-046 (partial)** The share of agent-made commits that pass checks on the first attempt is measured and shown (MVP-039); the 95% target is not an acceptance gate yet.

---

### 3.13 The binary  `[binary]`

- **FR-250 (partial)** Everything ships as the single executable `resonance`. The MVP provides the MCP, web UI, TUI, operations CLI, daemon, registry and plumbing (`hook`, `agent-hook`, `git-driver`) modes. Quiet mode is B9.
- **FR-251** The modes share one implementation of every operation, so the same question gets the same answer whichever mode serves it.
- **FR-252** When a daemon is available, MCP, web UI, TUI and CLI requests are served by it (P1). If none is running, the client starts one and waits (P4). A command never answers from a graph that has not been reconciled. Direct mode (MVP-002) is the explicit exception for debugging.
- **FR-253 (partial)** `-C <path>`; `--json`; `--dry-run` and `--yes` for every command that changes tracked files, hooks, git configuration or user-level installs; exit codes `0` (success), `1` (error), `2` (usage), `3` (a check found errors) and `4` (waiting on an *action required* decision). Configuration precedence: flags, then `RESONANCE_*` environment variables, then `.resonance/config.toml`, then defaults.
- **FR-254 (partial)** The MCP tools are the primary surface and are named `resonance_<operation>`; they are not a mirror of the CLI (FR-259). `check`, `status` and `recover`, offered by both, share names and JSON.
- **FR-255 (partial)** Everything the binary installs or writes from templates is embedded in it: the agent kit (instructions, skill, harness hook definitions), git hook blocks, `.resonance/README.md`, `.gitignore` and `.gitattributes`, the registry's service definitions, schema migrations and the web UI's bundle. Nothing is downloaded at run time.
- **FR-256 (partial)** `resonance version [--json]` reports the binary version, the store schema version, the delta and snapshot format versions, and the MCP protocol version.
- **FR-264 (partial)** The binary follows semantic versioning, and from stage 2 the committed `config.toml` records the minimum binary version the repository's data needs; an older binary refuses to write (FR-258).
- **FR-258** The binary refuses to write a store with a newer schema than it understands, and says which version is needed.

### 3.14 Packaging  `[packaging]`

- **FR-260 (partial)** The binary builds and runs on macOS and Linux, with SQLite bundled.
- **FR-261 (partial)** The MVP channel is build from source: `cargo install --path crates/resonance-cli`, or `cargo install --git <repo>`.
- **FR-263 (partial)** Installing the binary changes nothing anywhere. The agent kit's skill and MCP registration (installed by `init`, which says what it installs) and `resonance registry install` (previewed) are the only user-level installs.


### 3.15 Daemon, watcher and registry  `[daemon]`

- **FR-040** Each repository has a persistent watcher process for its working tree: the daemon, started on demand (MVP-019).
- **FR-041** The watcher uses native file-event APIs: FSEvents on macOS and inotify on Linux (MVP-021).
- **FR-042** The watcher tolerates batched, coalesced, delayed and out-of-order events. It re-examines affected paths by hash rather than trusting event contents.
- **FR-043** The daemon maintains an ingest watermark: the point up to which disk changes have been applied to the graph (MVP-020).
- **FR-044 (partial)** The daemon's writes under `.resonance/` are never ingested as project content, and its project-file writes (recovery restores, MVP-032) are recorded by path, hash and generation so that the events they cause are ignored. The projection writer's writes join this with B10.
- **FR-048** A burst of changes that matches a git operation is handed to the git monitor and applied as one batch (MVP-023, MVP-029).
- **FR-080 (partial)** The watcher, ingest, git monitor and query API for a repository run under one owner (P1). The projection writer (B10) will join it.
- **FR-081 (partial)** Queries are served by the same owner, so they honor read-your-writes. No direct access to the store file is offered to other tools.
- **FR-082** A daemon takes an exclusive lock on `.resonance/local/daemon.lock` at start. The lock, not the registry, guarantees one daemon per repository.
- **FR-083** A daemon writes `.resonance/local/daemon.json` with its endpoint, process ID, Resonance version, schema version and start time, so clients can find it without the registry.
- **FR-084** Clients start a daemon on demand when none is running. Daemons exit after a configurable idle period (`daemon.idleSeconds`).
- **FR-085** The per-repository component is a library that can run in its own process or be hosted alongside others in a shared process later, without design changes.
- **FR-086** Daemons listen on Unix domain sockets in a short per-user runtime directory with user-only permissions (MVP-019).
- **FR-087** The web UI's listener is the only TCP listener (NFR-010). It binds to loopback only and requires a per-working-tree token (MVP-043).
- **FR-088** A system-wide registry provides only service discovery and health monitoring of per-repository daemons (MVP-022).
- **FR-089** The registry never sits between a client and a repository daemon on any request (P5).
- **FR-090** Daemons register with a lease and renew it by heartbeat; entries that miss renewal expire.
- **FR-091** Registry entries record the canonical repository root, process ID, endpoint, Resonance version, schema version and start time, so version skew is detectable.
- **FR-092** After a restart, the registry rebuilds its view by scanning known endpoint files and checking which locks are held.
- **FR-093** The registry can run under launchd (macOS) or `systemd --user` (Linux), installed explicitly with `resonance registry install`.
- **FR-094** The registry monitors only; it does not restart daemons.
- **FR-095** Daemons distinguish liveness from readiness, and report their state (`starting`, `reconciling`, `ready`, `git-operation`, `degraded`), ingest lag, pending event count, last successful ingest and last error.
- **FR-096 (partial)** `resonance doctor` shows every known daemon with its health and flags version skew. It also checks this repository: the store's integrity and schema version, and the `.git/info/exclude` line. Adoption health (FR-207) arrives with B8.
- **FR-101** The API transport is JSON-RPC 2.0 over the daemon's Unix socket (MVP-019).
- **FR-104** The API supports subscriptions to change streams: `subscribe` sends a notification for every change set the daemon applies.
- **S14** Discovery without the registry, through `daemon.json`; `resonance doctor` shows readiness and lag.
- **Commands:** `daemon [status]`, `daemon start|stop|restart` (and the plumbing `daemon run`), `registry run|install|uninstall [--dry-run] [--yes]`, `doctor`, and the global `--no-daemon` and `--cached` flags.

### 3.16 Git integration  `[git]`

- **FR-067** The git monitor watches `HEAD`, refs, `index.lock`, `MERGE_HEAD` and rebase state, and suspends per-event ingest while a git operation is in progress (MVP-029).
- **FR-068** After a git operation completes, the daemon rebuilds for the new commit: derived data (from the cache where possible, MVP-028), the snapshot and the deltas present at that commit (in offline mode, the local deltas associated with its ancestors, MVP-030), classification of the file changes the operation brought in (§3.17), then uncommitted changes and the open delta. The graph is a function of the commit plus uncommitted changes.
- **FR-026** From stage 2, the graph is committed as a snapshot plus an add-only sequence of small delta files that accumulate on whatever branch the work happens on, compacted into a new snapshot every N deltas on the trunk (MVP-026, MVP-027, MVP-034).
- **FR-062** The derived layer is never committed.
- **FR-063** From stage 2 the authored and identity layers are versioned in git as the snapshot and deltas; in offline mode they are kept locally with the same structure (FR-182).
- **FR-064** File content stays in git as ordinary files. A delta carries only a file manifest for the files whose changes Resonance observed, except that identity-changing edits are recorded as identity operations.
- **FR-065** Authored and identity changes merge without manual resolution in the common case; genuine conflicts become `Conflict` nodes, listed by `status`, `recover` and finding `R006`.
- **FR-066 (partial)** Graph changes in a pull request are reviewable through tooling: the `git-driver textconv` driver and the `resonance_review` summary (MVP-036). Posting the summary from CI is backlog (§13.3).
- **FR-069** Onboarding installs git hooks for every Resonance user (MVP-035): `pre-commit` and `pre-push` check and may block; the `post-*` hooks notify the daemon. Offline trial users get only the non-blocking `post-*` hooks. The git monitor alone remains sufficient for correctness.
- **FR-070** Each git worktree is a separate repository root with its own daemon, open delta and local state. Worktrees do not share a derived-data cache in the MVP (OQ-08).
- **FR-071 (partial)** Resonance works within an ordinary git workflow (branch, commit, merge, rebase, cherry-pick, revert, stash, pull requests on any host) at stages 0–2, without teammates installing Resonance or changing how they use git. Developing Resonance's own repository this way waits for self-dogfooding (B17).
- **FR-072** The data model is not bent for git beyond the snapshot-plus-deltas layout.
- **FR-073** Committed data is exactly one snapshot and zero or more delta files; the graph at a commit is the snapshot with every delta present at that commit applied in fold order (MVP-027).
- **FR-074** The snapshot holds authored and identity data only, in a canonical, compact form with a header, and is immutable between compactions (MVP-027).
- **FR-075** A delta holds a header, canonical operations and a file manifest (MVP-026).
- **FR-076** Deltas are named by unique, time-sortable IDs, and sealed deltas are never modified, so deltas never conflict in git.
- **FR-077** Each working tree has at most one open delta, rewritten atomically and sealed when a commit contains it; `git add -A && git commit` is enough (MVP-033).
- **FR-078** Deltas accumulate on the branch where the work happens; merges, rebases, cherry-picks and squash merges carry them like any other file, and a delta that arrives twice is harmless (FR-193).
- **FR-079** From stage 2, `.resonance/.gitattributes` (never the repository root's) marks `graph.db` as binary and names the merge and diff drivers, and `.resonance/README.md` tells hold-outs that taking either side of a conflict there is safe.
- **FR-154** Compaction folds the snapshot and sealed deltas into a new snapshot and deletes the folded deltas, as one working-tree change, when the trunk has `compaction.maxDeltas` sealed deltas (default 100) (MVP-034).
- **FR-155** Compaction runs only on the trunk by default, so merging a feature branch never conflicts in `.resonance/`.
- **FR-156** Resonance never creates commits on its own; `resonance_compact` with `commit`, called by an agent at the developer's request, puts a compaction in its own commit.
- **FR-157** Snapshots keep the operations they folded, so colliding compactions are always repaired mechanically, by the merge driver or by `resonance recover`.
- **FR-159** A `git revert` of a delta already folded into the snapshot is noticed, and the inverse operations are offered rather than the reverted change being kept silently.

### 3.17 Divergence and recovery  `[recovery]`

- **FR-190** Every disagreement between the graph and the working tree, the index or the committed `.resonance/` data is classified from git evidence (C1–C9, §8.18) before any action (MVP-031).
- **FR-191** Resolutions follow authority. Offline, git and the file tree are authoritative and the graph follows them, with repair through FR-238. For an opted-in developer the graph is authoritative: their own git commands are instructions the graph follows (C4, C5), and anything else that changed their files is a conflict (C3). Changes from people who do not run Resonance enter through ingest (C1, C2). Content someone committed to git is never silently discarded.
- **FR-192** Automatic action is taken only when it is non-destructive. Whatever a resolution displaces is kept in `local/recovery/`, and every automatic resolution is undoable with `resonance recover undo <id>` (MVP-032).
- **FR-193** Operations are idempotent by `opId`, and file content is content-addressed; recovery re-applies rather than reasoning about what was already applied.
- **FR-194** A local journal records every operation applied or written, independent of the working-tree copy of the open delta (MVP-032).
- **FR-195** After any absence the daemon catches up in order: git to `HEAD` (C1, C2, C7, C8), then classification of the remaining differences (C3–C6, C9), then serving queries. A bulk catch-up produces one summary.
- **FR-196** `resonance recover` lists unresolved divergences with their class and evidence and applies a chosen resolution (take the graph's version, take git's or the disk's, merge three ways, keep both), interactively or from scripts, and through MCP (`resonance_recover`). Agents get the developer's confirmation before anything that cannot be undone.
- **FR-197** From stage 2 the whole graph can be rebuilt from git alone (snapshot and deltas at `HEAD`, plus file contents); only uncommitted graph changes depend on the journal and backups.
- **FR-198** Every classification and its outcome is logged and counted per class, so that C9 cases can be studied.
- **FR-012b** For an opted-in developer, file changes made while the daemon was not running are classified at start-up. Changes a git operation explains are applied as git-originated. A remaining unexplained change is a conflict: the file is restored from the graph through the recovery writer, the disk version is kept in the recovery journal, and the developer, usually through their agent, chooses to merge, keep the disk version, discard it, or do something else. A file whose bytes the graph does not hold (MVP-016) is not restored; its conflict is surfaced with the disk version left in place. Offline, such changes are simply ingested (FR-188).
- **FR-050 (partial)** Recovery restores write files with an atomic replace in the same directory, preserving permissions and line endings (MVP-032). Graph-to-disk code mutations are B10.

### 3.18 Adoption  `[adoption]`

- **FR-170 (partial)** Stages 0 (absent), 1 (offline trial) and 2 (shared) are supported, and a repository can stay at any of them indefinitely. Stage 3 (enforced) is B11.
- **FR-171** `resonance status` shows the stage, the developer's participation, and, for any path asked about, whether Resonance governs it and why.
- **FR-172 (partial)** Every change of stage or participation is an explicit command with a preflight check, a preview (dry run by default for anything that touches tracked files, hooks or git configuration) and an undo hint: `init` (offline, or joining when committed data exists), `init --offline`, `share`, `unshare`, `leave` and `eject` (MVP-037). `enforce` (B11) and quiet setup (B9) are not in the MVP.
- **FR-173** Every change is reversible without loss of authored data, including leaving as an individual and stepping back to stage 0. `resonance eject` exports graph-only data in the engine-neutral format and, with `--markdown`, as Markdown, before removing Resonance's files, hook blocks and exclusions.
- **FR-174 (partial)** Hold-outs work in a stage 2 repository with plain git and without installing Resonance. Taking either side of a conflict in `.resonance/` is safe, because a Resonance user's daemon repairs it, and `.resonance/README.md` says so. Stage 3 bookkeeping for hold-outs is B9 and B11.
- **FR-175 (partial)** Both data paths work at every supported stage: git → graph through ingest of commits and pulls made without Resonance, and graph → git through deltas committed with the code. Projecting graph-originated file content is B10.
- **FR-176 (partial)** Consistency checks compare each commit's changed files with its deltas' manifests. For each Resonance user they run through the agent (FR-215, FR-216) and the `pre-commit` and `pre-push` hooks, and block inconsistent commits and pushes. For the team, `resonance check --range` runs in any CI system and reports which commits came from outside Resonance; at stage 2 it is informational and never blocks non-Resonance developers.
- **FR-177** Existing git hooks and hook managers are never overwritten or disabled: Resonance appends a marked block to shell hooks, or prints instructions (MVP-035).
- **FR-178** `config.toml` may limit what Resonance governs to path globs (`[governance] paths`, resolving OQ-20 for the MVP). Outside them files are still ingested, but nobody is held to consistency (FR-203) and no C3 conflicts are raised.
- **FR-179** No tracked file outside `.resonance/` is modified without explicit confirmation, at any stage: not the root `.gitignore` or `.gitattributes`, and not the shared agent instructions file.
- **FR-203** Nothing done by a developer who has not opted in is ever blocked. For an opted-in developer, commits, merges, rebases and pushes must be consistent with the graph: their agent's checks and git hooks may block an inconsistent operation, with the inconsistency and the fix explained, and a bypass (`--no-verify`) is detected and repaired before the push, or absorbed after it (C8). Saves and builds are never blocked.

### 3.19 The spec TUI and the change log  `[tui]`

- **FR-270** `resonance tui` lists spec items (requirements, decisions, designs and constraints) with filtering by type, status and text; shows an item's details, links, latest evidence and, for an item that came from a document, its source; edits fields; adds items; and removes them. Its changes go through the daemon's API as the person using it (MVP-041).
- **FR-271** Removing an item retires it, after a confirmation that shows its link count.
- **FR-272** Every item is edited in place, since the graph is the source of truth; editing an item that came from a document makes its `derivedFrom` link stale, so that the agent updates the document (FR-124). The TUI writes no project files.
- **FR-273** New items get a suggested next free key for their prefix; an existing key is refused, in the TUI and in the agent's import (whose preview reports it).
- **FR-279** The TUI stays deliberately simple: listing, viewing, editing, adding and retiring spec items, and the change log. Richer interaction is verified in the web UI (MVP-043) on the way to the JetBrains plugin (B14), the intended primary interactive surface.
- **FR-274** Resonance keeps a concise change log of people's changes to spec items, from the web UI or the TUI, and of spec changes that arrive through git. Agent changes are logged only with `changes.includeAgents` (MVP-040).
- **FR-275** An entry has a sequence number, time, actor, item, kind of change, changed fields and a one-line summary; `resonance_changes_show` gives the full before and after. Sequence numbers increase and are never reused, and a newer change to an item with a pending entry replaces it with a new entry, so there is at most one entry per item.
- **FR-276** Agents keep, in their own memory, the sequence number of the last entry they read; read entries after it at the start of work and before git operations; act on each; and remove handled entries with `resonance_changes_done` (optionally with a note). Removing an entry deletes it from the log only; the operations stay in the operation log.
- **FR-277** As a general rule the change log is empty before commits, pushes and pulls. It is an agent duty (MVP-038); `resonance check` reports pending entries (`R009`, *attention*); the agent-side hook stops the agent's own `git commit`, `push` and `pull` while entries are pending unless the developer says to proceed; people's git hooks only warn.
- **FR-278** The change log is local to the working tree and never committed.
- **FR-283** People's edits reach the agent without it looking: the pending entries come with every prompt (the kit's `UserPromptSubmit` hook) and a reminder comes with every MCP result other than the change-log tools, including a failed call, which keeps its error text and then the reminder. Both ask the agent to clear what it has handled (MVP-038, MVP-040).
- **FR-284** Items with a pending entry are flagged in item views, the web UI and the TUI, retired ones included; the flag is derived from the log, never stored (MVP-040).
- **FR-285** People clear entries in the web UI and the TUI, one at a time or all after a confirmation (MVP-041, MVP-043).

### 3.20 The web UI, diagnostics and logs  `[ui]`

- **FR-286** Each daemon serves the web UI on `127.0.0.1` while it runs, on a port that is reused for its working tree when free and assigned by the operating system otherwise (or `ui.port`); the address is in `daemon.json`, the registry and the daemon's health (MVP-043).
- **FR-287** `resonance ui [<key>]` starts the daemon if needed and opens the web UI, or the item, in the default browser; `--no-open` prints the address. Not offered in direct mode.
- **FR-288** No login: a per-working-tree token, handed over by `resonance ui` and kept as a same-site, HTTP-only cookie, plus `Host` and `Origin` checks and no CORS (MVP-043).
- **FR-289** The bundle is built ahead of time from TypeScript and React and embedded in the binary; nothing is loaded from elsewhere at run time.
- **FR-290** The web UI uses the daemon's API and its change stream; an open page keeps the daemon from going idle; on disconnect it reconnects, or says to run `resonance ui`.
- **FR-291** Everything the TUI does (FR-270–FR-273, FR-284, FR-285), attributed to the person, with origin `ui` in the change log (MVP-040).
- **FR-292** An address for every item and view, in-page editing of long text, the TUI's keys, a command palette, and a warning when the item being edited changed underneath (MVP-043).
- **FR-293** A diagnostics view: identity, health, configuration with sources, the activity timeline, counters, clients and a store summary, since the daemon started (MVP-044).
- **FR-294** Diagnostics are kept in a bounded buffer in memory only, hold no item text or file content, and export as JSON.
- **FR-295** Operational actions from the diagnostics view: reconcile (incremental or full), rebuild derived data, restart, stop, and the session's log level, with confirmation where other clients are affected.
- **FR-296** The daemon logs structured records with a level and a component (`tracing`).
- **FR-297** The log file, level, format and rotation size are configurable (`log.*`, `RESONANCE_LOG`, `RESONANCE_LOG_FILE`); the default file is `.resonance/local/daemon.log` (MVP-044).
- **FR-298** The web UI shows the log since the daemon started, live and filtered, from a bounded buffer in memory.

---

## 4. Data layout

`.resonance/` follows MVP-025: the snapshot, deltas, `config.toml` and (from stage 2) `README.md`, `.gitignore` and `.gitattributes` at the top, everything local under `local/`. In offline mode the whole directory is excluded through `.git/info/exclude`.

```
~/.resonance/          # per user, outside every repository
  run/                 # sockets: <hash-of-root>.sock, registry.sock (or $RESONANCE_RUNTIME_DIR, MVP-019)
  known-roots          # repositories that have run a daemon, for the registry and doctor (MVP-022)
  exports/             # eject and leave exports
  backups/             # resonance backup (FR-184)
```

Illustrative tables (an internal detail of the adapter): `nodes`, `edges`, `blobs`, `files`, `anchors`, `ops`, `meta`, plus `deltas` (open, sealed, folded; commit association, MVP-030), `divergences` (MVP-031), `recovery`, `changes` (MVP-040) and `counters` (MVP-039). These follow §8.2, with edge endpoint hashes (MVP-012) and anchors (MVP-013). The snapshot has its own `snapshot_meta` table (MVP-027).

---

## 5. Acceptance

The MVP is done when the following pass. Each is an automated integration test against a temporary git repository unless marked *manual*. Graph work is driven through MCP, as agents do it; people's spec edits through the TUI's test backend and the web UI's HTTP API; setup, operation and repair through the CLI (FR-259).

- **S1 (offline form)** AC1 (no file modified), AC2 (files present, hooks and agent kit installed, no requirements read; nothing parsed), AC3 (builds unaffected), AC4 (deleting `.resonance/` leaves a working project), AC5 / NFR-041 (`git status` unchanged).
- **S2** AC1 and AC2: `resonance_trace` with statuses and evidence; `resonance_unverified`.
- **S3** AC1: `resonance_why`.
- **S4** AC1 (anchored to a code unit), AC2 (survives edits elsewhere in the file, edits inside the region that keep at least half its lines, file renames, and moves of the region to another file; a region rewritten beyond recognition is orphaned and listed, never silently re-attached), AC3 (orphaned with last location when the anchor disappears, and restored when it reappears, MVP-005), AC4 (annotations appear in `resonance_why` and in context packs).
- **S7** AC1 (editing a requirement's text makes its links stale) and AC2 (`resonance_trace` lists everything affected).
- **S8 (adapted)** A file save followed by any request reflects the new text and re-located anchors.
- **S10** AC1: checkouts, pulls, merges, rebases, resets, cherry-picks and reverts are recognized from the reflog and applied as one batch, not as thousands of edits. AC2: the next answer reflects the new commit plus uncommitted changes.
- **S13** AC1: a daemon started after edits were made reconciles before answering any query. AC2: a missing or crashed daemon causes delay, never wrong answers. AC3: differences found at start-up are classified with git evidence and resolved by authority, with one summary. Edits made between commands are always reflected in the next answer, in daemon mode and in direct mode.
- **S14** AC1: a client finds a repository's daemon without the registry. AC2: `resonance doctor` shows every running daemon, whether it is ready, and how far behind the disk it is.
- **Daemon.** One daemon per repository (a second exits); it starts on demand and exits when idle; `daemon stop`, `start` and `restart` control it; a subscription receives change notifications; a request with `--cached` does not wait. The registry registers, renews and expires daemons, rebuilds its view after a restart, and `registry install --dry-run` previews the service definition.
- **Identity (B2).** A file renamed and edited in one step keeps its anchors and its file-level links (FR-036); a region moved into a file that did not change in that reconcile is found (MVP-024); a snippet anchor keeps its exact columns through edits on its own line; `resonance_split_unit` and `resonance_merge_unit` move links and annotations and record identity operations.
- **S15** AC1–AC7, including the repair list after a teammate's rename (AC5) and `resonance_value` (AC6).
- **Branch-aware offline deltas (B3).** An annotation made on a branch is absent on `main` and back on switching to the branch; after `rebase` and `commit --amend` its delta is re-associated by patch ID; a delta whose commit was dropped lands in the detached pool and is listed; `backup` writes outside the repository, and deleting `.resonance/` makes the next command offer a restore.
- **S18 / recovery (B3).** Scripted scenarios produce classes C1–C8 with the expected resolution and level, in offline mode and opted in; every automatic resolution is undone exactly by `recover undo`; `recover` works from the CLI and through MCP; graph changes survive `git stash`, `git clean -fdx` and a deleted `.resonance/` through the journal, backups or git.
- **Intent breadth (B4).** Designs, constraints, `supersede`, `derivedFrom` and `realizes` work end to end through MCP; an unknown extension type survives export, restore, a delta and a snapshot.
- **S26 / agent import.** `resonance_import_items` with `dryRun` returns the plan and writes nothing; a plan with an invalid item or a key clash applies nothing; applying creates the items, their `spec` units and `derivedFrom` links; calling again with a changed item updates it and leaves the rest `unchanged`; editing the document region, or the item, makes the link stale. `init` reads no documents, and the binary has no `import` command.
- **S5 and S6.** Scripted MCP sessions: an agent fetches a context pack, declares code units and tests, links them with agent provenance (`asserted`) and verifies them; another finds the requirements and tests related to a bug through search and `why`.
- **Fold and storage (B5).** Two clones folding the same operations produce byte-identical snapshots; `rebuild` reproduces the same authored state; applying a delta twice is harmless; export and `restore` round-trip; the in-memory adapter passes the conformance suite (S21 AC1); a migration compaction across a snapshot format version leaves deltas and history untouched (S21 AC2); a repository in the earlier layout is migrated (MVP-025).
- **Shared graph (B6).** S16: `share` previews, keeps local-only items local, and leaves only new files under `.resonance/` to commit. S11: two clones on different branches change the graph and merge with no conflict in `.resonance/`, and a genuine semantic conflict becomes a `Conflict` node. S12: a fresh clone rebuilds a complete graph from git. S19: trunk compacts while a branch is in flight, and the branch still merges cleanly; colliding compactions are repaired by the merge driver and, without it, by `recover`; a revert of a folded delta offers inverse operations. S17: a hold-out's commits are ingested with their author and make links stale, and taking either side of a `.resonance/` conflict is repaired. S24 and FR-012b: `pre-commit` blocks an inconsistent commit with its fix; a `--no-verify` commit is caught by `pre-push` and repaired; an edit made while the daemon was down becomes a conflict, the file is restored from the graph, and `recover undo` brings the edit back. S20 (stages 0–2): `init`, `share`, `unshare`, `leave`, rejoining and `eject` lose no authored data; `share` adds the instructions section to the shared `CLAUDE.md` as a change to commit. Hooks are appended to an existing shell hook and removed cleanly, and a detected hook manager gets instructions instead.
- **Agent kit (B7, S22).** `init` installs the MCP server, the skill, the harness hooks and the instructions section for the stage, and running it again refreshes them; the agent hook denies `git commit` when `check --staged` fails or change-log entries are pending, and allows it with the developer's override; `resonance_conventions` differs by stage and participation; `status` and `doctor` report a missing or stale kit; the orientation is available through MCP.
- **S25 / TUI and change log.** Driven through the test backend: listing, filtering, viewing, editing a graph-native item, adding one with a suggested key and refusing a duplicate key, retiring with confirmation, marking items with pending changes, and clearing one entry or all of them; editing an item that came from a document shows its source and makes the `derivedFrom` link stale. Change-log entries appear for people's changes from the TUI and for spec changes arriving with a pull, not for MCP or `RESONANCE_ACTOR` agent changes by default; a second edit to the same item replaces its pending entry with a new sequence number; `resonance_changes` with `since` and `resonance_changes_done` behave as specified, the agent hook holds back `git commit` while entries are pending; the prompt hook adds pending entries to the agent's context and is silent when there are none; MCP results carry the reminder while entries are pending; and nothing in the log reaches git.
- **S27 / web UI, diagnostics and logs.** Through the HTTP API: two daemons serve their UIs on different ports, and a restarted daemon takes its previous port; `resonance ui --no-open --json` starts the daemon and returns the address; a request without the token, with another `Host`, or a `POST` with another `Origin` is refused, and the `/auth` exchange sets the cookie and redirects to a local path only; the S25 spec and change-log operations work through `/api/call`, with origin `ui` in the change log; a method outside the allowlist is refused; the event stream delivers a change made over MCP, resumes after a reconnect, and keeps an idle daemon running; `diagnostics` reports requests, a reconcile and a git operation, with counters, clients and dropped events when the buffer is small, and with no item text in an export; `log_level` raises the level of the buffer and the file; `log.file` and `RESONANCE_LOG_FILE` are honored, rotation keeps one previous file, and a file in the working tree outside `.resonance/local/` is refused; `reconcile` and `daemon_restart` from the UI work, and the page's next request reaches the successor. The frontend's component tests pass (`just ui-check`). *Manual:* `resonance ui` opens the default browser on macOS and Linux.
- **Verification.** `resonance_test` marks passing `verifies` links `verified`, records failing evidence, and records `error` when the command cannot start or zero tests ran.
- **Anchor tracking.** A table-driven test suite of edit scenarios (insert above, edit inside, delete half, reformat, move to another file, duplicate snippet, delete entirely, restore) checks every MVP-013 step and its recorded operations.
- **Store boundary.** The conformance suite passes for `SqliteStore`, and `cargo tree -p resonance-core` shows no `rusqlite` and no source-code parser.
- **CLI surface.** `resonance --help` lists only the commands of FR-259.
- **MCP.** A scripted stdio session (initialize → tools/list → tools/call `resonance_trace`) succeeds.
- **Field test (manual).** On small and purpose-built test repositories, never on Resonance's own: the typical session in §1 works end to end, and edge cases (mixed languages, renames and moves, large and binary files, nested ignore rules, branch switches) behave as this file specifies. Stage 2 is field-tested with two clones of a test repository sharing a bare remote, one opted in and one a hold-out. `just fixture-repo` creates a fresh test repository from the starter fixtures. Running Resonance on its own repository is backlog item *Self-dogfood* (B17), done once the MVP is judged reliable.
