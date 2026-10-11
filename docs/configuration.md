# Configuration

Resonance works without any configuration. When you need to change something, settings live in `.resonance/config.toml`, which `resonance init` writes with comments explaining each one. The daemon notices changes to this file and applies them right away.

## Settings

```toml
[intent]
# Requirement documents imported with `resonance import`.
sources = []

[ingest]
# Extra ignore patterns (globs over repository-relative paths), on top of .gitignore and
# the built-in defaults.
ignore = []
# Files larger than this are recorded by hash and size only.
maxBlobBytes = 5242880

[test]
# Fallback for tests declared without --run; {name}, {path} and {address} are substituted.
command = ""
# Output that means no test actually ran; such a run is recorded as `error`, never `pass`.
zeroTestsPattern = "running 0 tests"
# A test that runs longer than this is stopped and recorded as `error`.
timeoutSeconds = 600

[daemon]
# The daemon exits after this many seconds without requests.
idleSeconds = 1800
```

| Setting | Default | Meaning |
| --- | --- | --- |
| `intent.sources` | `[]` | Imported requirement documents, in import order. `resonance import` adds to it. Remove a path to stop importing that document. |
| `ingest.ignore` | `[]` | Extra glob patterns of files and directories not to read, for example `["vendor/**", "*.min.js"]`. |
| `ingest.maxBlobBytes` | `5242880` (5 MiB) | Larger files are recorded by hash and size only, with no copy of their content. |
| `test.command` | `""` | The run command for tests declared without `--run`. `{name}`, `{path}` and `{address}` are replaced with the test's name, file path and address. |
| `test.zeroTestsPattern` | `"running 0 tests"` | If a test's output contains this text, the run is recorded as `error`, because nothing actually ran. Set it to what your test runner prints when a filter matches nothing (for pytest, `"no tests ran"`). |
| `test.timeoutSeconds` | `600` | A test still running after this is stopped and recorded as `error`. |
| `daemon.idleSeconds` | `1800` | The daemon exits after this many seconds without requests. |

## Environment variables

Every setting except `intent.sources` can be overridden for one command, or for a whole shell session, with an environment variable:

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
RESONANCE_TEST_TIMEOUT_SECONDS=60 resonance test FR-001
```

These variables travel with each command, so they apply even though an already-running daemon serves it.

Two more variables control how Resonance runs rather than what it does:

| Variable | Effect |
| --- | --- |
| `RESONANCE_NO_DAEMON=1` | Run every command in its own process instead of through the daemon, like `--no-daemon`. |
| `RESONANCE_RUNTIME_DIR` | Where daemon sockets go (see [The daemon](daemon.md#files)). Keep it short: socket paths are limited to about 100 characters. |

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

To check whether a file is read, ask Resonance about it:

```
resonance show path/to/file
```

An ignored file isn't in the graph, so `show` reports that nothing matches.

## The repository root

Resonance always works on a whole git repository. It finds the root with `git rev-parse --show-toplevel`, so you can run commands from any subdirectory. Paths you give are relative to your current directory, and paths Resonance prints are relative to the root. `-C <path>` runs a command as if it was started in `<path>`. Outside a git repository, Resonance refuses to run.
