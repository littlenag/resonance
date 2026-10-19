# resonance
Giving your agent a semantic API to your source code

Resonance is a superpower for your coding agent and a background tool for you. It keeps a graph beside a git repository: requirements, decisions, designs and constraints, the code units and tests that realize them, typed links between them, and annotations. Your agent works with it through an MCP server: it brings your spec documents in, declares the code and tests it works on, links, annotates, traces and verifies, and so can show you how it understood what you asked for. Resonance parses nothing, neither code nor documents: the agent names what matters, and Resonance keeps it attached as files change.

You use the `resonance` command only to set it up, run its daemon, check on it and repair it, plus `resonance tui` to browse and edit the spec. It starts offline: the graph lives in `.resonance/`, excluded through `.git/info/exclude`, so nothing is committed and `git status` never changes, until your team chooses to share it. See [`spec/mvp.md`](spec/mvp.md) for exactly what it does.

## Using it

The [user guide](docs/README.md) covers everything, starting with a [quick start](docs/quick-start.md). In short:

```
cargo install --locked --path crates/resonance-cli   # or: just install

cd your-repository
resonance init      # mirrors the repository into .resonance/, installs git hooks and the agent kit
claude              # then ask: "Give me the Resonance orientation."
                    #           "Bring docs/requirements.md into Resonance."
                    #           "What implements FR-001, and is it verified?"
resonance tui       # browse and edit the spec; your edits land in a change log for the agent
resonance status    # stage, divergences, the repair list, pending changes, the agent kit
resonance recover   # when git and the graph disagree: what happened, and undo
resonance eject     # export the graph, then remove every trace of Resonance
```

Requests are served by a per-repository daemon that watches the working tree; the first request starts it, and it exits after 30 idle minutes. Every answer reflects the files on disk, including edits made a moment before with any editor or agent. `resonance daemon start|stop|restart` controls it and `resonance doctor` shows its health; `--no-daemon` runs a command in-process instead (for debugging). An optional system-wide registry (`resonance registry install`) tracks the daemons of every repository for `doctor`. Add `--json` to any command for machine-readable output.

## Development

Repeatable tasks are recipes in the [`justfile`](justfile); run `just` to list them. They need [`just`](https://github.com/casey/just), the Rust toolchain pinned in `rust-toolchain.toml`, and `python3` for the spec checks.

| Recipe | What it does |
| --- | --- |
| `just check` | Everything CI should run: `fmt-check`, `lint`, `test`, `test-direct`, `boundary`, `spec-check` |
| `just build` / `just test` | Build or test the whole workspace against the committed `Cargo.lock` |
| `just test-direct` | Run the CLI acceptance tests in direct mode (`--no-daemon`) |
| `just fmt` / `just fmt-check` | Format Rust code, or fail if it is not formatted |
| `just lint` | Clippy, with warnings as errors |
| `just boundary` | Fail if `resonance-core` depends on SQLite or a source-code parser (FR-110, FR-161) |
| `just spec-check [ids,coverage,sections,spelling]` | Spec consistency: requirement IDs, MVP/backlog coverage, section references, US spelling |
| `just run <args>` | Run `resonance` from source |
| `just install` | Install `resonance` from source |
| `just fixture-repo [dir]` | Create a fresh test repository from [`fixtures/starter`](fixtures/starter), with generated edge cases, for field tests |

Field-test on fixture repositories and other small repositories, not on this one: running Resonance on its own repository is backlog item *Self-dogfood*, done once the MVP is judged reliable.

The workspace has three crates (MVP-001): `resonance-core` (the model, store API, ingest, anchor tracking, queries, MCP, daemon and registry, all engine-neutral), `resonance-store-sqlite` (the only crate that knows SQLite), and `resonance-cli` (the `resonance` binary). The acceptance tests for [`spec/mvp.md`](spec/mvp.md) §5 are in `crates/resonance-cli/tests/`; the anchor-tracking scenarios are in `crates/resonance-core/tests/anchors.rs`; the store conformance suite is in `crates/resonance-core/src/store/conformance.rs`.

The specification is in [`spec/`](spec/): the working MVP spec is [`spec/mvp.md`](spec/mvp.md), and the remaining work is in [`spec/backlog.md`](spec/backlog.md).
