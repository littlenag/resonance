# Sharing the graph with your team

Resonance starts **offline**: the graph is private to you, and git never sees it. When your team is ready, you can **share** the graph, so that it's committed with the code, reviewed in pull requests, and merged like everything else. Each step is reversible, and none of them loses data.

## Stages and participation

| | What it means |
| --- | --- |
| **Stage 1, offline** | The graph lives in `.resonance/`, excluded from git. Only you see it. Resonance follows git and your files, and repairs itself after pulls and checkouts. |
| **Stage 2, shared** | `.resonance/` is committed: a snapshot (`graph.db`), small change files (`deltas/`) and `config.toml`. Everyone who clones the repository gets the graph. |

At stage 2, each developer's **participation** is local to their clone:

- **Opted in** (`user`): Resonance is your source of truth. Your graph changes are recorded in an open delta that you commit with the code it describes, and git hooks check that your commits are consistent.
- **Not opted in**: you work with git as usual. Resonance never blocks you. If you also use Resonance offline (`init --offline`), your private graph reads the shared one and never writes it.

`resonance status` shows the stage and your participation.

## Sharing: stage 1 to stage 2

```
resonance share --dry-run
resonance share
```

`share` first shows what would be committed: every kind of item with counts, and the full text of every annotation. Keep anything private with `--keep-local <annotation-id>`; it stays in `.resonance/local/private/`, only on your machine.

When you confirm, `share`:

1. backs up `.resonance/` to `~/.resonance/backups/`;
2. compacts your history into a fresh snapshot, `.resonance/graph.db`;
3. writes `.resonance/config.toml` with `stage = 2`, a `README.md` for teammates, a `.gitignore` that keeps `.resonance/local/` out of git, and a `.gitattributes` that names Resonance's merge and diff drivers;
4. removes `/.resonance/` from `.git/info/exclude`;
5. opts you in, registers the drivers in your local git configuration, and installs the git hooks.

It commits nothing. Review the new files and commit them yourself:

```
git add .resonance && git commit -m "Share the Resonance graph"
```

## Joining: a teammate's clone

In a repository with a shared graph, run:

```
resonance init
```

This **joins**: Resonance rebuilds the graph from the committed snapshot and deltas, opts you in, installs the hooks and drivers, and changes no tracked file. If you used Resonance offline in this clone before, your offline work joins the shared graph as new deltas, to commit with your next commit.

To try Resonance privately instead, without opting in:

```
resonance init --offline
```

## What happens on commit, push and pull

Once you've opted in, every change you make to the graph (a link, an annotation, a requirement) goes into the **open delta**, `.resonance/deltas/<id>.delta`, a small JSON Lines file. Commit it with the code it describes; the `pre-commit` hook stages it for you when your staged code needs it.

The hooks:

| Hook | What it does |
| --- | --- |
| `pre-commit` | Runs `resonance check --staged`. Errors stop the commit, each with its fix. |
| `pre-push` | Runs `resonance check --range` on the commits being pushed, and catches commits made with `--no-verify`. |
| `post-checkout`, `post-merge`, `post-rewrite` | Tell the daemon that history moved, so the graph follows. |

If a hook file already exists, Resonance appends a marked block to it (between `# >>> resonance >>>` and `# <<< resonance <<<`) and never touches the rest. If a hook manager such as husky, pre-commit or lefthook owns your hooks, Resonance writes nothing and prints the lines to add instead.

Teammates who haven't opted in are never blocked. Their commits are read as outside changes: links to code they changed become stale, as they would for your own edits.

## Checks

```
resonance check --staged
resonance check --range origin/main..HEAD
```

Each finding has a code, a severity (`error`, `attention` or `info`), a message and a fix. The exit code is 3 when any finding is an error.

| Code | Finding |
| --- | --- |
| `R001` | The open delta isn't staged with the code it describes. |
| `R002` | A staged or committed file changed without Resonance seeing it (hooks bypassed, or the daemon was down). |
| `R003` | Something under `.resonance/local/` is staged. |
| `R004` | A compaction is staged on a branch other than the trunk. |
| `R005` | A sealed delta or the snapshot was modified. |
| `R006` | Unresolved `Conflict` nodes. |
| `R007` | The branch will conflict in `.resonance/` when merged into the trunk. |
| `R008` | Divergences that need a decision (see [Recovery](recovery.md)). |
| `R009` | Change-log entries are pending (see [The TUI and the change log](tui-and-change-log.md)). |
| `R010` | The agent kit is missing or stale (see [Working with agents](agents.md)). |

## Pull requests

```
resonance review origin/main..HEAD --markdown
```

summarizes the graph changes in a range: intent added, changed and retired; links added and removed, and status changes; annotations; identity operations; conflicts; and compactions. Paste it into the pull request description. In `git diff` and `git log -p`, deltas are shown readably too (`resonance show-delta` is registered as their diff driver).

## Merging and compaction

Deltas are new files, so branches that both change the graph merge without conflicts in `.resonance/`. When both sides changed the same item in incompatible ways, the result is a `Conflict` node, listed by `resonance recover` and `check` (`R006`), never a broken merge.

Deltas accumulate. **Compaction** folds them into a new snapshot:

```
resonance compact --commit
```

Compact only on the trunk (`compaction.trunk`, by default the remote's default branch). Resonance compacts on its own when the trunk has more than `compaction.maxDeltas` sealed deltas. Branches made before a compaction still merge cleanly. If two compactions collide, the snapshot merge driver combines them; without the driver, `resonance recover repair-snapshot` does the same after the merge.

## Stepping back

| Command | What it does |
| --- | --- |
| `resonance leave` | Stops being a Resonance user in this clone: removes the hook blocks, the agent kit and your opt-in. The open delta must be committed first, or written elsewhere with `--export FILE`. Your local graph stays; `resonance init` joins again. |
| `resonance unshare` | Takes the repository back to stage 1: exports the graph, then leaves the removal of the committed `.resonance/` files as working-tree changes for you to commit, and restores the local exclusion. |
| `resonance eject` | Removes Resonance from this clone entirely, after an export. |

None of these lose authored data: everything you and your agents recorded is in the export, the backup, or the committed history.
