# The web UI, diagnostics and logs

`resonance ui` opens a web UI for this repository in your browser. It does everything the [TUI](tui-and-change-log.md) does, with a more interactive flow, and it also shows what the daemon has been doing and its log.

```
resonance ui              # open the web UI
resonance ui FR-042       # open it on one spec item
resonance ui --no-open    # print the address instead of opening a browser
```

The repository's daemon serves the web UI, so it runs for as long as the daemon runs. `resonance ui` starts the daemon first if none is running. Nothing else is installed or started: the page comes from the `resonance` binary itself, and it loads nothing from the internet.

## Only for you, without a login

The web UI listens on `127.0.0.1` only. Each daemon picks its own port, so several repositories can have their UI open at once. A daemon reuses the port its repository had last time when that port is free, so bookmarks and open tabs keep working after a restart. Set `ui.port` in [the configuration](configuration.md) to prefer a particular port.

You don't log in. `resonance ui` hands the browser this repository's key, which the page keeps as a cookie. Requests without that key are refused, as are requests addressed to another host name and changes sent from other web sites. Being on `localhost` alone wouldn't stop other web pages, or other users on the machine, from reaching the UI. The key is kept in `.resonance/local/ui.json`, readable only by you. Delete that file to get a new key the next time the daemon starts.

The address `resonance ui --no-open` prints contains the key. Open it yourself, and don't share it.

## Spec items

The **Spec** page lists your requirements, decisions, designs and constraints beside the selected item. An item with a change your agent hasn't handled yet has an orange row.

- **Search** matches keys, titles and text.
- **Filters** are columns of checkboxes: type, status, change log (pending or not), links (some stale, all current, none) and source (from a document, or graph only). With nothing checked, everything shows. Checking values narrows the list to them; values of one filter are alternatives, and filters narrow each other. Each value shows how many items it would show; hover over one and click **only** to check just that value.
- **Sort** by any column by clicking its header; click again to reverse.

The search, filters and sort are part of the page's address, so you can bookmark a view.

- **Details** show every field (long text as Markdown), links with their statuses and latest test results, annotations, the document and line the item came from, and any pending change.
- **Links** open what they point to: click a code unit, test or document region and a drawer shows the whole file with the linked lines highlighted (or where they were last seen, if Resonance lost track of them). Click a spec item to see it and open it.
- **Add** suggests the next free key for the type (`FR-043`, `ADR-0007`). A key that already exists is refused.
- **Edit** changes the item in place. If someone else changes it while you're editing (a teammate through git, your agent, another window), the form says so, and you choose whether to keep your edits or take the newer version before anything is saved.
- **Retire** asks first, and shows how many links the item has. Items are retired, never deleted, and their links stay.

Every item has its own address (`/spec/FR-042`), which you can bookmark. Your changes are recorded as you, the person named by `git config user.email`, and go to the [change log](tui-and-change-log.md#the-change-log) with origin `ui`. As in the TUI, editing an item that came from a document marks the document for your agent to update. The web UI writes no project files.

The **Change log** page lists the pending entries. Click an entry to see what changed, field by field. You can clear an entry, or clear them all after confirming.

The page stays current on its own: when your agent, git or another window changes the graph, it refreshes.

| Key | |
| --- | --- |
| `/` | Filter spec items by text |
| `j`/`k`, `↓`/`↑` | Next or previous item |
| `a` / `e` / `r` | Add, edit or retire |
| `l` | The change log |
| `d` / `shift+D` | Clear the selected change-log entry, or all of them |
| `⌘K` / `ctrl+K` | Search spec items and actions |
| `⌘S` / `ctrl+S` | Save a form |
| `g` | Reload |
| `?` | Help |

## Diagnostics

The **Diagnostics** page is a view inside the daemon, covering everything since it started:

- **Daemon**: process ID, version, uptime, the repository, the socket and the web UI's address.
- **Health**: the daemon's state, how far behind the disk it is, and its last error (see [The daemon](daemon.md#commands)).
- **Store**: counts of files, spec items, code units, tests and links, the open delta, sealed deltas, pending changes, the repair list and divergences.
- **Activity**: a timeline of what the daemon did. That covers requests (which method, from whom, how long), clients connecting and leaving, reconciles, git operations it caught up with, changes, change-log entries cleared, divergences, deltas sealed and compacted, state changes, and actions taken here. You can filter it by kind and text, and pause it.
- **Counters**: requests per method, with errors and median, 95th-percentile and slowest times.
- **Clients**: who is connected now: agents (MCP), the TUI, commands and web pages.
- **Configuration**: every setting in effect, and whether it comes from the default, `.resonance/config.toml` or an environment variable.

**Export JSON** downloads the page's data, including the events and the log, for a bug report. Events hold keys, paths, counts and timings, never the text of your spec items or files.

The page also offers the daemon's operational actions, the same ones as the CLI's: **Reconcile now**, **Full reconcile**, **Rebuild derived data**, **Restart** and **Stop**, plus the **log level** for this session. Actions that make other clients wait or disconnect them ask first. After a restart the page reconnects on its own. After a stop, run `resonance ui` again; your agent's next request also starts a daemon.

Diagnostics are kept in memory, up to `diagnostics.bufferEvents` events (the oldest are dropped first, and the page says so), and are gone when the daemon exits. The log file is the only record that outlives the daemon.

## Logs

The **Logs** page shows the daemon's log since it started, live. You can filter it by level, component (`daemon`, `ui`, `ingest`, `git` and so on) and text, pause it, copy what's shown, or export it. **Daemon level** changes how much the daemon logs until it stops, without changing your configuration.

The same log goes to a file, `.resonance/local/daemon.log` by default. Choose another with `log.file` or `RESONANCE_LOG_FILE`, the level with `log.level` or `RESONANCE_LOG` (for example `info,ingest=debug`), and plain text or JSON lines with `log.format` (see [Configuration](configuration.md)). Past `log.maxBytes` the file moves to `<file>.1` and a new one starts. A log file inside your working tree, outside `.resonance/local/`, is refused, because Resonance would read it and it would show in `git status`. The daemon logs to the default file instead and says why.

## Building it from source

The web UI is written in TypeScript with React and built ahead of time. `just build` builds it (it needs Node and npm) and embeds it in the binary. A `resonance` built with plain `cargo build` serves a page that says to run `just ui-build` first. Everything else works without it.
