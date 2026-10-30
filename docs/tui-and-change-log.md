# The TUI and the change log

`resonance tui` is a deliberately simple terminal UI for your spec items: requirements, decisions, designs and constraints. The [web UI](web-ui.md) (`resonance ui`) does the same in your browser, with more room. Your edits there, and anywhere else you change the spec, land in a short **change log** that your agent reads and clears.

## The TUI

```
resonance tui
```

The TUI is a client of the daemon, so it stays current as files, git and other tools change the graph.

| Screen | Keys |
| --- | --- |
| **List**: key, type, status, title, links (and how many are stale), and `doc` for items your agent brought in from a document. `•` marks an item with a change your agent hasn't handled yet. | `↑`/`↓` or `j`/`k` move · `enter` details · `/` filter by text · `t` cycle the type filter · `s` cycle the status filter · `esc` clear filters |
| **Details**: every field, links with their statuses and latest test results, annotations, the document and line the item came from, and any pending change | `j`/`k` or `↑`/`↓` scroll · `pgup`/`pgdn` scroll a page · `n`/`p` next and previous item · `esc` back |
| **Edit** (`e`) | `tab`/`↑`/`↓` move between fields · type to change short fields · `enter` on a long field (requirement text, decision text) opens it in `$VISUAL` or `$EDITOR` · `ctrl-s` or `F2` save · `esc` cancel |
| **Add** (`a`) | `←`/`→` on the type row changes the type · the key is prefilled with the next free key for its prefix (`FR-043`, `ADR-0007`) · an existing key is refused |
| **Retire** (`r`) | Asks first, and shows how many links the item has. Items are retired, never deleted, and their links stay. |
| **Change log** (`l`) | The pending entries. `j`/`k` or `↑`/`↓` select · `pgup`/`pgdn` move a page · `d` clear the selected entry · `D` clear them all (asks first) · `esc` back |
| Anywhere | `?` help · `g` reload · `q` quit |

Every item is edited here, in place: the graph is the source of truth. For an item your agent brought in from a document, the edit makes its link to the document stale, and your agent updates the document to match (see [Your spec documents](spec-documents.md)). The TUI itself writes no project files.

Everything the TUI does, your agent can do too: ask it to add, change or retire items, or to show you one.

## The change log

When you change the spec, you usually go on to prompt your agent, and your edit doesn't necessarily show up in git. Teammates' changes arrive when you pull. Either way your agent needs to know: a new requirement may need code, a changed one may make tests stale, a retired one may leave code behind. The **change log** tells it. You can see the pending entries in the TUI (`l`), and `resonance status` shows how many there are:

```
  12  2026-10-18 09:14  tui    FR-043 created: Exports are signed  (human:dev@example.com)
  14  2026-10-18 09:41  git    ADR-0007 created: Use SQLite  (human:mate@example.com)
```

Each entry has a sequence number, a time, who made the change, the item, the kind of change (`created`, `edited`, `retired`, `superseded`), the fields that changed, a one-line summary, and where it came from (`tui` or `git`). There's at most one entry per item: a newer change replaces the pending entry with a new one, keeping the item's state from before the first change.

**Your agent sees the entries without looking for them.** With Claude Code, the agent kit adds the pending entries to your agent's context with every prompt you send. Every Resonance tool result also carries a short reminder while entries are pending, and items with a pending change are flagged, retired ones included. Your agent takes them into account in the task at hand.

**Handled entries are cleared.** Once your agent has handled an entry (implemented it, updated links and tests, updated the document it came from, or judged that nothing is needed), it clears it (`resonance_changes_done`), so that stale entries don't confuse later sessions with what looks like unfinished work. You can clear entries yourself in the TUI too: `d` for one, `D` for all. Clearing an entry removes it from the log only; the change itself stays in the graph's history.

Changes agents make themselves aren't logged, unless you set `changes.includeAgents = true`.

**Keep the log empty before commits, pushes and pulls.** `resonance check` reports pending entries (`R009`), and the agent kit's harness hook stops your agent's own `git commit`, `push` and `pull` until the log is empty, unless you tell it to proceed. Your own git hooks only warn.

The change log is local to your clone and never committed. `resonance status` shows how many entries are pending.
