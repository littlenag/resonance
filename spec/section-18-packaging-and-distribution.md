## 18. Packaging and distribution

Resonance is distributed as **one binary** (§17). Everything a developer, an agent or a CI job needs locally is in it. Installing it changes nothing in any repository; per-repository setup always happens through explicit commands (`init`, `agent install`, and so on). Other deliverables come later, once the MVP has matured, and each sits on top of the binary rather than beside it. [Decided]

### 18.1 The binary

- **FR-260** Resonance MUST be released as one self-contained executable per platform, with no runtime dependencies beyond the operating system and `git`. SQLite is bundled; Linux builds are statically linked (musl). Release targets: macOS on arm64 and x86_64, and Linux on x86_64 and aarch64 (NFR-038). Windows is deferred (OQ-07). [Decided: one binary. Proposed: targets]
- **FR-261** Distribution channels are added in stages:

  | Stage | Channels | Audience |
  | --- | --- | --- |
  | **MVP** | Build from source: `cargo install --path crates/resonance-cli`, or `cargo install --git <repo>` | Mark, Claude, early dogfooding |
  | **First releases** | GitHub Releases (archives per target, checksums, signatures); `cargo install resonance` from crates.io; a Homebrew tap | Early adopters, offline trial users |
  | **Team adoption** | The quiet bootstrap (FR-231) fetching pinned GitHub Release artifacts; documented install steps for team setup instructions | Shared repositories, quiet participants, CI |
  | **Later, on demand** | Official Homebrew formula, Linux distribution packages, container image for CI | Wider use |

  [Proposed]
- **FR-262** Every release artifact MUST have a published SHA-256 checksum and a signature (for example minisign or Sigstore). The quiet bootstrap and CI wrappers verify both. Releases are built reproducibly from a tagged commit by CI, never from a developer's machine. [Proposed]
- **FR-263** Installing or upgrading the binary MUST NOT modify any repository, git configuration or agent configuration. User-level installs happen only through explicit, previewed commands, and each has an inverse:
  - the registry service: `registry install` / `registry uninstall`;
  - agent registrations: `agent install`;
  - everything user-level at once: `uninstall --user`.

  [Proposed]
- **FR-264** The binary follows semantic versioning (FR-254). From stage 2 onward, a repository's committed `config.toml` records the minimum binary version its data needs. An older binary refuses to write and says what to install (FR-258). The quiet bootstrap pins an exact version and checksum (FR-231). [Proposed]
- **FR-265** Upgrades come through the channel the binary was installed from; the binary does not update itself. After an upgrade, running daemons notice the new version on disk and restart when idle, and the registry reports any remaining version skew (FR-091). [Proposed]
- **FR-266** `resonance uninstall --user` removes everything the binary installed outside repositories: the registry service, agent registrations, caches and the run directory. Removing Resonance from a repository is `resonance eject` (FR-173). Removing the binary itself is left to the channel it came from. [Proposed]

### 18.2 What ships inside the binary

| Item | Delivered as | Spec |
| --- | --- | --- |
| CLI, daemon, MCP server, registry, quiet mode, plumbing | Subcommands | §17 |
| Agent kit (instructions text, skill, harness hook definitions, conventions) | Embedded content, installed by `agent install` and served over MCP | §8.20 |
| Git hook shims, `graph.db` merge driver, delta textconv | Embedded templates calling `resonance hook …`, `resonance git-driver …`, `resonance show-delta` | FR-069, FR-079, FR-157 |
| `.resonance/README.md`, `.resonance/.gitattributes`, `.resonance/.gitignore` | Embedded templates written by `init` and `share` | FR-025, FR-079 |
| Quiet bootstrap script (`.resonance/bin/resonance-quiet`) | Embedded template written by `share`; fetches and verifies the pinned binary, then runs `resonance quiet …` | FR-231, FR-257 |
| launchd and systemd user units | Embedded templates written by `registry install` | FR-093 |
| Schema migrations; import-format schema | Embedded | FR-006, FR-243 |

### 18.3 Deliverables outside the binary (later)

These come after the MVP has matured. Each depends on the binary and adds no local functionality that the binary lacks.

| Deliverable | Purpose | Packaging | When |
| --- | --- | --- | --- |
| **CI wrappers** | Run `resonance check` and `resonance review` in CI; post PR summaries; quiet bookkeeping bot at stage 3 | A GitHub Action, plus documented snippets for other CI systems; each fetches the pinned, verified binary | With B6 and B11 |
| **IntelliJ plugin** | Gutter markers, navigation, annotation UI, stale and orphan warnings | JetBrains Marketplace plugin, talking to the daemon API | B14 |
| **Agent kit as a harness plugin** | The same kit packaged for one-step installation in harnesses that support plugins | For example a Claude Code plugin; content generated from the binary's embedded kit | With B7, optional |
| **Graph adapters** | Turn existing tools' output (for example Mache) into parallel or disconnected graphs | Small separate tools or upstream changes; output loaded with `resonance graph load` | B13 |
| **Phase 0 evaluation harness** | The experiments in §11 | Internal scripts; never shipped | B17 |

### 18.4 Repository layout for distribution

```
Cargo.toml                       # workspace
crates/
  resonance-core/                # library: model, store trait, queries, anchors, import, MCP/CLI logic
  resonance-store-sqlite/        # library: the SQLite adapter (only crate that depends on rusqlite)
  resonance-cli/                 # binary: `resonance`
  resonance-store-memory/        # library: in-memory adapter for tests (B5)
assets/                          # embedded templates: agent kit, hooks, README, service units, schemas
.github/workflows/release.yml    # reproducible release builds, checksums, signatures (first releases)
```

The licence, which affects every channel above, is still [Open] (OQ-15).
