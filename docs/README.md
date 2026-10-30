# Resonance user guide

Resonance is a superpower for your coding agent and a background tool for you. It keeps a graph beside your git repository: the requirements, decisions, designs and constraints behind your project, the code and tests that realize them, typed links between them, and notes for whoever changes that code next. Your agent works with the graph through an MCP server. With it, the agent can show you how it understood the spec and how that connects to code, tests and documents, and you can give it everything it needs to build what you said and what you meant.

You rarely touch Resonance yourself. You set it up once with `resonance init`, browse and edit the spec in the web UI (`resonance ui`) or the TUI (`resonance tui`) when you want to, and use a few commands to check on it and repair it. Everything else, from bringing in your requirements documents to declaring code, linking, annotating, tracing and running tests, you ask your agent to do.

Resonance parses nothing: not your code, and not your documents. Your agent reads them and records what it understood, and Resonance keeps that attached to the right text as files change, whatever tool changes them.

Resonance starts offline: the graph lives in `.resonance/` at the root of your repository, excluded from git, so nothing is committed and `git status` never changes. When your team is ready, you can share the graph, so that it's committed, reviewed and merged with the code.

## Start here

- [Quick start](quick-start.md): set Resonance up, onboard your agent and bring in your spec, in about ten minutes.
- [Concepts](concepts.md): spec items, code units, tests, links and their statuses, annotations, and the repair list.

## Guides

- [Working with your agent](agents.md): the agent kit, what your agent is told, what to ask it, and the gate on its git commands.
- [Your spec documents](spec-documents.md): how your agent brings documents in, and keeps them and the graph in step.
- [The web UI, diagnostics and logs](web-ui.md): browse and edit spec items in your browser, and see what the daemon is doing.
- [The TUI and the change log](tui-and-change-log.md): browse and edit spec items in the terminal, and the log of changes your agent picks up.
- [Sharing the graph](sharing.md): stages, `share`, joining, the git hooks, checks, pull requests, merging and compaction.
- [Recovery](recovery.md): when git, the files and the graph diverge, and how to undo a repair.
- [The daemon](daemon.md): the background process that keeps the graph current, `doctor`, and the optional registry.
- [Configuration](configuration.md): `.resonance/config.toml`, environment variables, and which files are read.

## Reference

- [CLI reference](cli-reference.md): every command, option and exit code.
- [Troubleshooting](troubleshooting.md): common problems, where your data lives, and removing Resonance from a repository.
