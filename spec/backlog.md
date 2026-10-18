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
*Moved into the MVP (mvp.md, "3.15 Daemon, watcher and registry"). What remains depends on later increments.*

- **FR-044** (remainder) [daemon] Record the path, hash and generation of the projection writer's writes too, and ignore the matching events (with B10). Recovery restores and intent projection already do this (MVP-032).
- **FR-080** (remainder) [daemon] The projection writer (B10) runs under the daemon's single owner.
- **FR-081** (remainder) [daemon] Optional engine-specific read-only access to the store file for tools that accept stale answers (FR-168).
- **FR-096** (remainder) [daemon] `doctor` checks adoption health (FR-207, with B8).

## B2 — Stronger identity reconciliation
*Moved into the MVP (mvp.md, "3.4 Identity" and MVP-024), including the parts that needed committed operations (with B6).*

## B3 — Git-aware offline mode and recovery
*Moved into the MVP (mvp.md, "3.11 Offline mode and lifecycle", "3.16 Git integration", "3.17 Divergence and recovery"; MVP-028–MVP-032).*

## B4 — Breadth of intent and ontology
*Moved into the MVP (mvp.md, "3.1 Model and link lifecycle" and "3.6 Intent").*

- **FR-121** (remainder) [intent] Continuous, opt-in projection of intent documents by Resonance, kept in step by the projection writer (with B10). In the MVP projection is an agent workflow (OQ-12).

## B5 — Storage maturity
*Moved into the MVP (mvp.md, "3.5 Storage and the store API"; MVP-026–MVP-028). What remains is research.*

- **OQ-03 / G11** [storage] Graph engine research track: candidates judged against the conformance suite and NFR-030–NFR-035; adopting one is a new adapter plus a migration compaction (FR-165).

## B6 — Sharing the graph (stage 2) and opting in
*Moved into the MVP (mvp.md, "3.16 Git integration", "3.17 Divergence and recovery", "3.18 Adoption"; MVP-033–MVP-037). What remains needs later increments or is still open.*

- **FR-010** (remainder) [truth] Hold the bytes of large and binary files for opted-in developers, so that they can be restored from the graph too (MVP-016 revisited).
- **FR-050** (remainder) [projection] The projection writer's atomic writes for graph-to-disk code mutations (with B10).
- **FR-066** (remainder) [git] An optional CI step that posts the review summary (`resonance_review`) on the pull request (with the CI wrappers of §13.3).
- **FR-071** (remainder) [git] Resonance's own repository developed this way, adopting Resonance through the same stages (with B17's self-dogfood).
- **FR-158** [git] Pruning of folded operations from snapshots (OQ-16). The MVP never prunes.
- **FR-170** (remainder) [adoption] Stage 3 (with B11).
- **FR-172** (remainder) [adoption] `resonance enforce on|off` (B11) and quiet setup (B9).
- **FR-174** (remainder) [adoption] At stage 3, bookkeeping for hold-outs' pull requests, by their quiet agent or a CI bot (with B9 and B11).
- **FR-175** (remainder) [adoption] The graph → git path for graph-originated file content: projecting files to the working tree (with B10).

## B7 — Agent stewardship
*Moved into the MVP (mvp.md, "3.10 Agent integration"; MVP-038, MVP-039).*

- **FR-214** (remainder) [agent] First-class kits for harnesses beyond Claude Code (OQ-21).
- **FR-224** (remainder) [agent] Aggregated reporting of the kit's effectiveness, with the metrics of B16 (NFR-021).
- **NFR-046** (remainder) [nfr] The target: over 95% of agent commits pass checks on the first attempt, as an acceptance gate.

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

- **FR-176** (remainder) [adoption] Stage 3 CI rejection, an exception path, absorbing what slips through (C8).
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
*JetBrains is the intended primary interactive surface for Resonance (FR-141, decided); the TUI stays deliberately simple until then.*

- **FR-140** [ide] Works fully without an IDE plugin (already true; kept as a constraint).
- **FR-141** [ide] A JetBrains (IntelliJ platform) plugin first; the intended primary interactive surface.
- **FR-142** [ide] Gutter markers, navigation, an annotation UI, stale and orphan warnings, unverified requirements, spec-item editing and the change log, and architectural summaries (FR-280).

## B19 — TUI iteration 2: architectural summaries
*Builds on the MVP's TUI (mvp.md, "3.19"). Naive linking is the starting point; agent-written summaries make the code behind a spec item understandable at an architectural level.*

- **FR-280** [tui] For the selected spec item, present an architectural-level summary of the code linked to it.
- **FR-281** [tui] Realization summaries are `Annotation`s of kind `summary` on the spec item, recording the code and hashes they describe, and go stale when either changes.
- **FR-282** [tui] The agent kit makes writing and refreshing summaries part of implementing a spec item, and agents can be asked for one.

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
- **OQ-15** [nfr] License.

## B18 — The binary's remaining modes, and packaging
*Modes arrive with the increments that need them (the daemon and registry arrived with B1, the TUI and plumbing with the move of B3–B7; quiet mode comes with B9). Release packaging is needed before the first users outside the project.*

- **FR-250** (remainder) [binary] Quiet mode in the same binary (with B9).
- **FR-253** (remainder) [binary] User-level configuration (`~/.config/resonance/config.toml`) in the precedence order.
- **FR-254** (remainder) [binary] Versioned JSON output schemas; semantic-versioning policy for the command surface.
- **FR-255** (remainder) [binary] Embedded quiet bootstrap and import-format schema.
- **FR-256** (remainder) [binary] The parallel-graph import-format version in `version --json` (with B13).
- **FR-257** [binary] Quiet mode as a restricted capability set of the same binary (with B9).
- **FR-260** (remainder) [packaging] Release targets (macOS arm64/x86_64, Linux x86_64/aarch64, musl static).
- **FR-261** (remainder) [packaging] GitHub Releases, crates.io, Homebrew tap; later an official formula, distribution packages, a CI container image.
- **FR-262** [packaging] Checksums and signatures; reproducible release builds in CI.
- **FR-263** (remainder) [packaging] `uninstall --user` (FR-266).
- **FR-264** (remainder) [packaging] Exact version pin for quiet mode (with B9).
- **FR-265** [packaging] Upgrades through the install channel; daemons restart on new versions.
- **FR-266** [packaging] `resonance uninstall --user`.
- **§13.3** [packaging] Later deliverables outside the binary: CI wrappers, IntelliJ plugin, agent kit as a harness plugin, graph adapters.

## B17 — Evaluation (Phase 0, §12)
*Not product code. The MVP can serve as arm C's intent server.*

- **§12.1–§12.6** [eval] Hypotheses H1 and H2, tasks, arms A–D, metrics, protocol, decision gates.
- **Self-dogfood** [eval] Run Resonance on its own repository: `resonance init`, then a Claude Code session brings every FR and NFR item in `spec/section-*.md` and `spec/mvp.md` into the graph (S26), declares the MVP's own main code units and tests and links them to the requirements they satisfy and verify, and reports what remains unverified (`resonance_unverified`); a Claude Code session answers "why does `SqliteStore` exist?" using the MCP tools. Done only after the MVP has passed field tests on other repositories (`mvp.md` §5) and Mark judges it reliable.

## Long term
- **G15** [model] An alternative to files: languages may store their source in Resonance in another format; Resonance stores and renders it, the language understands it (§2.5, §8.1 rationale).
- **§2.7** [model] Resonance's own change control, mimicking the git CLI; captured collaboration context (tickets, reviews); runtime feedback packets.
- **OQ-19** [git] Files leave git (stage 4).
