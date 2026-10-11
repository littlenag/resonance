## Appendix B. Proposals most in need of review

These **[Proposed]** items shape the rest of the design most. Confirming or rejecting them first will settle much of the document.

1. The layer split, and not committing the derived layer (§6.1, FR-062).
2. Stable identity anchored on intent and tests rather than code (§6.4).
3. The link lifecycle and its statuses (§6.5).
4. File content stored in the graph and authoritative for bytes; text writes never refused (FR-010, FR-012a, P3); authority now staged, see T13.
5. Queries served by the same owner as the watcher (FR-080, FR-081).
6. Operation-log commits (option A) as the git strategy (§8.6).
7. Opt-in projection of intent documents as Markdown (FR-121).
8. Evaluation arms separating code structure from intent structure (§11.3).
9. Phase order, especially building identity reconciliation first and deferring graph-to-disk writes to Phase 4 (§12).
10. Semantic, engine-neutral deltas, and the open/sealed delta lifecycle (FR-075–FR-077, T14).
11. Trunk-only compaction, no commits made by Resonance itself, and mechanical repair of colliding snapshots (FR-155–FR-157).
12. The four adoption stages, the four kinds of participation, and per-developer authority from the moment of joining the shared graph (§8.16, FR-203).
13. The divergence classification C1–C9 and its default resolutions (§8.18).
14. Offline mode as an invisible trial: on by default, branch-aware offline deltas, lazy and budgeted agent repair, and read-only use of committed data in shared repositories (FR-180, FR-183, FR-187, FR-238–FR-240, OQ-18).
15. The store API's scope and the snapshot-never-live rule (FR-162, FR-169).
16. The agent kit's layered delivery and the stewardship duties (FR-212, FR-215–FR-222).
17. Quiet onboarding: the bootstrap and pinning mechanism, and what quiet bookkeeping may write (§8.21, OQ-22).
