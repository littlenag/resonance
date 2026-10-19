# Configuration

Resonance works without any configuration. When you need to change something, settings live in `.resonance/config.toml`, which `resonance init` writes with comments explaining each one. The daemon notices changes to this file and applies them right away. Once the graph is shared, `config.toml` is committed, so its settings apply to the whole team.

## Settings

```toml
[repository]
# The adoption stage: 1 offline, 2 shared (written by `resonance share`).
stage = 1
# The oldest resonance version that may write this repository's data.
minVersion = ""

[ingest]
# Extra ignore patterns (globs over repository-relative paths), on top of .gitignore and
# the built-in defaults.
ignore = []
# Files larger than this are recorded by hash and size only.
maxBlobBytes = 5242880

[test]
# Fallback for tests declared without a run command; {name}, {path} and {address} are substituted.
command = ""
# Output that means no test actually ran; such a run is recorded as `error`, never `pass`.
zeroTestsPattern = "running 0 tests"
# A test that runs longer than this is stopped and recorded as `error`.
timeoutSeconds = 600

[daemon]
# The daemon exits after this many seconds without requests.
idleSeconds = 1800

[compaction]
# Compact when the trunk has this many sealed deltas.
maxDeltas = 100
# The trunk; empty means the remote's default branch, else main, else master.
trunk = ""

[governance]
# Path globs Resonance governs; empty means everything.
paths = []

[changes]
# Also log changes made by agents in the change log.
includeAgents = false
```

| Setting | Default | Meaning |
| --- | --- | --- |
| `repository.stage` | `1` | The adoption stage. `resonance share` sets it to 2; don't change it by hand. See [Sharing the graph](sharing.md). |
| `repository.minVersion` | `""` | The oldest Resonance version that may write this repository's data. Written by `share`. |
| `ingest.ignore` | `[]` | Extra glob patterns of files and directories not to read, for example `["vendor/**", "*.min.js"]`. |
| `ingest.maxBlobBytes` | `5242880` (5 MiB) | Larger files are recorded by hash and size only, with no copy of their content. |
| `test.command` | `""` | The run command for tests declared without `--run`. `{name}`, `{path}` and `{address}` are replaced with the test's name, file path and address. |
| `test.zeroTestsPattern` | `"running 0 tests"` | If a test's output contains this text, the run is recorded as `error`, because nothing actually ran. Set it to what your test runner prints when a filter matches nothing (for pytest, `"no tests ran"`). |
| `test.timeoutSeconds` | `600` | A test still running after this is stopped and recorded as `error`. |
| `daemon.idleSeconds` | `1800` | The daemon exits after this many seconds without requests. |
| `compaction.maxDeltas` | `100` | On the trunk, compact automatically past this many sealed deltas. |
| `compaction.trunk` | `""` | The branch compactions happen on. Empty: the remote's default branch, else `main`, else `master`. |
| `governance.paths` | `[]` | Globs of paths Resonance governs when you've opted in: edits there made while Resonance wasn't running are restored from the graph (`C3`), and uncovered changes there are reported by `check`. Empty means everything. |
| `changes.includeAgents` | `false` | Also record agents' changes to spec items in the [change log](tui-and-change-log.md). |

## Environment variables

Most settings can be overridden for one command, or for a whole shell session, with an environment variable:

| Variable | Overrides |
| --- | --- |
| `RESONANCE_INGEST_IGNORE` | `ingest.ignore`, as a comma-separated list |
| `RESONANCE_INGEST_MAX_BLOB_BYTES` | `ingest.maxBlobBytes` |
| `RESONANCE_TEST_COMMAND` | `test.command` |
| `RESONANCE_TEST_ZERO_TESTS_PATTERN` | `test.zeroTestsPattern` |
| `RESONANCE_TEST_TIMEOUT_SECONDS` | `test.timeoutSeconds` |
| `RESONANCE_DAEMON_IDLE_SECONDS` | `daemon.idleSeconds` |

For example:

```
RESONANCE_TEST_TIMEOUT_SECONDS=60 claude
```

These variables travel with each request, from the CLI, the TUI and your agent's MCP server alike, so they apply even though an already-running daemon serves it.

Two more variables control how Resonance runs rather than what it does:

| Variable | Effect |
| --- | --- |
| `RESONANCE_NO_DAEMON=1` | Run every command in its own process instead of through the daemon, like `--no-daemon`. |
| `RESONANCE_RUNTIME_DIR` | Where daemon sockets go (see [The daemon](daemon.md#files)). Keep it short: socket paths are limited to about 100 characters. |
| `RESONANCE_ACTOR` | Who is running a `resonance` command: `agent:<name>` for an agent (the agent kit sets `agent:claude-code` for Claude Code), or `human:<email>`. Without it, you are identified by `git config user.email`. |
| `RESONANCE_OVERRIDE=1` | Before a git command, tells the [agent gate](agents.md#the-agent-gate) that you told the agent to proceed. |

Precedence, from highest to lowest: command-line flags, environment variables, `.resonance/config.toml`, built-in defaults.

## Which files are read

Resonance reads every file in the repository except:

- `.git/` and `.resonance/`;
- files ignored by git: `.gitignore` files at any level, `.git/info/exclude`, and your global git excludes file;
- these directories, wherever they appear: `target`, `build`, `dist`, `out`, `node_modules`, `.venv`, `__pycache__`, `.gradle`, `.bsp`, `.bloop`, `.metals`;
- editor scratch files: `*.swp`, `*~`, `.#*` and `4913`;
- anything matching `ingest.ignore`;
- nested git repositories, including submodules.

Files that are read but contain NUL bytes, or are larger than `ingest.maxBlobBytes`, are recorded by hash and size only. You can link and annotate them as files, but they can't hold code units.

To check whether a file is read, look at the file count in `resonance status`, or ask your agent whether Resonance knows the file: an ignored file isn't in the graph.

## The repository root

Resonance always works on a whole git repository. It finds the root with `git rev-parse --show-toplevel`, so you can run commands from any subdirectory. Paths you give are relative to your current directory, and paths Resonance prints are relative to the root. `-C <path>` runs a command as if it was started in `<path>`. Outside a git repository, Resonance refuses to run.
