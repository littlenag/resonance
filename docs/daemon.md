# The daemon

Each repository that uses Resonance gets a background process, the **daemon**. It watches the working tree, applies every change to the graph as it happens, and answers your agent's tool calls, the TUI and every `resonance` command. You don't need to manage it. This page explains what it does, so you know what to expect and where to look when something seems off.

## What it does for you

- **Answers reflect the disk.** Before answering, the daemon makes sure it has seen every file change made before your request, including saves a moment earlier from any editor, agent or script. You never see a stale answer.
- **Answers are fast.** The graph is kept current in the background, so a request only has to apply the last few changes, not re-read the repository.
- **Git operations are applied as one batch.** While a git command is changing files (a checkout, pull, rebase or merge), the daemon holds the changes and applies them together when git finishes, so a branch switch is one change rather than hundreds of separate edits.

## Life cycle

- **Started on demand.** The first request in a repository (from your agent, the TUI or a `resonance` command) starts its daemon and waits until it's ready. A daemon that starts up first brings the graph up to date with everything that changed while none was running.
- **One per repository.** The running daemon is the process that holds the lock inode at `.resonance/local/daemon.lock`, or the one that answers on this repository's socket. A second daemon exits immediately. Deleting the lock file, the socket, or both does not make a running daemon gone: it keeps the store until it has reclaimed those files or stood down, and it never removes a socket a successor is listening on.
- **Exits when idle.** After 30 minutes without requests the daemon exits. Change this with `daemon.idleSeconds` in [the configuration](configuration.md).
- **Replaced after an upgrade.** If you install a new build of `resonance`, the next command stops the daemon from the old build and starts a new one.
- **Recovers from crashes.** If the daemon is killed or crashes, the next command starts a new one, which catches up with the disk before answering. A missing daemon only ever costs a short delay, never a wrong answer.
- **Stops when Resonance is removed.** If `.resonance/` disappears, the daemon exits.

## Commands

```
resonance daemon              # start the daemon if needed and show its health
resonance daemon start        # start it
resonance daemon stop         # stop it
resonance daemon restart      # stop it and start a new one
```

```
$ resonance daemon
Daemon for /home/you/inventory (pid 11866): ready
  endpoint: /tmp/resonance-you/697234d03201dae4.sock
  started: 2026-10-11T23:51:53.626Z; version 0.1.0
  watermark: 28 of 28 events applied; 0 pending
  last ingest: 2026-10-11T23:51:54.149Z
  1 connected, 0 subscribed, 3 requests served
```

The daemon reports one of these states:

| State | Meaning |
| --- | --- |
| `starting` | It has just started. |
| `reconciling` | It's bringing the graph up to date with the disk. |
| `ready` | It's up to date and answering. |
| `git-operation` | A git command is changing files. Changes are held until it finishes. |
| `degraded` | Something is wrong, for example a `.git/index.lock` file left behind by a git command that crashed. Its last error says what. |

The *watermark* shows how many file events the daemon has seen and applied. *Pending* events are seen but not yet applied.

## Global options

**`--cached`** answers from the graph as it is, without waiting for recent changes to be applied. Use it when you want an instant answer and don't mind missing the last moment's edits.

**`--no-daemon`** (or `RESONANCE_NO_DAEMON=1`) runs the command in its own process instead of through the daemon. The command reads the disk itself to bring the graph up to date, then answers. Answers are the same either way; this mode is for debugging, and for environments where a background process can't run.

## Git operations

While `.git/index.lock` exists, a git command is changing the working tree. The daemon reports the state `git-operation` and applies the held changes as one batch when the lock disappears. A request made during a git operation waits for it, for at most 10 seconds. A lock older than that is treated as left over from a crashed git command, and the daemon reports `degraded` and carries on. If that happens, check that no git command is running and remove `.git/index.lock`, as git itself would tell you to.

## Which files the daemon watches

The daemon watches the files Resonance reads (see [Configuration](configuration.md#which-files-are-read)), plus any `.gitignore` file, `.git/info/exclude` and `.resonance/config.toml`. A change to any of those last three makes it re-check the whole repository, since a different set of files may now be read.

On macOS the daemon uses one FSEvents stream for the whole repository. On Linux it uses an inotify watch for each directory it reads, skipping ignored directories such as `node_modules/` and `target/`. If the system reports that it dropped events, the daemon re-checks the whole repository before answering.

## Tests run inside the daemon

Your agent's `resonance_test` runs test commands in the daemon. The commands run from the repository root and inherit the daemon's environment, which comes from the command that started the daemon. `RESONANCE_*` variables are the exception: they're passed along with each command.

If a test needs something from your shell, such as an activated virtual environment, a `PATH` entry or other variables, either make the run command self-contained (`.venv/bin/python -m pytest …`), or restart the daemon from a shell that has them:

```
resonance daemon restart   # from a shell with the environment the tests need
```

## Health of every daemon: doctor

```
$ resonance doctor
resonance 0.1.0 (runtime directory /tmp/resonance-you)
This repository: /home/you/inventory (schema 1, excluded from git, daemon running)
Registry: not running (daemons are found through daemon.json)
Daemons:
  /home/you/inventory (pid 11866): ready, ready, 0 pending, 0 ms behind
No problems found.
```

`doctor` checks this repository (that the graph opens and passes an integrity check, its schema version, and that `.resonance/` is still excluded from git) and every daemon it knows about on this machine: whether it answers, its state, how far behind the disk it is, its last error, and whether it runs a different build of Resonance than yours. It also reports a config that still has `intent.sources` or `ingest.sourceTags`, and spec items that still have a legacy `source` property and no document link: those came from the removed importer, and one `resonance_import_items` call replaces them. It lists any problems it finds. Run it from inside a repository, or from anywhere to see only the daemons.

## The registry (optional)

The registry is a small per-user service that tracks every Resonance daemon on your machine, for `doctor`. It never sits between you and a daemon, never starts or restarts one, and nothing depends on it. Without it, `doctor` finds daemons through a list of repositories that have run one (`~/.resonance/known-roots`).

Run it in a terminal:

```
resonance registry run
```

Or install it as a user service (a launchd agent on macOS, a `systemd --user` unit on Linux), so it starts when you log in:

```
resonance registry install --dry-run   # show the service definition and commands
resonance registry install             # asks first; --yes to skip the question
resonance registry uninstall           # stop and remove it
```

Daemons register with the registry when they start, renew their entry every 10 seconds, and unregister when they exit. An entry that isn't renewed for 30 seconds expires. A restarted registry rebuilds its list from `~/.resonance/known-roots`.

## Files

| Path | What |
| --- | --- |
| `.resonance/local/daemon.lock` | Held by the running daemon. |
| `.resonance/local/daemon.json` | The running daemon's socket, process ID, versions and start time. Clients find the daemon through it. |
| `.resonance/local/daemon.log` | The daemon's diagnostics. Look here first when the daemon won't start. |
| `.resonance/local/cookies/` | Short-lived marker files the daemon uses to know it has seen every change made before a request. |
| runtime directory | The daemons' sockets (`<hash>.sock`) and the registry's (`registry.sock`). |
| `~/.resonance/known-roots` | Repositories that have run a daemon. |

The **runtime directory** holds Unix sockets, which must have short paths. It's the first of these that applies:

1. `$RESONANCE_RUNTIME_DIR`, if set;
2. `$XDG_RUNTIME_DIR/resonance`, if `XDG_RUNTIME_DIR` is set;
3. `~/.resonance/run`;
4. `/tmp/resonance-<user>`, when the path above would be too long for a socket.

The directory and the sockets are readable only by you. The daemon has no network listener.
