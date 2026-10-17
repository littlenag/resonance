## 18. Glossary

| Term | Meaning |
| --- | --- |
| **FOD** | "Files on disk": the model in which agents and developers work primarily by reading and editing files. |
| **Working tree / replica** | The ordinary files on disk. A replica of the file content held in the graph. |
| **Intent layer** | Requirements, decisions, designs and constraints. |
| **Realization** | The code that currently implements intent. |
| **Authored data** | Anything a person or agent stated directly: intent, links, annotations. Versioned in git. |
| **Derived data** | Anything recomputable from file content and recorded anchors: anchor positions, containment, caches. Not versioned. |
| **Identity layer** | Stable IDs and the recorded history of renames, moves, splits and merges. |
| **Evidence** | A recorded result of a check, such as a test run, supporting a link. |
| **Link status** | `asserted`, `verified`, `stale` or `orphaned` (§6.5). |
| **Edit algebra** | The set of textual and node-centric operations in §9. |
| **Ingest watermark** | How far disk changes have been applied to the graph. |
| **Self-write suppression** | Ignoring file events caused by the daemon's own writes. |
| **Context pack** | The bundle of intent, links, locations and annotations given to an agent for a task. |
| **Registry** | The system-wide daemon for discovery and health. |
| **Code unit** | A named region of a file's content, such as a function or type as a person or agent sees it. Declared by a person or agent, and tracked by Resonance through edits by its text. Resonance does not parse code to find code units (§2.5). |
| **Primary graph** | The graph Resonance stores and is authoritative for: intent, files, code units, tests, evidence, annotations and links (§6.7). |
| **Parallel graph** | A graph derived from the primary graph by a separate tool (for example code structure from Mache). Linked both ways and queried freely, but never able to modify the primary graph (FR-115–FR-119). |
| **Disconnected graph** | A parallel graph loaded stand-alone from an existing tool's output, with no links to primary nodes; the first rung of tool integration (FR-242). |
| **Write-capable parallel graph** | A parallel graph whose nodes carry enough information for exact edits of primary nodes, so that its edits can be translated soundly; gated (FR-241). |
| **Anchor** | How a code unit is attached to text: a byte range, the quoted text, and surrounding context, re-located after every change (§8.3). |
| **Snapshot** | `.resonance/graph.db`: the committed authored and identity data as of the last compaction. Immutable between compactions. |
| **Epoch** | The identity of a snapshot; each compaction starts a new epoch. |
| **Delta** | A small, add-only, uniquely named file of edit-algebra operations plus a file manifest, committed next to the code. *Open* while still being written; *sealed* once committed. |
| **File manifest** | The content hashes, recorded in a delta, of files whose changes Resonance saw or made. Used to detect commits made without Resonance. |
| **Compaction** | Folding a snapshot and N deltas into a new snapshot and deleting the folded deltas. Trunk only by default. |
| **Store API / adapter** | The engine-neutral persistence interface, and its engine-specific implementation (first: SQLite) in its own module. |
| **Adoption stage** | Where a repository stands: 0 absent, 1 offline trial, 2 shared (developers join one by one), 3 enforced (§8.16). |
| **Participation** | A developer's relationship to Resonance: *Resonance user* (opted in; the graph is their source of truth), *offline trial user*, *quiet participant* (§8.21) or *hold-out*. |
| **Opting in** | Joining a repository's shared graph (stage 2 or later). Opting in commits the developer to keeping their operations consistent with the graph (FR-203). Offline mode is not opting in. |
| **Quiet participant** | A developer who has not opted in, but whose agent runs a locally fetched Resonance CLI to do bookkeeping; never blocked. |
| **Offline mode** | Trying Resonance without buying in: a private graph whose only inputs are git history and the file tree; nothing committed and no visible trace (§8.17). |
| **Hold-out** | A developer who works in a Resonance repository without running Resonance at all. |
| **Authority** | Which side is the source of truth: the graph for anyone who has opted in, git for changes made outside Resonance; limited by path scope. |
| **Divergence** | A disagreement between the graph and the working tree, the index or committed `.resonance/` data, classified from git evidence (C1–C9, §8.18). |
| **Recovery journal** | The local record of every automatic resolution and of the versions it displaced, which makes each resolution undoable. |
| **Agent kit** | What onboarding installs into a developer's agent: a short static part, conventions served live over MCP, and harness hooks (§8.20). |
| **Stewardship** | The agent's duty to keep the graph and the git tree in sync and pull requests clean, by running Resonance's checks and following its conventions. |
| **Repair list** | What an offline graph needs fixed after outside changes arrive: orphaned or at-risk annotations, stale links, low-confidence identities (FR-238). |
| **Detached pool** | Offline deltas whose commits were rewritten and could not be re-associated; kept and surfaced, never dropped. |
| **Spec item** | A requirement, decision, design or constraint: the intent the TUI lists and edits. |
| **Spec TUI** | `resonance tui`, a deliberately simple terminal UI over spec items (§8.22). |
| **Change log** | A concise, local log of people's changes to spec items, which agents read from a remembered cursor and clear as they handle entries (FR-274–FR-278). |
| **Realization summary** | An agent-written, architectural-level summary of how the code linked to a spec item behaves and satisfies it, stale when either changes (FR-280, FR-281). |
