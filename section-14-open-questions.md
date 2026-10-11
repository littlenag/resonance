## 14. Open questions

| ID | Question | Notes |
| --- | --- | --- |
| **OQ-01** | How is authored data committed to git: operation-log files (A), custom refs (B), or the SQLite file (C)? | §8.6. Recommended: A, with C only for bootstrapping. |
| **OQ-02** | Is the committed operation format text (JSON Lines) or compact binary? | Text favors review; binary favors size. |
| **OQ-03** | Is SQLite sufficient as the query engine long term, or is an embedded graph engine needed? | Start with SQLite and recursive CTEs; measure. |
| **OQ-04** | API transport: JSON-RPC or gRPC over Unix sockets? | §8.8 |
| **OQ-05** | Is a general query language needed, and which one? | Start with fixed operations and a read-only SQL escape hatch. |
| **OQ-06** | Which languages are supported first? | Likely Scala plus the evaluation codebase's language. |
| **OQ-07** | Windows support: when, and with which file-event API? | Not in the first release. |
| **OQ-08** | Do git worktrees share a derived-data cache? | §8.6 FR-070 |
| **OQ-09** | Is verification evidence committed, summarized, or kept local? | Proposed: full records local, latest summary committed. |
| **OQ-10** | How do code-fluent developers annotate code: graph-only, tagged comments, or both? | Proposed: both, graph-only by default. |
| **OQ-11** | Implementation language: Scala (deep expertise, JVM cost for daemons), Rust (small, fast daemons), Go, or a mix (for example a Rust daemon with Scala tooling)? | Affects FR-085. |
| **OQ-12** | Where do projected intent documents live, and in what Markdown conventions? | FR-121 |
| **OQ-13** | Should the registry ever supervise (restart) daemons? | FR-094 |
| **OQ-14** | Is there eventually a shared, multi-user graph service, or does collaboration stay git-only? | NG6 |
| **OQ-15** | Licence. | Project name decided: Resonance. |
