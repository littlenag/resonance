## 13. Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Links rot and people stop trusting them (the historical failure of traceability tools) | Product value collapses | Link lifecycle (§6.5); prefer execution-checkable links; surface stale links prominently |
| Identity reconciliation makes wrong matches | Annotations attach to the wrong code | Build and test §8.3 first against real histories; orphan rather than guess; allow correction (FR-037) |
| Ontology grows until nobody agrees on it | Semantics become meaningless | Small core, namespaced extensions, evidence-driven additions (§6.6) |
| File-event unreliability (coalescing, delays, missed events) | Graph silently diverges from disk | Hash-based re-examination, ingest watermark, startup reconciliation, read-your-writes (§8.4, §8.8) |
| Committing SQLite causes merge conflicts and repository growth | Unusable for teams | Move to operation-log commits before shared use (§8.6) |
| Agents ignore the graph API and just grep | Graph unused | Agent instructions (FR-132); context packs that are clearly better than grepping; Phase 0 measures this |
| Hypothesis is false | Wasted effort | Phase 0 before building |
| Sensitive text committed via annotations or intent | Information exposure | Review tooling, local-only annotation class (NFR-012) |
| JVM daemon cost per repository (if built in Scala) | Memory and start-up overhead | FR-085: design for hosting several repositories in one process |
| Performance at large scale | Slow queries or ingest | Measure early against NFR-030–035 |
