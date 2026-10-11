## 16. Glossary

| Term | Meaning |
| --- | --- |
| **FOD** | "Files on disk": the model in which agents and developers work primarily by reading and editing files. |
| **Working tree / replica** | The ordinary files on disk. A replica of the file content held in the graph. |
| **Intent layer** | Requirements, decisions, designs and constraints. |
| **Realization** | The code that currently implements intent. |
| **Authored data** | Anything a person or agent stated directly: intent, links, annotations. Versioned in git. |
| **Derived data** | Anything recomputable from file content: code units, references. Not versioned. |
| **Identity layer** | Stable IDs and the recorded history of renames, moves, splits and merges. |
| **Evidence** | A recorded result of a check, such as a test run, supporting a link. |
| **Link status** | `asserted`, `verified`, `stale` or `orphaned` (§6.5). |
| **Edit algebra** | The set of textual and node-centric operations in §9. |
| **Ingest watermark** | How far disk changes have been applied to the graph. |
| **Self-write suppression** | Ignoring file events caused by the daemon's own writes. |
| **Context pack** | The bundle of intent, links, locations and annotations given to an agent for a task. |
| **Registry** | The system-wide daemon for discovery and health. |
| **Code Unit** | (This needs a definition!) |
