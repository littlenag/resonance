## 3. Goals and non-goals

### 3.1 Goals

- **G1** — Make requirements, decisions, code units, tests and annotations first-class, typed, queryable nodes with typed relationships. [Decided]
- **G2** — Let a developer or agent traverse requirement → code → test and back in one query. [Decided]
- **G3** — Let code-fluent developers annotate and call out code units to agents, with annotations that survive ordinary edits. [Decided]
- **G4** — Keep the working tree a standard software project, with all extra graph information behind a separate API. [Decided]
- **G5** — Work well with build systems, IDEs and source control, which requires ordinary file-system events. [Decided]
- **G6** — Integrate with git using a git-friendly algebra of textual and node-centric edits (§9), committed as a snapshot plus a sequence of delta files (§8.6.1). [Decided]
- **G7** — Provide an onboarding path for existing projects that is incremental and reversible at every step (G12). [Decided]
- **G8** — Keep links trustworthy by tracking provenance, verification status and staleness. [Decided]
- **G9** — Git compatibility is non-negotiable. Teams that onboard Resonance already use git and will keep using git while they try and adopt Resonance. That includes Resonance's own development: Mark and Claude develop Resonance in git and adopt Resonance into its own repository stage by stage (FR-071). "Compatible" means Resonance fits ordinary git workflows and never requires teammates to change how they use git. It does **not** mean bending the graph store to suit git (NG5). Working without git, through Resonance's own change control (§2.7), is a long-term option and never an onboarding requirement. [Decided]
- **G10** — For existing projects Code stays in its existing languages as text. However, in the near future Code is likely to be re-imagined for agents. A graph structure should support whatever possible structure that is. [Decided]
- **G11** — Not a general-purpose graph database product; there are particular usage patterns and data models that must be optimized for. The graph engine is more than an implementation detail, since its performance, behavior, and long-term support is critical to our success. That said, we can either develop one ourselves, fork one, or develop on top of an existing one. [Open]
- **G12** — Support incremental adoption by people and by teams: offline trial by one developer, a shared graph, Resonance as source of truth for one developer, then a few, then most, and finally enforcement. Each stage must be safe to stay in indefinitely and to step back from, with strong guardrails, recovery-focused communication, and working data paths in both directions between git and the graph at every stage (§8.16–§8.19). [Decided]
- **G13** — Keep the storage engine replaceable. SQLite is the starting engine; a dedicated graph engine is expected later. All engine-specific code sits behind an engine-neutral storage API in its own module (§8.2.1). [Decided]
- **G14** — Onboarding Resonance includes onboarding the developer's agent, which gets foreknowledge of Resonance's conventions and operations and enforces most of them: it keeps the graph and the git tree in sync and keeps pull requests merging cleanly. Deterministic checks back it up, with hooks and CI behind those (§8.20). [Decided]
- **G16** — Resonance is a superpower for the agent and a background tool for the developer. It gives agents a direct way to connect, and so to show back to the developer, their understanding of the spec and how it relates to everything else: code, tests and documents. It gives developers an easier way to organize the totality of input to an agent, so that the agent builds both what they said and what they meant. Graph work therefore happens through the API, used by agents over MCP, and through the spec TUI; the CLI is for setup, operation and repair (FR-259). [Decided]
- **G15** — As an eventual goal, offer an alternative to files for languages that want to store their source in a format other than text files. Semantic understanding of that source stays outside Resonance (NG4). [Decided]

### 3.2 Non-goals

- **NG1** — No NFS-mounted file system; Mark judged NFS mounting too fraught. [Decided] The problems identified in discussion were that changes made on the server side produce no FSEvents or inotify events, and that NFS client caching delays visibility. Other virtual file systems (FUSE, FSKit) are excluded for similar file-event reasons. [Proposed]
- **NG2** — No required changes to build tools, IDEs, test runners or CI. Integrations (such as an IDE plugin) are optional improvements. [Proposed]
- **NG3** — Not a hosted multi-user server in the first versions. Resonance is local-first; collaboration happens through git, or other change control. [Proposed]
- **NG4** — Resonance core does not parse source code into syntax trees or other abstract data types, and does not provide semantic understanding of code (symbols, types, references). Such structure may be supplied later by a separate tool, as a parallel graph that cannot modify the primary graph (§2.5, §6.7, §8.9). [Decided]
- **NG6** — Resonance does not parse specification documents or any other intent sources. Reading a requirements document, a Spec Kit or Kiro spec, EARS statements or ADRs, and turning them into graph items, is the agent's job, guided by the agent kit and served by the API (FR-122). Resonance never needs a parser for a document format. [Decided]
- **NG5** — No reshaping of the data model for git's benefit. Beyond committing a SQLite snapshot plus delta files that are periodically compacted (FR-026, §8.6.1), Resonance will not adopt line-oriented formats, one-file-per-node layouts or similar structures just to improve git diffs or merges. [Decided]
