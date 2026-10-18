# Resonance user guide

Resonance keeps a private graph beside your git repository. It records why your code exists: the requirements and decisions behind it, the code and tests that realize them, typed links between them, and notes for the people and agents who change that code. Your agent reaches the graph through an MCP server; you use the `resonance` command.

Resonance parses no source code, so it works with every language and any text file. You or your agent declare the regions of files that matter, and Resonance keeps them attached as the files change, whatever tool changes them.

Resonance starts offline: the graph lives in `.resonance/` at the root of your repository, excluded from git, so nothing is committed and `git status` never changes. Your teammates, CI and build tools see no difference. When your team is ready, you can share the graph, so that it's committed, reviewed and merged with the code.

## Start here

- [Quick start](quick-start.md): install Resonance and use it on a small project in about fifteen minutes.
- [Concepts](concepts.md): requirements, decisions, code units, tests, links, annotations and the repair list.

## Guides

- [Requirements and decisions](requirements-and-decisions.md): import requirements from Markdown, author them by hand, and record decisions.
- [Code units and tests](code-units-and-tests.md): declare regions of code, how Resonance follows them through edits, renames and moves, and how tests are declared.
- [Links and verification](links-and-verification.md): link types and statuses, why links become stale, and running tests to verify them.
- [Annotations](annotations.md): notes, warnings, call-outs and instructions attached to code.
- [The repair list](repair-list.md): what needs attention after code or requirements change, and how to resolve each item.
- [Queries](queries.md): `trace`, `why`, `context`, `search`, `show` and `unverified`.
- [Working with agents](agents.md): the agent kit, what your agent is told, and the gate on its git commands.
- [The TUI and the change log](tui-and-change-log.md): browse and edit spec items in a terminal UI, and the log of changes your agent picks up.
- [Sharing the graph](sharing.md): stages, `share`, joining, the git hooks, checks, pull request reviews, merging and compaction.
- [Recovery](recovery.md): when git, the files and the graph diverge, and how to undo a repair.
- [The daemon](daemon.md): the background process that keeps the graph current, `doctor`, and the optional registry.
- [Configuration](configuration.md): `.resonance/config.toml`, environment variables, and which files are read.

## Reference

- [CLI reference](cli-reference.md): every command, option and exit code.
- [Troubleshooting](troubleshooting.md): common problems, where your data lives, and removing Resonance from a repository.
