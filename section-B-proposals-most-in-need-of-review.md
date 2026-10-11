## Appendix B. Proposals most in need of review

These **[Proposed]** items shape the rest of the design most. Confirming or rejecting them first will settle much of the document.

1. The layer split, and not committing the derived layer (§6.1, FR-062).
2. Stable identity anchored on intent and tests rather than code (§6.4).
3. The link lifecycle and its statuses (§6.5).
4. File content stored in the graph and authoritative for bytes; text writes never refused (FR-010, FR-012, P3).
5. Queries served by the same owner as the watcher (FR-080, FR-081).
6. Operation-log commits (option A) as the git strategy (§8.6).
7. Opt-in projection of intent documents as Markdown (FR-121).
8. Evaluation arms separating code structure from intent structure (§11.3).
9. Phase order, especially building identity reconciliation first and deferring graph-to-disk writes to Phase 4 (§12).
