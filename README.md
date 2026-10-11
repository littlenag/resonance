# resonance
Giving your agent a semantic API to your source code

## Development

Repeatable tasks are recipes in the [`justfile`](justfile); run `just` to list them. They need [`just`](https://github.com/casey/just), the Rust toolchain pinned in `rust-toolchain.toml`, and `python3` for the spec checks.

| Recipe | What it does |
| --- | --- |
| `just check` | Everything CI should run: `fmt-check`, `lint`, `test`, `boundary`, `spec-check` |
| `just build` / `just test` | Build or test the whole workspace against the committed `Cargo.lock` |
| `just fmt` / `just fmt-check` | Format Rust code, or fail if it is not formatted |
| `just lint` | Clippy, with warnings as errors |
| `just boundary` | Fail if `resonance-core` depends on SQLite or a source-code parser (FR-110, FR-161) |
| `just spec-check [ids,coverage,sections,spelling]` | Spec consistency: requirement IDs, MVP/backlog coverage, section references, US spelling |
| `just run <args>` | Run `resonance` from source |
| `just install` | Install `resonance` from source |

The specification is in [`spec/`](spec/): the working MVP spec is [`spec/mvp.md`](spec/mvp.md), and the remaining work is in [`spec/backlog.md`](spec/backlog.md).
