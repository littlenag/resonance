## 5. Key scenarios

Each scenario lists acceptance criteria (AC). Scenario IDs are referenced from requirements and from the evaluation plan.

**S1 — Onboard an existing repository.** A developer runs `resonance init` in an existing git repository.
- AC1: No existing file is modified, moved or reformatted.
- AC2: `init` mirrors the repository into the graph (its files), installs the git hooks for the stage, and installs the agent kit. It reads no requirements: spec documents become graph items when the agent imports them (S26). Code units appear as agents declare them; nothing is parsed.
- AC3: Build, test and IDE behavior are unchanged.
- AC4: Removing Resonance (deleting `.resonance/`) leaves a working project.
- AC5: In a repository with no committed Resonance data, `init` starts in offline mode, and `git status` is unchanged afterwards (S15, FR-180).

**S2 — Trace forward from a requirement.** Given requirement `FR-042`, the developer asks their agent what satisfies and verifies it, and the agent answers from the graph.
- AC1: One query returns the code units linked by `satisfies`, the tests linked by `verifies`, and each link's status (asserted, verified, stale, orphaned).
- AC2: Requirements with no verifying test are identifiable in one query.

**S3 — Trace backward from code.** Given a function, the agent asks why it exists, before changing it or because the developer asked.
- AC1: One query returns the requirements it satisfies, the decisions that constrain it, the tests that cover it, and annotations on it.

**S4 — Annotate code for an agent.** A code-fluent developer marks a function ("do not change the signature; external callers depend on it") or calls out a set of functions as relevant to a task, by telling their agent (or, later, in the IDE plugin).
- AC1: The annotation is attached to the code unit, not to a line number.
- AC2: It survives edits elsewhere in the file, edits inside the region, and moves or renames of the file (§8.3). A region rewritten beyond recognition is orphaned and goes on the repair list for the agent to re-anchor; it is never silently re-attached.
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
- AC2: Code-unit anchors in the file are re-located immediately. Any that cannot be re-located become orphaned and appear on the repair list; they are never deleted.
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
- AC1: Resonance rebuilds everything derivable locally, and the authored graph is complete from what git carried (snapshot plus deltas at HEAD, stage 2 and later).
- AC2: In a repository where nobody has shared a graph yet, Resonance says so plainly and offers to start in offline mode.

**S13 — Edits while the daemon is not running.**
- AC1: On start, the daemon reconciles the graph with the disk before answering any query.
- AC2: A missing or crashed daemon causes delay, never wrong answers.
- AC3: Each difference found is classified using git evidence and resolved according to who is authoritative for that file (§8.18). Nothing is lost, and the developer gets one summary, not one message per file.

**S14 — Discovery and health.**
- AC1: A client can find a repository's daemon without the registry.
- AC2: `resonance doctor` shows every running daemon, whether it is ready, and how far behind the disk it is.

**S15 — Try Resonance offline.** A developer wants to try Resonance on a team repository without committing anything or letting teammates know.
- AC1: `resonance init` modifies no tracked file; `git status` is unchanged. `init` explains plainly what offline mode cannot do and what repair will cost (FR-180).
- AC2: Every local feature works: the agent's import of spec documents, trace, annotate and verify through MCP, the web UI and the spec TUI (FR-181).
- AC3: Branch switches, pulls and rebases keep the developer's local graph data attached to the right code (FR-183).
- AC4: Teammates see no trace of Resonance: nothing in commits, branches, pull requests or code (FR-189).
- AC5: After a teammate renames an annotated function, the next pull produces a repair list; the agent re-anchors the annotation, asking when unsure; and the cost is shown (FR-238, FR-239).
- AC6: Asked, the agent shows what Resonance did for the developer, and how to upgrade (`resonance_value`, FR-240).
- AC7: Leaving is clean: `resonance eject` offers an export, then removes `.resonance/` and every local hook and exclusion it added.

**S16 — Share the graph (offline → stage 2).** The offline developer decides to buy in and commit the graph.
- AC1: `resonance share` previews exactly what will be committed. Local-only annotations stay local (FR-185).
- AC2: The result is ordinary new files under `.resonance/` for the developer to commit. No existing tracked file changes without an explicit confirmation.
- AC3: If a teammate shared first, the local graph merges into the committed one with no duplicated requirements (FR-186).

**S17 — Hold-out teammate.** A teammate who does not run Resonance keeps working with plain git.
- AC1: The hold-out never installs, configures or learns Resonance. In the common case they never see a conflict in `.resonance/`; in the rare case they do, taking either side is safe and `.resonance/README.md` says so (FR-174).
- AC2: When a Resonance user pulls the hold-out's commits, the file changes are ingested as git-originated edits attributed to that commit. Affected links become stale, and the user gets a single summary (C2 in §8.18).

**S18 — Git and the graph disagree.** After a pull, reset, stash, merge, daemon outage or bypassed hook, the working tree no longer matches the graph.
- AC1: Resonance classifies the cause from git evidence before acting (FR-190).
- AC2: It acts automatically only when the action destroys nothing. The displaced version is preserved and can be restored with one command (FR-192).
- AC3: The message says what happened, why (citing git evidence), what Resonance did, where the displaced version is, how to undo, and what to do next (FR-200).

**S19 — Compaction with branches in flight.** Trunk reaches N deltas and is compacted while feature branches still carry older deltas.
- AC1: Feature branches created before compaction merge into trunk with no conflict in `.resonance/` (FR-155).
- AC2: If two compactions do collide on `graph.db`, the collision is repaired mechanically, with no data loss (FR-157).

**S20 — Step up, step back down.** A team moves between adoption stages, and at some point steps back.
- AC1: Each move is one explicit command with a preflight check and a preview (FR-172).
- AC2: Stepping back, including all the way to stage 0, loses no authored data (FR-173).

**S21 — Replace the storage engine.** Resonance moves from SQLite to a graph engine.
- AC1: No code outside the storage module changes, and the new adapter passes the store conformance suite (FR-161, FR-167).
- AC2: Existing repositories move over through one migration compaction. Delta files and git history need no rewriting (FR-165).

**S22 — Onboard the agent and let it steward a change.** A developer runs `resonance init`; later their agent implements a change and opens a pull request.
- AC1: Onboarding installs the agent kit, and the agent's orientation shows that it knows the stage, the developer's participation and its duties (FR-210, FR-225).
- AC2: Every commit the agent makes passes `resonance check --staged`, with the open delta committed together with its code (FR-215).
- AC3: The pull request carries a readable summary of graph changes and merges with no conflicts in `.resonance/` (FR-216).
- AC4: Asked to do something that breaks a convention (for example `git commit --no-verify`), the agent names the convention and offers the compliant alternative. It proceeds only on explicit instruction (FR-218, FR-220).

**S23 — Quiet onboarding.** A new team member follows the team's setup instructions, which include quiet onboarding, but does not opt in to Resonance.
- AC1: One documented bootstrap command fetches and verifies the pinned CLI. Nothing is installed system-wide, and no daemon runs (FR-231).
- AC2: The developer's commits are never blocked, and they may push without Resonance data (FR-203).
- AC3: When bookkeeping succeeds, Resonance users ingest the developer's commits as consistent, and annotations follow renamed code. When it fails, the commits are absorbed as outside commits, and nothing breaks (FR-233, FR-235).
- AC4: The developer can opt in later with `resonance init`, or stop by removing the local setting (FR-237).

**S24 — An opted-in developer slips.** A Resonance user commits with `--no-verify` while their daemon is down.
- AC1: Before the push leaves the machine, Resonance detects that the commit is inconsistent with the graph and explains how (C8).
- AC2: The agent repairs it by amending or adding a follow-up delta, with the developer's confirmation, and nothing is lost (FR-192, FR-203).

**S25 — A person edits the spec; the agent picks it up.** A developer changes requirements in the web UI or `resonance tui` (or in a spec document an agent imported), and later asks their agent to carry on.
- AC1: Each change is an ordinary graph change attributed to the person, and appears in the change log as one concise entry per item (FR-274, FR-275).
- AC2: The agent reads the entries after its remembered cursor, acts on each, and removes the handled ones (FR-276).
- AC3: Before the agent commits, pushes or pulls, the change log is empty, or the developer has told the agent to proceed anyway (FR-277).
- AC4: Retiring an item in the web UI or the TUI never deletes it (FR-271). Editing an item that came from a document makes its link to that document stale, and the agent brings the document back in step; editing the document makes the same link stale, and the agent proposes the change to the item (FR-272).

**S26 — The agent imports spec documents.** A developer asks their agent to bring the project's requirements into Resonance, from Markdown, a Spec Kit or Kiro spec, EARS statements or ADRs.
- AC1: Resonance parses nothing; the agent reads the documents itself and works through the API (NG6, FR-122).
- AC2: Before anything is written, the developer sees a preview: the items the agent will create or update, with keys, types and text, any key that clashes, and any reformatting it proposes for the documents (FR-122).
- AC3: Each item is linked to the region of the document it came from (`derivedFrom`), so that later edits on either side make the link stale (FR-124).
- AC4: Documents are kept. They are reformatted only with confirmation, so that each item is clearly delimited and carries its key. Afterwards the graph is the source of truth.

**S27 — A person works in the web UI and looks inside the daemon.** A developer runs `resonance ui` in each of two repositories, works on the spec in one, and investigates why the other seems slow.
- AC1: Each command opens the person's browser on its own repository's UI, starting the daemon if needed; the two daemons use different ports, and neither asks the person to log in (FR-286, FR-287).
- AC2: Another web page, another user on the machine, or a request naming another host cannot read or change anything (FR-288).
- AC3: Everything the TUI does can be done in the web UI, attributed to the person and logged in the change log, and the page stays current while an agent works (FR-290, FR-291).
- AC4: The diagnostics view shows what the daemon has done since it started (requests, reconciles, git operations, change sets) with timings, who is connected, and the live log; the person raises the log level, reconciles, and exports the events for a bug report without item text in it (FR-293–FR-298).
