## 12. Roadmap  [Proposed]

| Phase | Scope | Exit criteria |
| --- | --- | --- |
| **0 — Evaluate** | §11. No Resonance code beyond an evaluation harness and a hand-built intent graph. | Decision gates in §11.6 evaluated. |
| **1 — Core, read-mostly** | Per-repo daemon, watcher, SQLite store, file and code-unit ingest with identity reconciliation, intent import (FR-122), intent authoring, links and annotations, `trace` / `why` queries, MCP adapter, CLI, minimal registry. Agents edit files directly; no graph-to-disk code writes yet. | S1–S4, S8, S12–S14 pass on a real repository. |
| **2 — Git integration** | Committed operation log (option chosen in OQ-01), git monitor, fold and compaction, merge semantics, conflict nodes, optional hooks. | S10–S11 pass; two clones on different branches merge cleanly. |
| **3 — Verification** | Evidence recording from test runs, link lifecycle automation, stale and orphan reporting, impact queries. | S2 AC1–AC2 and S7 pass with automatic verification. |
| **4 — Graph-to-disk mutations** | Projection writer, self-write suppression, conflict policy, API code mutations. | S9 passes with IDE and build tool noticing changes. |
| **5 — IDE** | IntelliJ plugin (FR-141, FR-142). | Developers can navigate and annotate without the CLI. |
| **6 — Regeneration workflows** | Requirement change drives agent regeneration; `realizes` / `satisfies` links recomputed and re-verified. | S5 and S7 work end to end, with links re-verified automatically. |
