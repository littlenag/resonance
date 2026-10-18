# The TUI and the change log

`resonance tui` is a deliberately simple terminal UI for your spec items: requirements, decisions, designs and constraints. Your edits there, and anywhere else you change the spec, land in a short **change log** that your agent reads and clears.

## The TUI

```
resonance tui
```

The TUI is a client of the daemon, so it stays current as files, git and other tools change the graph.

| Screen | Keys |
| --- | --- |
| **List**: key, type, status, title, links (and how many are stale), and `doc` for items imported from a document | `↑`/`↓` or `j`/`k` move · `enter` details · `/` filter by text · `t` cycle the type filter · `s` cycle the status filter · `esc` clear filters |
| **Details**: every field (including `specStatus` and `alsoDefinedIn`), links with their statuses and latest test results, annotations, and the source document | `j`/`k` or `↑`/`↓` scroll · `pgup`/`pgdn` scroll a page · `n`/`p` next and previous item · `esc` back |
| **Edit** (`e`) | `tab`/`↑`/`↓` move between fields · type to change short fields · `enter` on a long field (requirement text, decision text) opens it in `$VISUAL` or `$EDITOR` · `ctrl-s` or `F2` save · `esc` cancel |
| **Add** (`a`) | `←`/`→` on the type row changes the type · the key is prefilled with the next free key for its prefix (`FR-043`, `ADR-0007`) · an existing key is refused |
| **Retire** (`r`) | Asks first, and shows how many links the item has. Items are retired, never deleted, and their links stay. |
| **Change log** (`l`) | The pending entries, read-only. `j`/`k` or `↑`/`↓` scroll · `pgup`/`pgdn` scroll a page · `esc` back |
| Anywhere | `?` help · `g` reload · `q` quit |

Items imported from a document are owned by that document. For them, `e` and `r` open the document in your editor at the item's line; Resonance picks up your edit when you save. The TUI itself writes no project files.

The same operations are available as commands: `resonance spec list`, `spec show <ref>`, `req add|edit|retire|supersede`, `decision add|edit`, `design add|edit|retire|supersede`, `constraint add|edit|retire|supersede`.

## The change log

People change the spec in the TUI, with the CLI, in imported documents, or by pulling teammates' work. Agents need to know: a new requirement may need code, a changed one may make tests stale. The change log tells them.

```
resonance changes
```

```
  12  2026-10-18 09:14  tui    FR-043 created: Exports are signed  (human:dev@example.com)
  13  2026-10-18 09:20  import FR-007 edited (text): Saving is atomic  (tool:resonance)
  14  2026-10-18 09:41  git    ADR-0007 created: Use SQLite  (human:mate@example.com)
```

Each entry has a sequence number, a time, who made the change, the item, the kind of change (`created`, `edited`, `retired`, `superseded`), the fields that changed, a one-line summary, and where it came from (`tui`, `cli`, `import` or `git`). There's at most one entry per item: a newer change replaces the pending entry with a new one, keeping the item's state from before the first change.

| Command | What it does |
| --- | --- |
| `resonance changes [--since SEQ]` | Pending entries, optionally only those after `SEQ`. |
| `resonance changes show <seq>` | One entry in full, with the item's fields before and after. |
| `resonance changes done <seq>… [--note TEXT]` | Remove handled entries. The changes themselves stay in the graph's history. |

Agents keep, in their own memory, the sequence number of the last entry they read. At the start of work and before git operations they read the entries after it, act on each one, and clear what they've handled. Changes agents make themselves aren't logged, unless you set `changes.includeAgents = true`.

**Keep the log empty before commits, pushes and pulls.** `resonance check` reports pending entries (`R009`), and the agent kit's harness hook stops your agent's own `git commit`, `push` and `pull` until the log is empty, unless you tell it to proceed. Your own git hooks only warn.

The change log is local to your clone and never committed. `resonance status` shows how many entries are pending.
