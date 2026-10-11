## 4. Actors

| Actor | Description | How they interact |
| --- | --- | --- |
| **Intent-level developer** | Works mainly in requirements and design; may rarely read code. | Authors requirements and decisions; reviews traces and verification status. |
| **Code-fluent developer** | Still works in programming languages. | Edits files in an IDE; annotates and calls out code units to agents; reviews generated code. |
| **Agent** | A coding agent such as Claude Code. | Uses the graph API (via MCP) for intent, traces and annotations; may edit files directly or mutate through the API. |
| **Unaware tools** | Build systems (sbt, Gradle, Maven, pip, Cargo…), IDEs, git, test runners, CI, linters. | See only an ordinary working tree. Must not need to know Resonance exists. |
| **Reviewer** | Reviews changes on a git hosting platform. | Reads code diffs plus a readable form of graph changes in the same pull request. |
| **Operator** | Installs and maintains Resonance on a machine. | Uses `resonance doctor`, the registry and logs. |
