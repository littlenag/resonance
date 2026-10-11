# Resonance MVP — Backlog

These are the items from the full specification (`spec/section-*.md`) that are **not yet** in the working spec, [`mvp.md`](mvp.md). The full text of each item is in the full spec under the same ID. The lines here are pointers with a one-line summary.

**Moving an item into the MVP**

1. Cut the whole bullet from here and paste it under the heading in `mvp.md` named by its area tag (for example `[storage]` → "3.5 Storage and the store API"). If no such heading exists yet (for example `[daemon]`), add one to §3 of `mvp.md`.
2. Expand the one-line summary into the requirement text, narrowed for the MVP if needed. Copy from the full spec rather than paraphrasing.
3. If only part of the item moves, leave the rest here as an "(remainder)" line under the same ID.
4. Add or extend acceptance criteria in `mvp.md` §5 for any scenario the move unlocks.

Items are grouped into **suggested increments**, in a sensible build order. Each increment is roughly one implementation session. The order is a recommendation, not a dependency proof; where an item depends on another increment, it says so.

---

## B1 — Persistent daemon and watcher
*Replaces MVP-002's reconcile-on-demand with a background owner. This is a prerequisite for low-latency queries on large repositories and for B10.*

- **FR-040** [daemon] Each repository has a persistent watcher process (§8.4).
- **FR-041** [daemon] Native file events: FSEvents on macOS, inotify/fanotify on Linux.
- **FR-042** [daemon] Tolerate batched, coalesced, delayed and out-of-order events; re-examine paths by hash.
- **FR-043** [daemon] Maintain an ingest watermark.
- **FR-044** [daemon] Suppress events caused by the daemon's own writes (path, hash, generation).
- **FR-047** (remainder) [daemon] Startup reconcile inside the daemon, with an optional FSEvents replay-since-event-ID optimization, falling back to a full scan.
- **FR-048** [daemon] Hand bursts that match a git operation to the git monitor instead of treating them as individual edits.
- **FR-080** [daemon] Watcher, ingest, projection writer, git monitor and API under one owner (P1).
- **FR-081** [daemon] Queries served by the same owner; optional engine-specific read-only file access (FR-168).
- **FR-082** [daemon] An exclusive lock on `.resonance/daemon.lock` guarantees a single daemon.
- **FR-083** [daemon] `daemon.json` holds the endpoint, pid, versions and start time.
- **FR-084** [daemon] Clients start the daemon on demand; it exits after an idle period.
- **FR-085** [daemon] The per-repository component can run in its own process or hosted in a shared process.
- **FR-086** [daemon] Unix domain sockets in a short per-user runtime directory.
- **FR-087** [daemon] If TCP is ever used: loopback only, plus a per-daemon token.
- **FR-088** [daemon] A system-wide registry for discovery and health only.
- **FR-089** [daemon] The registry is never on the request path (P5).
- **FR-090** [daemon] Registry leases renewed by heartbeat.
- **FR-091** [daemon] Registry entries record the canonical root, pid, endpoint, versions and start time.
- **FR-092** [daemon] The registry rebuilds its view after a restart from endpoint files and locks.
- **FR-093** [daemon] The registry runs under launchd or `systemd --user`.
- **FR-094** [daemon] The registry monitors only; it does not supervise daemons (OQ-13).
- **FR-095** [daemon] Separate liveness and readiness; report state, lag, pending events and the last error.
- **FR-096** [daemon] `resonance doctor` lists all daemons, their health and any version skew; adoption health via FR-207.
- **FR-101** [daemon] API transport: JSON-RPC (or gRPC) over the daemon socket (OQ-04).
- **FR-102** (remainder) [daemon] Read-your-writes by waiting on the watermark; a `consistency: cached` option.
- **FR-104** [daemon] Subscriptions to change streams.
- **S14** [daemon] Discovery without the registry; `doctor` shows readiness and lag.

## B2 — Stronger identity reconciliation
- **FR-031** [identity] Every clone agrees on IDs that authored data refers to, by committing code-unit creation and re-anchoring operations (with B6).
- **FR-032** (remainder) [identity] Smarter textual re-anchoring: token- or character-level diffs, better approximate matching, searching unchanged files for moved regions. Still no parsing (NG4).
- **FR-033** (remainder) [identity] Splits and merges declared through the API and recorded as identity operations; operations committed so that clones agree (with B6).
- **FR-036** [identity] Real file moves detected by content similarity and recorded.
- **§9.4** (remainder) [identity] `identity.split` and `identity.merge`, and producing a `Conflict` for irreconcilable concurrent identity operations.

## B3 — Git-aware offline mode and recovery
*Makes offline mode follow branches and explain divergence. Builds on B1's git monitor if B1 is done first; otherwise it runs on demand.*

- **FR-067** [git] Git monitor: watch HEAD, refs, `index.lock`, `MERGE_HEAD` and rebase state; suspend ingest during a git operation.
- **FR-068** [git] After a git operation, rebuild for the new commit: derived layer, deltas, then uncommitted changes.
- **FR-183** [offline] Branch-aware offline deltas associated with commits; re-associate them after rebases via the reflog and patch IDs; keep a detached pool (resolves OQ-18).
- **FR-184** (remainder) [offline] `resonance backup`; detect a missing `.resonance/` and offer to restore it.
- **FR-238** (remainder) [offline] A repair list triggered by git operations that bring in outside changes; agent-driven repair recorded as identity operations.
- **FR-239** [offline] Lazy, budgeted repair; visible repair cost.
- **FR-240** [offline] `resonance value`: a local report of the value Resonance delivered, plus an upgrade path.
- **FR-190** [recovery] Classify every divergence from git evidence before acting (C1–C9, §8.18).
- **FR-191** [recovery] Resolutions follow authority (offline: git and the file tree; opted in: the graph).
- **FR-192** [recovery] Automatic actions only when non-destructive; a recovery journal; `resonance recover undo <id>`.
- **FR-193** [recovery] Operations idempotent by `opId`; re-apply when in doubt.
- **FR-194** [recovery] A local journal of every operation, independent of the working tree.
- **FR-195** [recovery] Catch-up order: git to HEAD, then classify, then serve; one summary.
- **FR-196** [recovery] `resonance recover`, interactive and scriptable, also exposed over MCP.
- **FR-198** [recovery] Log and count classifications; study C9 cases.
- **S10** [git] Branch switch, rebase and pull recognized as git operations, not thousands of edits.
- **S13** (remainder) [recovery] AC3: differences classified with git evidence, one summary.
- **S18** [recovery] Git and the graph disagree: classify, act non-destructively, explain.

## B4 — Breadth of intent and ontology
- **§6.2** (remainder) [model] Node types `Design`, `Constraint` and `Conflict`; requirement `kind`, `priority`, and statuses `draft`/`superseded`.
- **§6.3** (remainder) [model] Edge types `derivedFrom`, `supersedes` and `realizes`. (`references` lives only in a parallel code-structure graph, B13.)
- **§6.4** [model] Stable identity anchored on intent; code units as the current realization.
- **FR-007** [model] Namespaced extension types (`acme:Service`) that do not change core meaning.
- **FR-008** [model] Unknown extension types preserved on read and write.
- **FR-120** (remainder) [intent] Designs and constraints; supersede.
- **FR-121** [intent] Opt-in projection of intent documents as Markdown with stable IDs (OQ-12).
- **FR-122** (remainder) [intent] Importers for GitHub Spec Kit, Kiro, EARS-style statements and ADR directories.
- **FR-123** (remainder) [intent] Tagged source comments (`// @satisfies FR-042`) ingested as edges; disabled in offline mode (FR-189); OQ-10.
- **S5** [intent] An agent implements a feature from a requirement, end to end, with context pack, links and provenance.
- **S6** [intent] An agent fixes a bug, finding related requirements and tests.

## B5 — Storage maturity
- **FR-024** [storage] Derived-data cache keyed by the content hash of inputs.
- **FR-060** [storage] Internal representation optimized for access patterns; need not be textual.
- **FR-061** [storage] Every change expressible in the edit algebra; git-carriable (§9).
- **FR-150** [storage] The authored graph is a deterministic fold over its operations (`resonance rebuild` from the ops log).
- **FR-151** [storage] Fold order: commit topology, then `hlc`, then `opId`.
- **FR-152** [storage] Deterministic compaction.
- **FR-153** [storage] Canonical serialization of committed operations (OQ-02).
- **FR-162** (remainder) [storage] Change feed, snapshot export/import, delta application.
- **FR-164** [storage] Engine-neutral delta format.
- **FR-165** [storage] Snapshot format identifier; migration compaction between engines.
- **FR-166** (remainder) [storage] Engine-neutral import (restore from export).
- **FR-167** (remainder) [storage] A second, in-memory adapter that also passes the conformance suite.
- **FR-169** [storage] The committed snapshot is never the live store.
- **S21** [storage] Replace the storage engine with no code changes outside the adapter.
- **OQ-03 / G11** [storage] Graph engine research track.

## B6 — Sharing the graph (stage 2) and opting in
*The big one: committed snapshot plus deltas, joining, and consistency obligations for opted-in developers. Depends on B5 (fold, canonical serialization) and, ideally, B3.*

- **FR-010** (remainder) [truth] For opted-in developers the graph is authoritative for file bytes; large and binary blob policy (MVP-016) revisited.
- **FR-012b** [truth] For opted-in developers, unexplained changes made while Resonance was not running become conflicts: restore from the graph, preserve the disk version, ask.
- **FR-020** (remainder) [storage] The store is committed alongside regular files (stage 2+).
- **FR-025** (remainder) [storage] `.resonance/.gitignore` for local files at stage 2+.
- **FR-026** [git] Committed snapshot plus add-only deltas, compacted every N.
- **FR-062** [git] The derived layer is never committed.
- **FR-063** [git] Authored and identity layers versioned in git from stage 2.
- **FR-064** [git] File content carried by git; deltas carry file manifests only.
- **FR-065** [git] Authored changes merge without manual resolution; real conflicts become `Conflict` nodes.
- **FR-066** [git] PR reviewability through tooling: textconv, `resonance review`, a CI comment.
- **FR-069** [git] Git hooks for opted-in developers (`pre-commit`, `pre-push`, `post-*`), chained.
- **FR-070** [git] Each worktree is its own root with its own open delta (OQ-08).
- **FR-071** [git] Works within ordinary git workflows at every stage; dogfooded.
- **FR-072** [git] The data model is not bent for git beyond snapshot plus deltas.
- **FR-073** [git] Committed data is one snapshot plus delta files; graph = snapshot + deltas in fold order.
- **FR-074** [git] Snapshot contents, canonical form and header.
- **FR-075** [git] Delta contents: header, operations, file manifest (OQ-02).
- **FR-076** [git] Unique, time-sortable delta names; sealed deltas are immutable.
- **FR-077** [git] The open-delta lifecycle; sealing on commit.
- **FR-078** [git] Deltas accumulate per branch; merge, rebase and cherry-pick are safe.
- **FR-079** [git] `.resonance/.gitattributes` and `README.md` for hold-outs.
- **FR-154** [git] Compaction every N deltas on trunk (OQ-17).
- **FR-155** [git] Trunk-only compaction by default.
- **FR-156** [git] Resonance never creates commits on its own; `compact --commit`.
- **FR-157** [git] Snapshots keep folded operations; mechanical repair of colliding compactions.
- **FR-158** [git] Pruning of folded operations (OQ-16).
- **FR-159** [git] Detect reverts of compacted deltas and offer inverse operations.
- **FR-170** [adoption] Every stage supported indefinitely.
- **FR-171** [adoption] `status` shows the stage, participation and governed paths.
- **FR-172** [adoption] Explicit commands: `share`, `unshare`, joining via `init`, `enforce`, `leave`, `eject`.
- **FR-173** (remainder) [adoption] Reversibility for `leave`, `unshare` and stage changes; Markdown export option.
- **FR-174** [adoption] Hold-outs work with plain git; taking either side of a `.resonance/` conflict is safe.
- **FR-175** [adoption] Both data paths at every stage.
- **FR-176** (stage 2 part) [adoption] Consistency checks: blocking for opted-in developers; CI reports at stage 2.
- **FR-177** [adoption] Never overwrite existing hooks or hook managers.
- **FR-178** [adoption] Path-scoped governance (OQ-20).
- **FR-179** [adoption] No tracked file outside `.resonance/` modified without confirmation.
- **FR-182** [offline] Offline uses the same snapshot-and-delta structure as committed stages, so sharing is a move (MVP-010's operation log is the seam).
- **FR-185** [offline] `resonance share`: privacy review, fresh snapshot, exclusion moved.
- **FR-186** [offline] Joining merges the offline graph in as a delta with no duplicates.
- **FR-187** [offline] Offline in a shared repository: read the committed graph, never write back, private until consent.
- **FR-197** [recovery] Rebuild the whole graph from git alone (stage 2+).
- **FR-203** [adoption] Opted-in developers' operations must be consistent with the graph; others are never blocked.
- **NFR-012** [nfr] Easy review of committed graph data; a local-only annotation class.
- **NFR-042** [nfr] At most one new `graph.db` blob per compaction.
- **S11** [git] Merge branches that both changed the graph.
- **S12** [git] A fresh clone rebuilds a complete graph from git.
- **S16** [offline] Share the graph (offline to stage 2).
- **S17** [adoption] A hold-out teammate.
- **S19** [git] Compaction with branches in flight.
- **S20** [adoption] Step up and step back down.
- **S24** [adoption] An opted-in developer's bypass is caught before push and repaired.

## B7 — Agent stewardship
*FR-215 and FR-216 assume committed deltas (B6); FR-213 and the offline variant can come earlier.*

- **FR-130** (remainder) [agent] Stewardship tools over MCP: conventions, checks, review, recovery.
- **FR-132** (remainder) [agent] A section in the shared instructions file from stage 2, with confirmation.
- **FR-210** (remainder) [agent] Onboarding incomplete without the kit; `status` and `doctor` report un-onboarded or stale kits.
- **FR-211** (remainder) [agent] Full kit contents: workflows and prohibitions.
- **FR-212** (remainder) [agent] The static part (instructions-file section or skill), plus a `conventions` tool.
- **FR-213** [agent] Placement by stage; offline variant; text safe for all participation types.
- **FR-214** [agent] Harness support, kit versioning, `resonance agent update` (OQ-21).
- **FR-215** [agent] Duties before commit (`resonance check --staged`).
- **FR-216** [agent] Duties before a PR (`resonance check --range`, review summary).
- **FR-217** [agent] Duties during pulls, merges and rebases.
- **FR-218** [agent] Prohibitions (no `--no-verify`, no writes to `.resonance/`, and so on).
- **FR-220** [agent] Communication and declining with alternatives.
- **FR-221** [agent] Every convention backed by a deterministic check.
- **FR-222** [agent] Agent-side harness hooks gating git commands.
- **FR-223** [agent] Agents are the first line; hooks, CI and recovery back them up.
- **FR-224** [agent] Measure kit effectiveness.
- **FR-225** [agent] An orientation at the end of onboarding.
- **NFR-046** [nfr] Over 95% of agent commits pass checks on the first attempt.
- **S22** [agent] Onboard the agent and let it steward a change.

## B8 — Guardrails and user communication
- **FR-013** (remainder) [guardrails] Parse git configuration; `doctor` reverts unparseable core files (git configs and similar) to known-good versions.
- **FR-200** [guardrails] Every message says what, why (with git evidence), what was done, where things were preserved, how to undo, and what next.
- **FR-201** [guardrails] Info, attention and action-required levels.
- **FR-202** [guardrails] Aggregated messages.
- **FR-204** [guardrails] Preview and confirm bulk or hard-to-reverse actions.
- **FR-205** [guardrails] Plain language, git terms, no bare IDs.
- **FR-206** [guardrails] The same messages on every surface (CLI, status, doctor, hooks, MCP, IDE).
- **FR-207** [guardrails] `doctor` checks adoption health.
- **NFR-040** [nfr] No automatic action loses data.

## B9 — Quiet onboarding
*Depends on B6.*

- **FR-230** [quiet] A quiet onboarding path for developers who have not opted in; never required.
- **FR-231** [quiet] A locally fetched, pinned and verified CLI through a bootstrap script; no install, no daemon.
- **FR-232** [quiet] Local opt-in (`resonance-quiet setup`), a quiet kit, no blocking hooks.
- **FR-233** [quiet] Bookkeeping: per-commit deltas, conflict resolution, PR summaries, read-only annotations.
- **FR-234** [quiet] Limits on what the quiet CLI may write or do.
- **FR-235** [quiet] Fail-safe; quiet deltas verified by Resonance users; limited content (OQ-22).
- **FR-236** [quiet] A CI bot adds bookkeeping to non-Resonance PRs at stage 3.
- **FR-237** [quiet] Upgrade to Resonance user, or drop back to hold-out.
- **S23** [quiet] Quiet onboarding.

## B10 — Graph-to-disk mutations
*Depends on B1 (self-write suppression).*

- **FR-050** [projection] Atomic replace that preserves permissions and line endings.
- **FR-051** [projection] Writes produce ordinary file-system events.
- **FR-052** [projection] Check the base version before writing; re-ingest on mismatch.
- **FR-053** [projection] Disk edits take precedence; a conflicting mutation is rejected, never overwrites.
- **FR-054** [projection] Multi-file mutations written as a batch.
- **FR-131** (remainder) [agent] API code mutations as an alternative to direct file edits.
- **NFR-002** [nfr] API changes appear to unaware tools as ordinary file writes.
- **S9** [projection] Mutation through the graph API; IDEs and builds notice; no re-ingest of own writes.

## B11 — Enforcement (stage 3)
*Depends on B6, B7 and B9.*

- **FR-176** (stage 3 part) [adoption] Stage 3 CI rejection, an exception path, absorbing what slips through (C8).
- **Roadmap Phase 7** [adoption] `resonance enforce`, team-wide adoption-health reporting.

## B12 — Verification and impact
- **FR-135** (remainder) [verification] Test frameworks beyond `cargo test`; per-language selectors.
- **§6.3** (remainder) [verification] Indirect checking of `satisfies` through tests that verify both endpoints.
- **FR-105** (remainder) [query] A general query language (OQ-05).
- **Impact operations** [query] What is affected by changing a requirement, decision or code unit (§8.8 "Impact").
- **OQ-09** [verification] Commit evidence summaries vs keep them local.

## B13 — Parallel graphs and a code-structure graph
*Parallel graphs are never part of the primary graph (§6.7, P10). The facility (FR-115–FR-119) can come before any particular producer, and disconnected graphs (FR-242–FR-244) are the cheapest first step: load an existing tool's output, link later. A code-structure producer such as Mache is not yet committed to; integrate one only if Phase 0 (B17) justifies it. Write-back (FR-241) is gated on a tool that does not exist yet.*

- **FR-115** [parallel] Primary graph versus named parallel graphs; every result labeled with its graph.
- **FR-116** [parallel] Operations on parallel graphs never modify the primary graph; enforced by per-graph write capabilities (MVP-018 is the seam).
- **FR-117** [parallel] Cross-graph links both ways; orphaned versus unresolved; stable parallel IDs.
- **FR-118** [parallel] Queries traverse across graphs; graph labels and freshness on every result.
- **FR-119** [parallel] Producer and derivation record; derived, never committed, cacheable (OQ-23).
- **FR-112** [parallel] A code-structure parallel graph (Mache or compiler-grade data).
- **FR-113** [parallel] Investment in structural queries justified by Phase 0.
- **OQ-06** [parallel] Which languages a code-structure graph would cover.
- **FR-242** [parallel] Disconnected graphs: loaded stand-alone, modeled as parallel graphs.
- **FR-243** [parallel] A published import format; `resonance graph load|list|drop`; producer and source recorded; local only (decided).
- **FR-244** [parallel] Connecting a disconnected graph: authored links, proposed matches, or the tool climbing the ladder.
- **FR-241** [parallel] *Gated.* Write-back only when parallel nodes carry enough information for exact edits of primary nodes (OQ-24).

## B14 — IDE
- **FR-140** [ide] Works fully without an IDE plugin (already true; kept as a constraint).
- **FR-141** [ide] An IntelliJ plugin first.
- **FR-142** [ide] Gutter markers, navigation, an annotation UI, stale and orphan warnings, unverified requirements.

## B15 — Regeneration workflows (roadmap Phase 6)
- **S5 / S7 end to end** [model] A requirement change drives agent regeneration; `realizes` and `satisfies` links recomputed and re-verified.

## B16 — Non-functional targets and operations
- **NFR-020** [nfr] Structured logs per daemon.
- **NFR-021** [nfr] Metrics: lag, events, reconciliation, orphans, stale links, latency, divergences, deltas.
- **NFR-022** [nfr] `doctor` health summary for one repository or all.
- **NFR-030** [nfr] Single-file ingest p95 under 200 ms.
- **NFR-031** [nfr] Startup reconcile of 50k files under 30 s.
- **NFR-032** [nfr] One-hop query p95 under 50 ms.
- **NFR-033** [nfr] `trace` / `why` p95 under 200 ms.
- **NFR-034** [nfr] Reference scale: about 1M LOC and 50k files.
- **NFR-035** [nfr] Daemon memory measured.
- **NFR-039** [nfr] Any git host; no host-specific features.
- **NFR-043** [nfr] Catch-up after a 1,000-file pull under 30 s.
- **NFR-044** [nfr] Snapshot size measured.
- **NFR-045** [nfr] Under 1% of divergences unexplained.
- **OQ-07** [nfr] Windows support.
- **OQ-14** [nfr] A shared multi-user graph service, or git-only collaboration.
- **OQ-15** [nfr] Licence.

## B18 — The binary's remaining modes, and packaging
*Modes arrive with the increments that need them (the daemon and registry with B1, plumbing with B6 and B7, quiet mode with B9). Release packaging is needed before the first users outside the project.*

- **FR-250** (remainder) [binary] Daemon, registry, quiet and plumbing modes in the same binary (with B1, B6, B7, B9).
- **FR-251** (remainder) [binary] One implementation of every operation across all modes, including the daemon.
- **FR-252** (remainder) [binary] CLI and MCP become clients of the daemon, starting it on demand (B1).
- **FR-253** (remainder) [binary] Exit codes `3` and `4`; user-level configuration; `--dry-run` everywhere.
- **FR-254** (remainder) [binary] Versioned JSON output schemas; semantic-versioning policy for the command surface.
- **FR-255** (remainder) [binary] Embedded hook shims, `.resonance/` templates, service units, quiet bootstrap and import-format schema.
- **FR-256** (remainder) [binary] Delta, snapshot and import-format versions in `version --json`.
- **FR-257** [binary] Quiet mode as a restricted capability set of the same binary (with B9).
- **FR-260** (remainder) [packaging] Release targets (macOS arm64/x86_64, Linux x86_64/aarch64, musl static).
- **FR-261** (remainder) [packaging] GitHub Releases, crates.io, Homebrew tap; later an official formula, distribution packages, a CI container image.
- **FR-262** [packaging] Checksums and signatures; reproducible release builds in CI.
- **FR-263** (remainder) [packaging] Registry service install and uninstall; `uninstall --user`.
- **FR-264** [packaging] Semantic versioning; minimum binary version recorded in committed configuration (stage 2+); exact pin for quiet mode.
- **FR-265** [packaging] Upgrades through the install channel; daemons restart on new versions.
- **FR-266** [packaging] `resonance uninstall --user`.
- **§18.3** [packaging] Later deliverables outside the binary: CI wrappers, IntelliJ plugin, agent kit as a harness plugin, graph adapters.

## B17 — Evaluation (Phase 0, §11)
*Not product code. The MVP can serve as arm C's intent server.*

- **§11.1–§11.6** [eval] Hypotheses H1 and H2, tasks, arms A–D, metrics, protocol, decision gates.

## Long term
- **G15** [model] An alternative to files: languages may store their source in Resonance in another format; Resonance stores and renders it, the language understands it (§2.5, §8.1 rationale).
- **§2.7** [model] Resonance's own change control, mimicking the git CLI; captured collaboration context (tickets, reviews); runtime feedback packets.
- **OQ-19** [git] Files leave git (stage 4).
