## 14. Open questions

| ID | Question | Notes |
| --- | --- | --- |
| **OQ-01** | How is authored data committed to git: operation-log files (A), custom refs (B), or the SQLite file (C)? | **Resolved** by Mark: a SQLite snapshot plus delta files, compacted every N (option D, FR-026, §8.6.1). |
| **OQ-02** | Is the committed delta format text (JSON Lines) or compact binary? | Decide on fidelity and size, not git diffs (FR-072); PR review comes from tooling (FR-066). Must be engine-neutral (FR-164). |
| **OQ-03** | Which graph engine replaces SQLite, and when? Build, fork, or build on an existing one (G11)? | Mark expects to move to a real graph engine after research. SQLite stays behind the store API until then (§8.2.1); candidates are judged against the conformance suite and NFR-030–035. |
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
| **OQ-14** | Is there eventually a shared, multi-user graph service, or does collaboration stay git-only? | NG3 |
| **OQ-15** | Licence. | Project name decided: Resonance. |
| **OQ-16** | Are folded operations ever pruned from snapshots, and how? | FR-157 needs them for mechanical repair; git history keeps the original deltas. |
| **OQ-17** | Compaction defaults: the value of N, a size trigger, how the trunk is detected, and whether the daemon compacts automatically or only CI and explicit commands do. | FR-154–FR-156 |
| **OQ-18** | Must offline mode be branch-aware from Phase 1, or is one global local graph acceptable at first? | FR-183 |
| **OQ-19** | Do files ever leave git (stage 4)? If so, file content moves into committed Resonance data, and unaware tools get files only from the projection. | §8.16, FR-064, FR-074 |
| **OQ-20** | Is path-scoped authority needed, and at what granularity (directory, glob, file)? | FR-178 |
| **OQ-21** | Which agent harnesses beyond Claude Code get first-class kits (skills, hooks), and which get only `AGENTS.md` plus MCP? | FR-214 |
| **OQ-22** | What may quiet bookkeeping write: manifests and identity operations only, or also agent-asserted links? Should a CI bot add bookkeeping commits by default at stage 3? | FR-233, FR-235, FR-236 |
