## 3. Goals and non-goals

### 3.1 Goals

- **G1** — Make requirements, decisions, code units, tests and annotations first-class, typed, queryable nodes with typed relationships. [Decided]
- **G2** — Let a developer or agent traverse requirement → code → test and back in one query. [Decided]
- **G3** — Let code-fluent developers annotate and call out code units to agents, with annotations that survive ordinary edits. [Decided]
- **G4** — Keep the working tree a standard software project, with all extra graph information behind a separate API. [Decided]
- **G5** — Work well with build systems, IDEs and source control, which requires ordinary file-system events. [Decided]
- **G6** — Integrate with git using a git-friendly algebra of textual and node-centric edits. [Decided]
- **G7** — Provide an onboarding path for existing projects. [Decided]
- **G8** — Keep links trustworthy by tracking provenance, verification status and staleness. [Decided]
- **G9** — Users decide if they want to use Git or not. First-class support for and easy operation with git is non-negotiable. [Decided]
- **G10** — For existing projects Code stays in its existing languages as text. However, in the near future Code is likely to be re-imagined for agents. A graph structure should support whatever possible structure that is. [Decided]
- **G11** — Not a general-purpose graph database product; there are particular usage patterns and data models that must be optimized for. The graph engine is more than an implementation detail, since its performance, behavior, and long-term support is critical to our success. That said, we can either develop one ourselves, fork one, or develop on top of an existing one. [Open]

### 3.2 Non-goals

- **NG1** — No NFS-mounted file system; Mark judged NFS mounting too fraught. [Decided] The problems identified in discussion were that changes made on the server side produce no FSEvents or inotify events, and that NFS client caching delays visibility. Other virtual file systems (FUSE, FSKit) are excluded for similar file-event reasons. [Proposed]
- **NG2** — No required changes to build tools, IDEs, test runners or CI. Integrations (such as an IDE plugin) are optional improvements. [Proposed]
- **NG3** — Not a hosted multi-user server in the first versions. Resonance is local-first; collaboration happens through git, or other change control. [Proposed]
- **NG4** — Code-structure extraction is not a primary investment until Phase 0 shows it pays off. [Decided in substance; see §2.5]
