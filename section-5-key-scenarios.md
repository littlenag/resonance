## 5. Key scenarios

Each scenario lists acceptance criteria (AC). Scenario IDs are referenced from requirements and from the evaluation plan.

**S1 — Onboard an existing repository.** A developer runs `resonance init` in an existing git repository.
- AC1: No existing file is modified, moved or reformatted.
- AC2: The graph is populated with files and code units, and with any requirements it can import (§8.10).
- AC3: Build, test and IDE behavior are unchanged.
- AC4: Removing Resonance (deleting `.resonance/`) leaves a working project.

**S2 — Trace forward from a requirement.** Given requirement `FR-042`, the developer asks what satisfies and verifies it.
- AC1: One query returns the code units linked by `satisfies`, the tests linked by `verifies`, and each link's status (asserted, verified, stale, orphaned).
- AC2: Requirements with no verifying test are identifiable in one query.

**S3 — Trace backward from code.** Given a function, the developer or agent asks why it exists.
- AC1: One query returns the requirements it satisfies, the decisions that constrain it, the tests that cover it, and annotations on it.

**S4 — Annotate code for an agent.** A code-fluent developer marks a function ("do not change the signature; external callers depend on it") or calls out a set of functions as relevant to a task.
- AC1: The annotation is attached to the code unit, not to a line number.
- AC2: It survives edits, reformatting, and moves or renames of the code unit (§8.3).
- AC3: If its anchor disappears, the annotation becomes *orphaned* and is surfaced, never silently lost. If implemented correctly, we would be able to ask where that annotation used to be anchored in a previous version, and surface that as helpful context.
- AC4: Agents receive relevant annotations when working on that code unit.

**S5 — Agent implements a feature from a requirement.** A developer adds or edits a requirement and asks an agent to implement it.
- AC1: The agent can retrieve a context pack: the requirement, related decisions, linked code, tests and annotations (§8.11).
- AC2: The resulting code units are linked to the requirement with `satisfies`, and new or existing tests with `verifies`.
- AC3: Links created by the agent carry agent provenance and start as *asserted* until verified.

**S6 — Agent fixes a bug.** A developer asks an agent to fix a bug described in prose.
- AC1: The agent can find requirements and tests related to the affected area.
- AC2: The fix is linked to the requirement whose behavior it restores, where one exists.

**S7 — Requirement changes.** A requirement is edited or superseded.
- AC1: Links from code and tests to the changed requirement become *stale*.
- AC2: A query lists everything affected by the change.

**S8 — Developer edits a file in an IDE.**
- AC1: The file save is accepted immediately; the graph's text for that file updates.
- AC2: Structure and links update once the file parses; until then they are marked stale, not deleted.
- AC3: No IDE or build-tool behavior changes.
- AC4: Any Resonance-aware tools surface changes, and the effect on the graph.

**S9 — Mutation through the graph API.** An agent or tool changes code or intent through the API.
- AC1: Affected files on disk update atomically.
- AC2: IDEs, build tools and test watchers notice the change through normal file-system events.
- AC3: The daemon does not re-ingest its own writes as new edits.

**S10 — Branch switch, rebase, pull.**
- AC1: The daemon recognizes the git operation and does not treat it as thousands of individual edits.
- AC2: Afterwards, the graph reflects the new commit plus any uncommitted changes.

**S11 — Merge branches that both changed the graph.**
- AC1: Authored graph changes from both branches merge without manual resolution in the common case.
- AC2: True semantic conflicts become visible conflict items, never silent data loss.

**S12 — Fresh clone.**
- AC1: Resonance rebuilds everything derivable locally, and the authored graph is complete from what git carried.

**S13 — Edits while the daemon is not running.**
- AC1: On start, the daemon reconciles the graph with the disk before answering any query.
- AC2: A missing or crashed daemon causes delay, never wrong answers.

**S14 — Discovery and health.**
- AC1: A client can find a repository's daemon without the registry.
- AC2: `resonance doctor` shows every running daemon, whether it is ready, and how far behind the disk it is.
