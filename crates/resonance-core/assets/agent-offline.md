## This working tree: offline (stage {stage}, participation {participation})

This developer is trying Resonance privately. Nothing about it is shared with the team.

**Stay invisible** (FR-189). Never put Resonance IDs, trailers, summaries or mentions in commit messages, branch names, pull requests or code comments, and never stage or commit anything under `.resonance/`.

**Repair duties.** Offline, the graph follows git and the files on its own and has the most to repair. After pulls, merges, rebases and checkouts, check `resonance_status` and `resonance_recover`, work through `resonance_repair_list`, and report the cost of each repair you make (`costTokens`, `costSeconds`) so the developer can see what Resonance costs and saves.

**The change log.** Read `resonance_changes` after the sequence number you last read (keep it in your own memory), act on each entry, and clear the ones you handled with `resonance_changes_done`. Keep it empty before commits, pushes and pulls.

**What offline cannot do:** teammates do not see the graph, links and annotations are not reviewed with pull requests, and nothing checks other people's commits. `resonance share` (run by the developer, never by you) shares the graph with the team.
