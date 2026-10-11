## Appendix A. Consistency check against Mark's statements

Each row is a statement Mark made during the design discussion, paraphrased, with where the specification addresses it.

| # | Mark's statement | Where addressed | Status |
| --- | --- | --- | --- |
| 1 | Wants a bridge between a file system and a graph database, using the graph to provide information about files that would otherwise not be possible. | §1, §6 | Addressed |
| 2 | Wants the graph database as primary, projecting an editable version into a file system. | §8.1, §8.5 | Addressed; see T1, T2 |
| 3 | Mache best matches his requirements (write-back, identity). | §8.3, §15 | Addressed |
| 4 | Wants something optimized for the usage patterns and workflows Mache targets, rather than Perkeep's. | §8.11, §8.8 | Addressed |
| 5 | Recreate Mache with the graph as source of truth. Restriction: the file system is a standard software project; extra graph nodes and operations are hidden behind another API. | G4, FR-014, FR-015 | Addressed; see T1 |
| 6 | Most interested in how files reach the disk (virtual mount versus a replica checkout). Weak points to solve: build systems and source control. | §7, §8.4–§8.6, §8.13 | Addressed |
| 7 | The graph store will likely be a structured blob such as SQLite, stored in the repo alongside regular files; not ideal, but a good start and an onboarding path for existing projects. | FR-020, G7, S1 | Addressed; see T3 |
| 8 | Must work well with build systems, IDEs and source control, which requires regular file events. | G5, NFR-001–005, FR-051 | Addressed |
| 9 | Wants a watcher; NFS mounting is too fraught. | FR-040, NG1 | Addressed |
| 10 | Asked whether the watcher should also serve graph queries, or whether that belongs in a system-wide process. | FR-080, FR-081 | Proposed answer: same owner. Needs confirmation. |
| 11 | A system-wide daemon only for service discovery (which endpoint serves a directory) and watcher health monitoring. | FR-088–FR-096 | Addressed |
| 12 | Git integration is the next significant problem. The graph store need not be textual; it should be optimized. Edits are textual or node-centric with a git-friendly algebra. | FR-060, FR-061, §8.6, §9 | Addressed; see T4 |
| 13 | Files-on-disk development (FOD) is an obsolete model; much FOD tooling will atrophy. | §2.1 | Addressed; see T6 |
| 14 | Agents should work with a representation natural to their abilities that also tracks context and alignment with higher-level concerns. | §2.1, §6, §8.11 | Addressed; see T6 |
| 15 | Developers will focus on high-level design and requirements; agents generate solutions, of which code is only a part. | §2.2, S5 | Addressed |
| 16 | The critical capability is stepping between requirements, the code that satisfies them, and the tests of that code. | G2, S2, S3, §6.3 | Addressed |
| 17 | Developers who still speak programming languages will want to annotate and call out functions to an agent. | G3, S4, FR-123, FR-133 | Addressed; mechanism is OQ-10 |
| 18 | Graph databases break everything up, annotate every node and relation with agreed semantics, and let an agent and an IDE speaking the same graph model create solutions. | §2.3, §6.6, §8.11, §8.12 | Addressed |
| 19 | Skeptical that code structure (tree-sitter, as in Mache) is a first-order benefit; possibly useful for planning and verifying large refactors. | §2.5, §8.9, NG7 | Addressed; see T5 |
| 20 | Wants to test "fix this bug" and "add this feature" with and without structure, on a fairly large existing codebase, ideally the one at work. | §11.2, §11.3 | Addressed |
| 21 | Expects structure to decrease tokens, wall-clock time and, if fully leveraged as envisioned, developer time. | §11.1, §11.4 | Addressed; see T5 |
| 22 | Wants a Markdown specification, as complete as possible, that he can expand and that Claude can work from. | This document; §0 | Addressed |

### Tensions found

- **T1 — "Mounted file system" versus "no mount."** Mark's restriction was first phrased around a mounted file system; he later rejected NFS mounting as too fraught. This specification keeps the substance of the restriction (the files form a standard project; graph-only information stays behind a separate API) and delivers it as a replica working tree instead of a mount (NG1, §7). **Confirm** that the restriction still applies unchanged to the replica.
- **T2 — "Graph is the source of truth" versus text files being edited directly.** Resolved in §8.1: the graph stores exact file content and is authoritative, and the working tree is its replica. Text writes are always accepted and flow into the graph. Structure is derived from that text.
- **T3 — "SQLite in the repo" versus mergeable git history.** A committed SQLite file cannot be merged by git. §8.6 keeps SQLite as the store in every option but proposes committing an operation log for anything shared. **Decide** OQ-01.
- **T4 — "Need not be textual" versus a textual committed form.** Mark's statement is honored for the store itself (FR-060). The committed form of the edit algebra is a separate choice; text is recommended only for review in pull requests. **Decide** OQ-02.
- **T5 — Skepticism about structure versus a hypothesis that structure reduces tokens, time and developer time.** These can both be true if they refer to different kinds of structure. §11 separates code structure (arm B, expected small effect) from intent structure (arm C, the Resonance thesis). **Confirm** which kind of structure the stated hypothesis referred to, or whether it covers both.
- **T6 — "FOD is obsolete" versus agents still editing text files.** This specification lets agents keep editing files (FR-131) because generation is still strongest in text and every downstream tool consumes files, and it treats files as a projection (§2.4). **Confirm** this is an acceptable migration stance, rather than requiring agents to work only through the graph.
- **T7 — "Which port is active" versus Unix sockets.** Mark described discovery as finding which port serves a directory. FR-086 proposes Unix domain sockets instead of TCP ports, for access control and to avoid exposing a mutating service on localhost (FR-087). Discovery works the same either way: it returns an endpoint. **Confirm** sockets, or keep TCP ports with the token requirement.
- **T8 — Projected intent documents versus "everything extra hidden behind another API."** FR-121 proposes optionally projecting requirements and decisions as Markdown files in the working tree, which departs from the decided restriction in FR-015. It is opt-in and labelled as an exception. **Confirm** or drop it.
