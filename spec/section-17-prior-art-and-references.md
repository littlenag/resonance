## 17. Prior art and references

**Closest systems**

- Mache — graph projection of code with write-back and stable node paths; file-primary: https://github.com/agentic-research/mache
- ley-line-open — Mache's parsing and graph substrate: https://github.com/agentic-research/ley-line-open
- Perkeep — immutable blobs plus mutable permanodes built from signed claims; `pk-mount` projects the graph as a file system: https://perkeep.org/ , https://perkeep.org/cmd/pk-mount/
- Unison — code stored as a content-addressed syntax-tree database, edited through scratch files.
- JetBrains MPS — projectional editing over stored syntax trees; shows the costs of making structure primary.

**Spec-driven development (intent layer, file-based)**

- Kiro (AWS): https://kiro.dev
- GitHub Spec Kit
- Tessl: https://tessl.io
- Codeplain
- Böckeler, *Understanding Spec-Driven Development: Kiro, spec-kit, and Tessl*: https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html

**Code-graph and memory servers for agents**

- lxDIG MCP: https://github.com/lexCoder2/lxDIG-MCP
- CodeGraph: https://github.com/colbymchenry/codegraph
- foghorn (decision staleness): https://pypi.org/project/foghorn-ai/
- MemoryGraph: https://github.com/gregorydickson/memory-graph

**Research**

- *Code Isn't Memory: A Structural Codebase Index Inside a Coding Agent* (2026) — controlled ablation of a structural index: https://arxiv.org/abs/2606.22417
- RepoGraph (ICLR 2025): https://arxiv.org/abs/2410.14684
- LocAgent (ACL 2025): https://arxiv.org/abs/2503.09089
- TraceDev (ISSTA 2026), requirement-to-code traceability: https://github.com/ISSE-Lab/ISSTA2026-TraceDev
- Trust-aware multi-agent traceability with confidence-calibrated knowledge graphs (2026): https://arxiv.org/abs/2606.17203
- TDAD, graph-based impact analysis to reduce agent regressions (2026): https://arxiv.org/abs/2603.17973
- GumTree — fine-grained source-code differencing (tree matching); relevant only to a code-structure parallel graph.
- W3C Web Annotation Data Model (TextQuoteSelector, TextPositionSelector) and Hypothesis — robust anchoring of annotations to text by quote plus context, the model for code-unit anchors (§8.3).
- Kleppmann et al. — a highly available move operation for replicated trees.

**Versioning and storage patterns**

- git-bug (operation histories as git objects); Gerrit NoteDb (metadata in git refs); Dolt and TerminusDB (databases with git-like versioning).

**Process and file-watching patterns**

- Watchman (system-wide watcher with per-root queries); git's built-in fsmonitor daemon (per-repo, socket in `.git`); sbt server (`project/target/active.json` for discovery); Bloop and the Gradle daemon (shared daemons).
