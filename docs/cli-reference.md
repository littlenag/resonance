# CLI reference

```
resonance [GLOBAL OPTIONS] <COMMAND> [ARGS]
```

The `resonance` command is for setting Resonance up, running its daemon, checking on it and repairing it. Graph work (bringing in your spec, declaring code and tests, linking, annotating, tracing, verifying) is done by your agent through the MCP server, and by you in `resonance tui`; see [Working with your agent](agents.md). `resonance help <command>` and `resonance <command> --help` show the same information in your terminal.

## Global options

| Option | Meaning |
| --- | --- |
| `-C <path>` | Run as if Resonance was started in `<path>`. |
| `--json` | Print machine-readable JSON instead of text. |
| `--cached` | Answer from the graph as it is, without waiting for recent file changes to be applied. |
| `--no-daemon` | Run in this process instead of through the daemon (also `RESONANCE_NO_DAEMON=1`). |
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version. |

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | An error. |
| `2` | A usage error: a missing or invalid argument, or an unknown command. |
| `3` | `check` (or a git hook) found a consistency error. |

---

## Setup and life cycle

### init

```
resonance init [--agent claude-code|generic|none | --no-agent] [--offline]
```

Sets up Resonance in the current repository:

1. mirrors the repository into the graph in `.resonance/` (in offline mode, excluded through `.git/info/exclude`);
2. installs the git hooks for this working tree: the non-blocking `post-checkout`, `post-merge` and `post-rewrite` offline, plus the checking `pre-commit` and `pre-push` once you've opted in to a shared graph;
3. installs the [agent kit](agents.md): for Claude Code (the default), the MCP server at local scope, a skill, a harness hook and a `CLAUDE.local.md` section; with `--agent generic`, it prints the MCP command line and an instructions section for any other agent; `--agent none` (or `--no-agent`) skips it.

It reads no requirements: your agent brings your spec in (see [Your spec documents](spec-documents.md)). It changes no tracked file. Running it again refreshes the hooks and the kit.

In a repository whose graph is shared, `init` **joins** it instead: it rebuilds the graph from the committed snapshot and deltas, and opts you in. `--offline` tries Resonance privately over the shared graph instead. See [Sharing the graph](sharing.md).

### status

```
resonance status
```

The stage and your participation, counts, unresolved divergences, the repair list, pending change-log entries and the agent kit's state.

### doctor

```
resonance doctor
```

Checks this repository (the graph's integrity and schema version, the git exclusion, and the agent kit) and the health of every Resonance daemon on this machine. See [The daemon](daemon.md#health-of-every-daemon-doctor).

### tui

```
resonance tui
```

A terminal UI to list, view, edit, add and retire spec items, and to see the change log. See [The TUI and the change log](tui-and-change-log.md).

### ui

```
resonance ui [<key>] [--no-open]
```

Opens this repository's web UI in your default browser (`$BROWSER` if set), starting the daemon if needed: spec items, the change log, diagnostics and logs. With a key, it opens that spec item. `--no-open` prints the address instead, and `--json` gives `url`, `address`, `port` and `root`. The address carries this repository's key, so don't share it. Not available with `--no-daemon`. See [The web UI, diagnostics and logs](web-ui.md).

### share, unshare, leave

```
resonance share [--dry-run] [--yes] [--keep-local <annotation>…]
resonance unshare [--yes] [--out FILE]
resonance leave [--export FILE]
```

`share` takes the repository from offline to a shared graph: it previews everything that would be committed, then writes the snapshot and the shared files under `.resonance/`, adds the agent kit's section to the shared `CLAUDE.md`, opts you in, and installs the git hooks and drivers. It commits nothing. `--keep-local` keeps annotations private.

`unshare` takes it back: it exports the graph, leaves the removal of the committed `.resonance/` files for you to commit, and restores the local exclusion.

`leave` stops you being a Resonance user in this clone: it removes the hook blocks, the agent kit and your opt-in. The open delta must be committed, or written to `FILE` first. See [Sharing the graph](sharing.md).

### eject

```
resonance eject [--dry-run] [--yes] [--out FILE]
```

Removes Resonance from the repository:

1. exports the graph to `FILE`, by default `~/.resonance/exports/<repository>-<time>.jsonl`;
2. stops the daemon;
3. removes Resonance's blocks from the git hooks, and the agent kit's local files and settings;
4. removes `.resonance/`;
5. removes the `/.resonance/` line from `.git/info/exclude`.

It asks before removing anything; `--yes` skips the question, and `--dry-run` only shows the plan.

### version

```
resonance version [--json]
```

The binary's version, the graph's schema version, and the MCP protocol versions supported.

---

## The daemon

```
resonance daemon [status]
resonance daemon start|stop|restart
```

With no subcommand (or `status`), starts the repository's daemon if needed and shows its health. `start`, `stop` and `restart` do what they say. Every command, the TUI and your agent's MCP server start the daemon on demand, so you rarely need these. See [The daemon](daemon.md).

---

## Checks and repair

### check

```
resonance check --staged
resonance check --range <a>..<b>
```

Consistency checks, with findings `R001` to `R010`. Exits with 3 when any finding is an error. The `pre-commit` and `pre-push` hooks run these, and so does your agent. See [Sharing the graph](sharing.md#checks).

### recover

```
resonance recover
resonance recover undo <id>
resonance recover dismiss <id> [--note TEXT]
resonance recover attach <delta>
resonance recover drop <delta> [--yes]
resonance recover repair-snapshot
```

Lists divergences, the detached pool and recovery records; puts back what a resolution displaced; marks a divergence handled; attaches or drops a detached offline delta; or repairs a conflicted snapshot after a merge. See [Recovery](recovery.md).

### reconcile, rebuild, rebuild-derived

```
resonance reconcile [--full]
resonance rebuild
resonance rebuild-derived
```

`reconcile` brings the graph up to date with the disk now; every request already does this, so you rarely need it. `--full` re-reads every file, ignoring sizes and modification times. `rebuild` re-folds the graph from its snapshot and deltas, then brings it up to date (use it after a `C6` hold). `rebuild-derived` drops the data Resonance computes from your files and rebuilds it; nothing authored is touched.

### export, restore, backup

```
resonance export [--out FILE]
resonance restore <file>
resonance backup [--list]
resonance backup --restore [--from DIR]
```

`export` writes everything you and your agents authored as JSON Lines: a header line, then one operation per line that recreates the current state. `restore` reads such an export back; operations for things that already exist are skipped. `backup` copies `.resonance/` to `~/.resonance/backups/`, lists backups, or restores a missing `.resonance/` from the newest backup (or from `DIR`).

---

## Services

### mcp

```
resonance mcp
```

Runs the MCP server over standard input and output. Your agent's harness starts this; you don't run it by hand.

### registry

```
resonance registry run
resonance registry install [--dry-run] [--yes]
resonance registry uninstall [--dry-run] [--yes]
```

Runs the optional registry of daemons, or installs or removes it as a user service (launchd on macOS, `systemd --user` on Linux). `install` and `uninstall` show what they'll do and ask first. See [The registry](daemon.md#the-registry-optional).

### Plumbing

`resonance hook <name>`, `resonance agent-hook pre-git`, `resonance git-driver merge` and `resonance git-driver textconv` are called by git and by your agent's harness. They are hidden from `--help`, and you don't need to run them.
