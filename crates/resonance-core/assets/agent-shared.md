## This working tree: shared graph, opted in (stage {stage}, participation {participation})

The graph is committed with the code in `.resonance/` and is this developer's source of truth. Every graph change is recorded in the open delta (`.resonance/deltas/`), which is committed together with the code it describes. The git hooks check consistency; your duties keep them from ever having to stop anyone.

**Before every commit** (FR-215): make sure no *action required* divergences are open (`resonance_recover`), the change log is empty (`resonance_changes`), then run `resonance check --staged` (or `resonance_check`) and fix what it reports. Stage the open delta with the code it describes. If a finding cannot be fixed safely, stop and ask the developer.

**Before opening or updating a pull request** (FR-216): run `resonance check --range <base>..HEAD`, re-verify links the change made stale (`resonance_test`) or report them, and include `resonance review <base>..HEAD --markdown` in the pull request description.

**During pulls, merges and rebases** (FR-217): never hand-edit `.resonance/graph.db` or delta files. Resolve conflicts in `.resonance/` with `resonance recover` (`resonance recover repair-snapshot` for the snapshot). After every git operation, relay Resonance's *attention* and *action required* items to the developer.

**The change log** (FR-276, FR-277). Keep, in your own memory, the sequence number of the last entry you read. At the start of work and before git operations, read `resonance_changes` after it, act on each entry (implement, update links, re-verify, or tell the developer), and remove handled entries with `resonance_changes_done`. The log must be empty before commits, pushes and pulls; a harness hook stops your `git commit`, `push` and `pull` while entries are pending.

**Never, unless the developer explicitly tells you to** (FR-218): bypass hooks (`--no-verify`), write into `.resonance/`, delete or rewrite sealed deltas, run `resonance share`, `unshare`, `eject` or `compact --commit`, or run a git command that could discard uncommitted graph state (`reset --hard`, `checkout -- .`, `clean`, `stash drop`) without first checking `resonance status` and getting confirmation. When the developer does tell you to proceed past a gate, prefix the command with `RESONANCE_OVERRIDE=1`; the result is still checked.

**Speak plainly** (FR-220): relay Resonance's messages, get confirmation before anything that cannot be undone, and when you decline a request because of a convention, name it and offer the compliant alternative.
