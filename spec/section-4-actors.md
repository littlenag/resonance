## 4. Actors

| Actor | Description | How they interact |
| --- | --- | --- |
| **Intent-level developer** | Works mainly in requirements and design; may rarely read code. | Authors requirements and decisions; reviews traces and verification status. |
| **Code-fluent developer** | Still works in programming languages. | Edits files in an IDE; annotates and calls out code units to agents; reviews generated code. |
| **Agent** | A coding agent such as Claude Code. Onboarded with the agent kit as part of onboarding Resonance (§8.20). | Uses the graph API (via MCP) for intent, traces and annotations; may edit files directly or mutate through the API. Acts as the first line of enforcement of the developer's obligation to stay consistent with the graph: checks before commits and pull requests, recovers through Resonance, never bypasses hooks unasked. |
| **Offline trial developer** | Curious about Resonance but not ready to buy in. Tries it alone, in offline mode, on a repository they own or on a shared one, invisibly (§8.17). | Has a private graph fed only by git history and the file tree; uses every local feature. To the team they look like a hold-out. Their agent repairs the graph after teammates' changes. Can share or join at any time. |
| **Quiet participant** | Has not opted in, but followed the team's setup instructions for quiet onboarding (§8.21). | Uses plain git. Their agent runs a locally fetched Resonance CLI on demand to write bookkeeping deltas, resolve `.resonance/` conflicts and add PR summaries. Never blocked. |
| **Hold-out developer** | Works in a repository where others use Resonance, but does not run it, temporarily or for good. | Uses plain git. Never has to install, configure or understand Resonance; their commits are ingested by everyone else's daemons (FR-174, §8.18). |
| **Unaware tools** | Build systems (sbt, Gradle, Maven, pip, Cargo…), IDEs, git, test runners, CI, linters. | See only an ordinary working tree. Must not need to know Resonance exists. |
| **Reviewer** | Reviews changes on a git hosting platform. | Reads code diffs plus a readable summary of graph changes in the same pull request, produced by Resonance tooling rather than by raw diffs of the store (FR-066). |
| **Operator** | Installs and maintains Resonance on a machine. | Uses `resonance doctor`, the registry and logs. |
| **CI / enforcement** | CI jobs that check Resonance's committed data, from stage 2 onward. | Report (stage 2) or reject (stage 3) pull requests that changed files outside Resonance. May run compaction, and may add quiet bookkeeping to non-Resonance pull requests (FR-156, FR-176, FR-236). |
