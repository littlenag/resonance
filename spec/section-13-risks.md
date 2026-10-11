## 13. Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Links rot and people stop trusting them (the historical failure of traceability tools) | Product value collapses | Link lifecycle (§6.5); prefer execution-checkable links; surface stale links prominently |
| Identity reconciliation makes wrong matches | Annotations attach to the wrong code | Build and test §8.3 first against real histories; orphan rather than guess; allow correction (FR-037) |
| Ontology grows until nobody agrees on it | Semantics become meaningless | Small core, namespaced extensions, evidence-driven additions (§6.6) |
| File-event unreliability (coalescing, delays, missed events) | Graph silently diverges from disk | Hash-based re-examination, ingest watermark, startup reconciliation, read-your-writes (§8.4, §8.8) |
| Committing SQLite causes merge conflicts and repository growth | Unusable for teams | Snapshot rewritten only once per compaction; add-only deltas on branches; trunk-only compaction; mechanical repair of colliding snapshots (§8.6.1, FR-154–FR-157) |
| Agents ignore the graph API and just grep | Graph unused | Agent kit installed at onboarding (FR-132, FR-210); context packs that are clearly better than grepping; Phase 0 measures this |
| Hypothesis is false | Wasted effort | Phase 0 before building |
| Sensitive text committed via annotations or intent | Information exposure | Review tooling, local-only annotation class (NFR-012) |
| JVM daemon cost per repository (if built in Scala) | Memory and start-up overhead | FR-085: design for hosting several repositories in one process |
| Performance at large scale | Slow queries or ingest | Measure early against NFR-030–035 |
| One silent data loss during adoption | Trust is lost and adoption stops, possibly for good | Non-destructive automatic actions only, recovery journal, one-command undo, local journal independent of the working tree (FR-192–FR-194, NFR-040) |
| Mixed authority confuses people ("which copy is right?") | Wrong recoveries; people revert to plain git | Explicit authority per path (P6, FR-171); classification from git evidence (§8.18); explanations that cite git operations (FR-200) |
| Hold-outs hit Resonance conflicts or unfamiliar files | Resentment; pressure to remove Resonance | Add-only deltas, trunk-only compaction, `.resonance/README.md`, "take either side is safe" (FR-174) |
| Offline data lost before it is shared (`git clean -fdx`, a new laptop) | Trial users lose work and give up | Local backups outside the repository, detection of a missing `.resonance/` (FR-184) |
| SQLite assumptions leak beyond the adapter | Moving to a graph engine becomes a rewrite | Store API, adapter in its own module, dependency check, conformance suite, second adapter from day one (§8.2.1) |
| Snapshot grows without bound because folded operations are retained | Repository bloat, slow clones | Measure (NFR-044); pruning policy (OQ-16) |
| Team-wide enforcement arrives before trust | Developers bypass checks or demand removal | Enforcement is the last stage, explicit and reversible. Before it, only developers who opted in are held to consistency, and anyone can leave (FR-173, FR-176, FR-203) |
| Agents do not follow the conventions reliably (instructions forgotten, context truncated, a different harness) | Out-of-sync commits, messy PRs, divergences | Deterministic checks rather than prose (FR-221), agent-side hooks (FR-222), conventions served live over MCP (FR-212), hooks and CI as backstop (FR-223), measured effectiveness (FR-224) |
| Agent kit drifts from the installed Resonance version or stage | Agents enforce outdated rules | Minimal static part, conventions served by the daemon, kit versioning and doctor checks (FR-212, FR-214) |
| Hold-outs' agents read shared instructions about a tool they do not have | Confused agents, edits to `.resonance/` | Shared kit text is safe without Resonance: work normally, never touch `.resonance/` (FR-213) |
| Opted-in developers find consistency checks a burden | They leave, or bypass hooks habitually | The agent does the work and offers the fix along with the finding (§8.20). Blocks always explain themselves. Leaving is easy and lossless (FR-173) |
| Supply-chain attack through the quiet CLI bootstrap | Code execution on developers' machines | Version and checksum pinned in committed `config.toml` and verified before running (FR-231); signed releases |
| Quiet bookkeeping writes wrong manifests or identity operations | Annotations follow the wrong code | Resonance users verify quiet deltas against their commits (FR-235); orphan rather than guess (FR-034) |
| Offline trial users judge Resonance by its hobbled form (divergence, repair cost) and walk away | Lost adopters | Set expectations at `init`; recommend repositories with few outside changes; lazy, budgeted repair; `resonance value` showing benefit against cost; an easy upgrade (FR-180, FR-238–FR-240) |
