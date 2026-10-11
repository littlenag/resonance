# resonance
Giving your agent a semantic API to your source code

Resonance keeps a private graph beside a git repository: requirements and decisions, the code units and tests that realize them, typed links between them, and annotations. It parses no source code. Code units are named regions of files that people and agents declare, and Resonance keeps them attached as files change. Agents reach the graph through an MCP server; people use the CLI.

The MVP is single-developer and offline: the graph lives in `.resonance/`, excluded through `.git/info/exclude`, so nothing is committed and `git status` never changes. See [`spec/mvp.md`](spec/mvp.md) for exactly what it does.

## Using it

```
cargo install --locked --path crates/resonance-cli   # or: just install

cd your-repository
resonance init                          # .resonance/, excluded locally; reads every file
resonance import spec/requirements.md   # `- **FR-001** text` items and `| **NFR-001** | text |` rows
resonance agent install                 # registers the MCP server with Claude Code, local scope only
resonance unit add src/store.rs:40-95 --name open --kind function
resonance test add tests/store.rs:10-20 --name opens --run "cargo test -- --exact opens"
resonance link open satisfies FR-001
resonance link opens verifies FR-001
resonance test FR-001                   # runs linked tests, records evidence, marks links verified
resonance trace FR-001                  # requirement -> code -> tests, with statuses
resonance why open                      # code -> requirements, decisions, tests, annotations
resonance annotate open --kind instruction --text "do not change the signature"
resonance status                        # counts plus the repair list
resonance eject                         # export the graph, then remove .resonance/
```

Commands are served by a per-repository daemon that watches the working tree; the first command starts it, and it exits after 30 idle minutes. Every answer reflects the files on disk, including edits made a moment before with any editor or agent. `resonance doctor` shows the daemon's health; `--no-daemon` runs a command in-process instead (for debugging), and `--cached` answers without waiting for changes still being applied. An optional system-wide registry (`resonance registry install`) tracks the daemons of every repository for `doctor`. References accept a requirement key (`FR-001`), a unit address (`src/store.rs::open`), a unique unit name, a file path, or an ID (or a unique prefix of one). Add `--json` to any command for machine-readable output.

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
