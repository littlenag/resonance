# Troubleshooting

Start with:

```
resonance doctor
resonance status
```

`doctor` checks the graph and the daemons; `status` shows the repair list. If the daemon is involved, its log is `.resonance/local/daemon.log`.

## Common problems

### "not inside a git repository"

Resonance only runs inside a git repository. Run it from within one, or pass `-C <path>`. If the directory isn't a repository yet, run `git init` first.

### My agent says a unit is orphaned, but its code is still there

The code changed more than Resonance would follow safely, for example a function that was rewritten, or split and moved in one step. Ask your agent to re-anchor the unit to where the code is now ("Work through the Resonance repair list"). Its links and annotations come back with it.

### A unit is attached to the wrong code

A low-confidence match appears on the repair list, but a wrong match with high confidence doesn't. Tell your agent where the code really is, and it re-anchors the unit. Units anchored on a short, distinctive snippet rather than a range of lines are the most precise for small regions.

### My agent says something isn't a file Resonance tracks

The file is missing, ignored by git, or excluded by the built-in or configured ignores (see [Which files are read](configuration.md#which-files-are-read)). Check the path, `.gitignore` and `ingest.ignore`.

### A test passes in my shell but fails or errors when my agent runs it

Tests run in the daemon, from the repository root, with the environment of the command that started the daemon (see [Tests run inside the daemon](daemon.md#tests-run-inside-the-daemon)). Check that:

- the run command works from the repository root;
- it doesn't depend on an activated virtual environment, an alias or shell function, or variables that the daemon doesn't have. Make the command self-contained, or restart the daemon from your current shell with `resonance daemon restart`;
- its output doesn't contain `test.zeroTestsPattern`, which marks the run as `error`;
- it finishes within `test.timeoutSeconds`.

The last 4 KiB of each run's output are recorded with its evidence; ask your agent for them.

### The daemon won't start

The error includes the end of `.resonance/local/daemon.log`. Common causes:

- **"path must be shorter than SUN_LEN"**: the socket path is too long. This happens when `RESONANCE_RUNTIME_DIR` (or `XDG_RUNTIME_DIR`) points to a deeply nested directory. Unset it, or point it at a short path such as `/tmp/resonance-$USER`.
- **The runtime directory isn't writable.** Check its permissions, or set `RESONANCE_RUNTIME_DIR` to a directory you own.

While you investigate, `--no-daemon` runs any command without the daemon.

### The daemon reports `degraded`

Run `resonance daemon` to see the last error. The most common cause is a `.git/index.lock` file left behind by a git command that crashed. If no git command is running, remove the file. The daemon recovers by itself.

### Answers seem out of date

They shouldn't be: every answer waits until the daemon has applied every change made before the request. Check that you didn't use `--cached`. If an answer is still wrong, compare it with `resonance --no-daemon <command>`, which re-reads the disk itself. If the two differ, run `resonance reconcile --full` and report the problem, with `.resonance/local/daemon.log`.

### `git status` shows `.resonance/`

The exclusion line was removed from `.git/info/exclude`. `resonance doctor` reports this. Run `resonance init` to restore it; this changes nothing else.

### Lots of things orphaned after switching branches

Units whose code doesn't exist on the current branch are orphaned until you switch back, and then they're restored automatically. Only repair them if the code is really gone.

## Where your data lives

| Location | What |
| --- | --- |
| `.resonance/local/state.db` | The live graph. A SQLite database, never committed. |
| `.resonance/config.toml` | This repository's settings. |
| `.resonance/graph.db`, `.resonance/deltas/` | Once the graph is shared: the committed snapshot and the change files. |
| `.resonance/local/private/` | Offline history, and annotations kept private when sharing. |
| `.resonance/local/journal/`, `.resonance/local/recovery/` | Every graph operation, and what recoveries displaced. |
| `.resonance/local/daemon.*`, `.resonance/local/cookies/` | The daemon's lock, endpoint, log and markers. |
| `.resonance/local/agent-kit.json` | The installed agent kit. |
| `.git/info/exclude` | One line, `/.resonance/`, so git ignores the directory. |
| runtime directory | Daemon sockets (see [The daemon](daemon.md#files)). |
| `~/.resonance/known-roots` | Repositories that have run a daemon. |
| `~/.resonance/exports/` | Exports written by `resonance eject`, `unshare` and `recover drop`. |
| `~/.resonance/backups/` | Backups of `.resonance/`, written by `resonance backup`, `share` and layout migrations. |

Resonance never writes anywhere else. It never changes your source files or their timestamps, never holds locks on them, and has no telemetry or network listener. Outside the repository, only `resonance init` (the agent kit's skill and MCP registration, which it lists) and `resonance registry install` (which asks first) change anything.

To keep a copy of everything you've authored, run `resonance export --out <file>`.

## Removing Resonance from a repository

```
resonance eject --dry-run   # see what will happen
resonance eject
```

`eject` writes an export of the graph to `~/.resonance/exports/`, stops the daemon, removes `.resonance/`, and removes its line from `.git/info/exclude`. It asks before removing anything. No other file is touched, so the repository is exactly as it would be without Resonance.

You can also delete `.resonance/` by hand. Your project keeps working; you only lose the graph.

To remove the Claude Code registration, run `claude mcp remove --scope local resonance` in the repository. To remove the registry service, run `resonance registry uninstall`.
