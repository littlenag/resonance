# Testing Resonance

This is for people and agents changing the code. The user guide starts at [the docs index](README.md). The behavior under test is [spec/mvp.md](../spec/mvp.md).

## Run the suite

Recipes live in the [`justfile`](../justfile). They use the committed `Cargo.lock` and the toolchain in `rust-toolchain.toml`.

| Command | What it runs |
| --- | --- |
| `just test` | `cargo test --workspace --all-features --locked`. CLI tests go through a per-repository daemon. |
| `just test-direct` | The same CLI tests with `RESONANCE_TEST_DIRECT=1`, so each command reconciles in-process (`--no-daemon`). |
| `just check` | Format, clippy, both test modes, the crate boundary, and `just spec-check`. |
| `cargo test -p resonance-cli --test git_ux --locked` | One acceptance file. Add `-- --nocapture` to see failure output. |

CLI acceptance tests are in `crates/resonance-cli/tests/`. They build a throwaway git repository (`TestRepo` in `tests/common/mod.rs`) and run the `resonance` binary plus real `git`. A test must pass in both daemon mode and direct mode. `TestRepo` stops its daemon on drop.

Anchor scenarios that do not need git are in `crates/resonance-core/tests/anchors.rs`. The store contract is `crates/resonance-core/src/store/conformance.rs`, run by both store crates.

## Code coverage

Coverage is line coverage from `cargo-llvm-cov` (LLVM source-based instrumentation). It is a review stat, not a gate: `just check` does not run it, and there is no minimum percentage.

Install once, into the toolchain pinned by `rust-toolchain.toml`:

```
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

Then:

| Command | Result |
| --- | --- |
| `just coverage` | Per-file table on stdout, with a TOTAL row. |
| `just coverage html` | Same data as HTML at `target/llvm-cov/html/index.html`. |
| `just coverage lcov` | `target/llvm-cov/lcov.info` for an external viewer. |

`scripts/coverage.sh` runs the same suite as `just test` (workspace, `--all-features`, daemon mode), then `resonance-cli` again with `RESONANCE_TEST_DIRECT=1`, and reports the union. It cleans `cargo llvm-cov` artifacts first. Extra arguments are cargo-llvm-cov test arguments. Through `just`, put them after `--`: `just coverage summary -- -p resonance-cli --test git_ux`. The script itself takes them directly: `scripts/coverage.sh summary -p resonance-cli --test git_ux`. Passing `-p` replaces `--workspace`, because Cargo rejects the two together. A bare `--test git_ux` fails, because other crates have no test target by that name. A test-name filter is a test-binary argument and goes after a second `--`: `scripts/coverage.sh summary -p resonance-core --lib -- templates`. The HTML and LCOV files are build output under `target/` and are not committed.

How to read the summary:

- The table is one row per source file. **Lines** / **Cover** is the percentage of instrumented lines that ran. Compare the TOTAL row across runs.
- A high TOTAL can still miss a whole command path. Open the HTML and look at the files named in a review.
- `resonance-store-sqlite` and `resonance-store-memory` share the conformance suite. A gap in one of them is usually an engine-specific branch, not a missing scenario.
- Direct mode and daemon mode share most library code. A line that runs only inside the daemon (socket accept, watcher, idle exit) stays uncovered if you measure only `just test-direct`.

## Git workflow tests

Resonance has to keep working when people use ordinary git (FR-071, MVP-029, MVP-030). The tests below drive real git commands against a `TestRepo` and then ask the graph a question. "Offline" means stage 1 (nothing under `.resonance/` is committed). "Shared" means stage 2 after `resonance share`.

| Workflow | Test | Mode |
| --- | --- | --- |
| Edit, commit, switch branch, switch back | `offline_graph_changes_follow_branches` in `git_offline.rs` | Offline |
| Commit on a branch, check out the old commit, commit on main, come back | `a_round_trip_to_the_recorded_commit_seals_the_branch_delta` | Offline |
| Uncommitted graph changes survive `checkout` | `uncommitted_graph_changes_travel_with_checkouts` | Offline |
| `commit --amend` and `rebase` | `amended_and_rebased_commits_keep_their_graph_changes` | Offline |
| `reset --hard` drops a commit's delta into the detached pool; `recover attach` puts it back | `a_dropped_commit_leaves_its_delta_in_the_detached_pool` | Offline |
| `checkout -- path` discards an edit; `recover undo` restores it | `an_edit_discarded_by_git_can_be_put_back` | Offline |
| `pull` of a teammate's commits | `a_pull_with_teammates_commits_is_classified` | Offline |
| Two branches that both change the graph merge | `branches_that_both_change_the_graph_merge_without_conflicts` in `shared.rs` | Shared |
| Hold-out commits, hook bypass, `push` | `a_hold_outs_commits_are_ingested_and_never_blocked`, `inconsistent_commits_are_blocked_and_bypasses_caught_before_push` | Shared |
| Compaction while a branch is in flight, and two compactions colliding | `compaction_on_the_trunk_and_colliding_compactions` | Shared |
| Pull, edit, commit, push, and the other side pulls | `pull_edit_commit_push_round_trip` in `git_ux.rs` | Shared. Checks the file and the annotations. The receiver's `git status` is not asserted: a fast-forward pull currently leaves an untracked open delta for the incoming file. |
| `switch -c`, edit, commit, `merge --no-ff` | `branch_edit_commit_merge_no_ff` | Offline |
| `merge --ff-only` | `fast_forward_merge_keeps_the_annotation` | Offline |
| `commit -a` | `commit_a_records_a_tracked_edit` | Offline |
| `git add` one file, leave another unstaged | `a_partial_add_commits_one_file_and_keeps_the_other` | Offline |
| `commit --amend` of the message only, with no command between the commit and the amend | `a_message_only_amend_keeps_the_delta` | Offline |
| `stash push` and `stash pop` of an anchored file | `stash_and_pop_round_trip_an_anchored_edit` | Offline |
| `git restore` of one file | `git_restore_discards_an_edit_and_undo_puts_it_back` | Offline |
| `revert` of a code commit | `revert_of_a_code_commit_keeps_the_anchor` | Offline |
| `cherry-pick` of a code commit | `cherry_pick_of_a_code_commit_follows_the_anchor` | Offline |
| `pull --rebase` of a code-only commit | `pull_rebase_replays_a_local_commit` | Shared. The commit is made with `--no-verify`, so it contains no delta. Rebasing a commit that contains a delta fails in direct mode; see below. |
| Merge conflict in a source file, then resolve and commit | `a_resolved_source_conflict_keeps_the_anchor` | Offline |
| Detached `HEAD`, then back | `detached_head_round_trip` | Offline |
| `governance.paths` leaves files outside the globs out of `check` | `files_outside_governance_paths_are_not_consistency_errors` in `shared.rs` | Shared |
| A hook manager is present | `a_hook_manager_is_left_for_its_owner` (husky), `a_pre_commit_config_is_left_for_its_owner`, `lefthook_yml_is_left_for_its_owner`, `lefthook_yaml_is_left_for_its_owner`, `core_hooks_path_inside_the_work_tree_is_left_alone`, `a_non_shell_hook_is_left_unchanged` in `s01_init.rs` | Offline |
| Linked worktree | `a_linked_worktree_keeps_its_own_graph` in `git_ux.rs` | Offline. Each worktree is its own root and graph. |
| File mode change (`chmod +x` then commit) | `a_file_mode_change_keeps_the_anchor` | Offline |
| Symlink committed next to an anchored file | `a_symlink_is_not_its_own_file_and_resolves_to_its_target` | Offline. The link is not a graph file. A path typed through it anchors the target. |
| `commit --fixup` and `rebase --autosquash` of code-only commits | `autosquash_of_code_commits_keeps_the_anchor` | Offline. The replayed commits are `--no-verify`, so they contain no delta. |
| `resonance restore` into a graph that already has nodes | `restore_into_a_live_graph_adds_what_is_missing_and_keeps_the_rest` in `storage.rs` | Offline. Adds what the export has and the target lacks. Keeps the target's own nodes. |
| Declare a unit in a file larger than `ingest.maxBlobBytes` | `a_unit_in_a_file_past_max_blob_bytes_stays_attached` in `s01_init.rs` | Offline. Attached after declare. `rebuild` is not asserted: settle re-checks stored blobs only. |
| Empty search, retired status, duplicate names, context pack edges | `an_empty_graph_searches_repairs_and_rejects_an_empty_context_pack`, `retired_requirements_match_their_status_and_duplicate_names_both_match` in `queries.rs` | Offline |
| `resonance registry install --yes` and `uninstall --yes` | `registry_install_and_uninstall_write_a_user_service` in `daemon.rs` | A fake `launchctl` (macOS) or `systemctl` (other) is first on PATH. The dry run is `registry_install_previews_and_changes_nothing_without_confirmation`. |

`cherry_pick_of_a_code_commit_follows_the_anchor` checks that the picked text and the anchors already on the branch survive. It does not check that an offline delta sealed on the picked commit is re-associated. That gap is listed below.

## Workflows with no passing test yet

These are real git commands the spec names. Do not treat a green `git_ux` run as coverage of them.

- **`pull --rebase` of a commit that contains a delta.** In direct mode the rebase aborts: `post-checkout` writes `.resonance/deltas/<id>.delta` while the commit that adds that same path is being replayed, and git refuses to overwrite the untracked file. Daemon mode got through the same sequence once; do not treat that as covered.
- **A clean `git status` after `git pull` in a shared repository.** `pull_edit_commit_push_round_trip` checks the file and the annotations. The receiver still gets an untracked `.resonance/deltas/*.delta` whose manifest records the incoming file (`AFTER.md - -> <hash>`, no authored operations). `file.put` is not stored in the delta (FR-064), so the receiver's ingest records the file again, after `manifest_after`, and `sync` writes a new open delta. `join` avoids this with `rebase_open`. A pull should too.
- **Offline cherry-pick of a commit that has its own delta.** The code cherry-pick above does not move a sealed offline delta onto the new commit.
- **`git merge --squash` then commit**, offline. Shared squash works by carrying delta files; offline deltas are associated by patch id, which does not match a squash commit.
- **`git reset --soft` and `git reset --mixed`** (recommit after undoing the commit). `reset --hard` is tested. Soft and mixed are not.
- **`git revert` of a commit that deletes a delta already folded into `graph.db`** (FR-159). Revert of an ordinary source commit is tested.
- **Interactive rebase** (`reword`, `edit`, an editor-driven `squash` of a delta commit). `commit --fixup` plus `rebase --autosquash` is tested for code-only commits. Autosquash of a commit that sealed a delta is not.
- **`git clean -fdx`**, **`git stash -a`**, and **`stash drop`** of uncommitted graph state. The journal is written and never read back.
- **`git add -p`** (hunks). Partial path adds are tested; hunks are not.
- **Submodules, sparse checkout, CRLF renormalization, signed commits.** File-mode-only changes and symlinks are tested.

## Other thin areas

Worth more tests when someone is already in that code. One test each is not enough to call them covered.

- **Recovery choices other than undo.** `recover undo`, `attach`, and `drop` are tested. Taking git's version, a three-way merge, or keeping both (FR-196) have no command and no test.
- **`resonance restore` of a retire or remove over a live graph.** Adding missing nodes is tested. Replaying an export that retires something the target still has active is not: restore skips existing creates and still applies retires and removes.
- **`test.command` values that need shell quoting.** The substitution test uses a name and a path with no spaces or metacharacters.
- **An anchor past `ingest.maxBlobBytes` across `resonance rebuild`.** Declare-and-show is tested. Rebuild re-checks anchors against stored blobs and does not read the disk, so a unit in an unstored file can be orphaned. That outcome is not asserted.
- **C8 and C9 as divergence records.** A bypassed commit is blocked on push (`shared.rs`). Nothing asserts a `C8` row from `resonance recover`. Nothing produces a `C9`.
- **Events missed while the daemon stays up** (`local/downtime.log` overlapping a file's mtime). Startup-after-stop is tested; a daemon that lost watcher events is not. Nothing writes `downtime.log`.
- **The spec TUI** is covered by simulated keystrokes in `crates/resonance-cli/src/tui/tests.rs`, not by a terminal session. The real terminal entry in `tui/mod.rs` is what a coverage report shows as thin.
- **Registry on Linux.** `service_plan` emits a systemd unit only when compiled for Linux. The install test runs whichever branch this machine compiles, through a fake `systemctl` or `launchctl`.

When you add a git workflow test, put it in `crates/resonance-cli/tests/git_ux.rs` if it is an everyday command sequence, or next to the scenario it extends (`git_offline.rs`, `shared.rs`). Add a row to the table above in the same change.
