## 9. Edit algebra

Edits are **textual** or **node-centric**, with a git-friendly algebra. [Decided] This section proposes that algebra. [Proposed throughout]

### 9.1 Operation envelope

Every operation carries:

| Field | Purpose |
| --- | --- |
| `opId` | Globally unique, sortable ID (ULID or UUIDv7) |
| `kind` | Operation type (below) |
| `actor` | Who made it: human, agent or tool, with an identifier |
| `hlc` | Hybrid logical clock timestamp, for ordering and last-writer-wins |
| `changeSet` | Groups operations applied atomically |
| `schema` | Schema version the operation was written against |
| `payload` | Kind-specific data |

### 9.2 Textual operations

| Operation | Meaning | Committed as an op? |
| --- | --- | --- |
| `file.put(path, blob)` | Set file content | No: git carries file content (FR-064) |
| `file.delete(path)` | Remove file | No |
| `file.move(from, to)` | Move or rename a file | Only when authored data depends on the file's identity (FR-036) |

Textual operations are recorded locally (for undo, history and self-write suppression) but are not the committed record of file content.

### 9.3 Node-centric operations

| Operation | Semantics under concurrent edits |
| --- | --- |
| `node.create(id, type, props)` | IDs are unique, so creates never conflict |
| `node.set(id, key, value)` | Last-writer-wins register per property, ordered by `hlc` then `opId` |
| `node.unset(id, key)` | Same register semantics |
| `node.retire(id)` | Tombstone; the node stays for history and link resolution |
| `edge.add(id, type, src, dst, props)` | Observed-remove set: a concurrent add and remove of different instances both take effect |
| `edge.remove(id)` | Removes only the instances the remover had observed |
| `edge.set(id, key, value)` | Last-writer-wins register per property |

### 9.4 Identity operations

| Operation | Meaning |
| --- | --- |
| `identity.rename(id, newQualifiedName)` | Same node, new name |
| `identity.move(id, newLocation)` | Same node, new file or parent |
| `identity.split(id, [newIds])` | One node became several; links need re-targeting |
| `identity.merge([ids], newId)` | Several nodes became one |
| `identity.retarget(edgeId, newDst)` | Re-attach a link or annotation, for example after orphaning |

Concurrent identity operations that cannot be reconciled (for example, one branch splits a node while another renames it) MUST produce a `Conflict` node rather than guess. The literature on move operations in replicated trees (Kleppmann et al.) is directly relevant here.

### 9.5 Evidence operations

| Operation | Meaning |
| --- | --- |
| `evidence.record(target, kind, result, commit, tool)` | Record the outcome of a check, such as a test run, against a link or node |

Whether evidence is committed or kept local is [Open] (OQ-09). Proposed: keep full run records local; commit only the latest verification summary per link.

### 9.6 Folding, ordering and compaction

- **FR-150** The authored graph MUST be a deterministic fold over its operations: the same set of operations always yields the same graph. [Proposed]
- **FR-151** Operation order for folding: topological order of the git commits that introduced them, then `hlc`, then `opId`. [Proposed]
- **FR-152** Compaction MUST be deterministic, so that two clones compacting the same history produce identical snapshots. [Proposed]
- **FR-153** The committed serialization MUST be canonical (stable key order, normalized numbers and strings) so that identical operations produce identical bytes. Whether the committed form is text (for example JSON Lines) or a compact binary format is [Open] (OQ-02). Text is recommended for pull-request review. [Proposed]
