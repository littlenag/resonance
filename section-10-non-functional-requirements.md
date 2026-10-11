## 10. Non-functional requirements

Numeric targets below are placeholders to be replaced with measurements from Phase 0 and Phase 1. They are listed so that they exist to be argued with.

| ID | Requirement | Target | Status |
| --- | --- | --- | --- |
| **NFR-030** | Single-file ingest latency (file under 1 MB, save to graph updated) | p95 under 200 ms | [Open] |
| **NFR-031** | Startup reconciliation, 50,000-file repository, warm OS cache | under 30 s | [Open] |
| **NFR-032** | One-hop navigation query | p95 under 50 ms | [Open] |
| **NFR-033** | `trace` / `why` query (up to 3 hops) | p95 under 200 ms | [Open] |
| **NFR-034** | Reference repository scale | ~1M lines of code, ~50,000 files | [Open] |
| **NFR-035** | Daemon memory at reference scale | to be measured | [Open] |
| **NFR-036** | No loss of a text write, including across daemon crashes | always | [Proposed] |
| **NFR-037** | A missing or crashed daemon causes delay, never wrong answers (P4) | always | [Proposed] |
| **NFR-038** | Platforms for the first release | macOS and Linux | [Proposed] |
| **NFR-039** | Git hosting | Must work with any host; no host-specific features required | [Proposed] |
| **NFR-040** | No automatic action loses file content or authored graph data; whatever a resolution displaces can be restored (FR-192) | always | [Decided in substance] |
| **NFR-041** | `resonance init` in offline mode leaves `git status` unchanged (FR-180) | always | [Proposed] |
| **NFR-042** | New `graph.db` blobs entering git history | at most one per compaction (every N deltas on trunk, FR-154) | [Proposed] |
| **NFR-043** | Catch-up after a pull that changes 1,000 files, including classification (FR-195) | under 30 s | [Open] |
| **NFR-044** | Committed snapshot size at reference scale (authored and identity data only) | to be measured | [Open] |
| **NFR-045** | Divergences classified as unexplained (C9) after the first months of real use | under 1% of divergences | [Open] |
| **NFR-046** | Agent-made commits that pass `resonance check` on the first attempt (FR-224) | over 95% | [Open] |
