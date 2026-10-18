# Recovery

Git, the files on disk and the graph can drift apart: a branch is rebased, a file is edited while the daemon is down, a teammate who doesn't use Resonance changes code, a merge goes wrong in `.resonance/`. Resonance notices these **divergences**, classifies them from git's own evidence, repairs what it safely can, and keeps whatever a repair displaced so that you can undo it.

```
resonance recover
```

lists open divergences, offline deltas that lost their commit, and recovery records. `resonance status` shows the same divergences in short.

## Divergence classes

Each divergence has a class and a level: *info* (nothing to do), *attention* (worth a look) or *action required* (a decision is needed; `check` reports it as `R008` and the agent gate stops git commands until it's resolved).

| Class | What happened | What Resonance does |
| --- | --- | --- |
| `C1` | A git operation (checkout, pull, merge, rebase) brought new content. | Applies it as git-originated changes and re-checks links to changed code. *Info.* |
| `C2` | Commits were made without Resonance (a teammate who hasn't opted in, CI). | Ingests their changes with their authors; links to code they changed become stale. *Info.* |
| `C3` | A file changed while Resonance wasn't running. | Offline, or outside governed paths: ingests it. Opted in, inside governed paths: restores the file from the graph and keeps your edit as a recovery record. *Action required* when it restores. |
| `C4` | Git discarded an uncommitted change (`checkout --`, `reset --hard`, `stash`). | Keeps the discarded version as a recovery record. *Attention.* |
| `C5` | A merge changed code that has code units. | Takes git's merged text, re-locates anchors, and puts anything uncertain on the repair list. *Attention.* |
| `C6` | `.resonance/` changed unexpectedly (a bad merge, a hand edit) and the graph would lose operations. | Changes nothing until you run `resonance rebuild` or restore the files. *Action required.* |
| `C7` | A long absence or rewritten history: many commits at once, or offline deltas whose commits are gone. | Catches up in bulk; deltas that lost their commit go to the detached pool. *Attention.* |
| `C8` | A commit bypassed the consistency checks (`--no-verify`). | Caught before push (`R002`). |
| `C9` | An unexplained change. | Reported for a person to look at. |

## Commands

| Command | What it does |
| --- | --- |
| `resonance recover` | List divergences, the detached pool and recovery records. |
| `resonance recover undo <id>` | Put back what a resolution displaced. The version in place is kept too, so undo can itself be undone. |
| `resonance recover dismiss <id> [--note TEXT]` | Mark a divergence handled. For a conflict, this also closes the `Conflict` node. |
| `resonance recover attach <delta>` | Attach a detached offline delta to the current commit. |
| `resonance recover drop <delta> [--yes]` | Export a detached delta to `~/.resonance/exports/`, then remove it. |
| `resonance recover repair-snapshot` | Merge both sides of a conflicted `.resonance/graph.db` mechanically, without the merge driver. |
| `resonance rebuild` | Re-fold the graph from its snapshot and deltas, then bring it up to date. Clears a held `C6`. |
| `resonance restore <file>` | Restore authored data from an export. |
| `resonance backup [--list]` | Back up `.resonance/` to `~/.resonance/backups/`, or list backups. |
| `resonance backup --restore [--from DIR]` | Restore a missing `.resonance/` from the newest backup, or from `DIR`. |

Recovery records live in `.resonance/local/recovery/`, and every graph operation is also appended to a journal in `.resonance/local/journal/`, independent of the working tree.

## What Resonance did for you

```
resonance value
```

summarizes, locally, what Resonance did: traces and context packs served, anchors that followed edits on their own, repairs and their recorded cost, divergences by class, and how well the agent kit works (agent commits checked, how many passed on the first attempt, denials and overrides). Offline, it also says what improves when you share the graph. Nothing leaves your machine.
