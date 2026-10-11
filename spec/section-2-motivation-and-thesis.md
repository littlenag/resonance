## 2. Motivation and thesis

### 2.1 Files on disk (FOD) is becoming obsolete as the primary working model  [Decided]

Mark's thesis: the model in which agents and developers work primarily by reading and editing files on disk ("FOD") is extremely inefficient, i.e. agents are hamstrung working with files when a more efficient and expressive encoding would benefit both agents and humans in their tasks. Agents can, and should, work with a representation and protocol that suits their abilities and that connects code to the higher-level concerns that increasingly define software work: requirements, design, and alignment. Software development will continue to be defined even more as a system that takes inputs and that produces useful outputs. We merely mean to emphasize to the extreme the boundaries where humans and agents communicate while this project fills in the middle that handles the semantic mappings through and back.

### 2.2 Where developer effort moves  [Decided]

Developers increasingly concentrate on describing the behavior a solution must have. The mechanism for that is high-level design documents and requirements. Meanwhile agents generate solutions behind the scenes, of which code is only one part (other parts include, for example, tests, configuration and documentation). During the migration between models, the critical capability is a representation that lets developers step directly between:

- requirements,
- the code (and other artifacts) generated from those requirements that satisfies them,
- the tests that exercise that code,
- the solution ultimately generated,

It will be too much to ask that developers immediately quantum leap into a word where they no longer interact with code; an intermediate state of humans and agents have equal agency over code must be created. That will require a reasonable and expressive way to communicate about all aspects of software development until agents are sufficiently adept to abstract solely from requirements. Thus developers who still work at the code level must be able to **annotate and call out** specific functions and other constructs to an agent.

### 2.3 Why a graph  [Decided]

A graph database breaks the project into nodes and relations that can each be annotated with **agreed-upon semantics**. An agent, and eventually an IDE that speaks the same graph model, can then work from that shared model to generate solutions, but to also under how behavior of a solution is reflected/inferred (or not) in the provided requirements and designs.

### 2.4 Files as compiled output, not as obsolete  [Decided]

Compilers, runtimes, CI, code review and deployment will continue to consume text files for a long time, and language models are strongest at reading and writing text. The working position of this specification is therefore that **source files become a projection of the graph**, much as assembly is a projection of a compiled program: still present and still executed, but no longer where developers and agents do their thinking. This is also what makes adoption possible: existing projects can adopt Resonance without giving anything up.

A corollary of this perspective is that migration to any tooling cannot be instantaneous, and to be successful a reasonable migration path must be supported. Supporting file trees is thus viewed as a required to support current tooling, but nonetheless a temporary condition that will be replaced by updated tooling that operates on the graph model natively. In the extreme files are but an (optional) implementation detail. 

The migration path is made concrete as **adoption stages** (§8.16): offline trial, then a shared graph that developers join one at a time, then enforcement. Authority is explicit at every stage. For each developer who has opted in, the graph is the source of truth and their work must stay consistent with it. For everyone else, git is the source of truth for their own work. Both data paths (git → graph and graph → git) stay open, and every step can be undone. Even late in adoption, files in git will sometimes be the real source of truth for a change (a hold-out's commit, a bypassed hook, a hotfix made in a hurry), so Resonance is designed to recognize those cases from git's own record and recover from them gracefully (§8.18). [Decided]

Taking this model to its logical conclusion Resonance will offer a container in which nearly all of existing tooling, and the creation of "software" itself is but an implementation detail. This is similar to how compilers obscure the transformation of code into assembly, so too will Resonance provide the scaffolding necessary to obscure the next abstraction level of the software development lifecycle.

### 2.5 Position on code structure  [Decided]

Mark is skeptical that giving agents easy access to *code structure* (as Mache does, using tree-sitter) is a first-order benefit. Agents already handle large codebases well when requests are well structured. Structural access is likely a second- or third-order efficiency gain, useful mainly for planning and verifying large refactors.

Going further, **Resonance core does not parse source code.** It does not turn source into syntax trees or any other abstract data type, and semantic understanding of code (symbols, types, call graphs) is outside Resonance's scope as Mark envisions it today. Consequences for this specification:

- **Code units are regions of file content that people and agents name.** Resonance tracks them through edits by their text (§6.2, §8.3). The semantic judgment ("this is the function that implements FR-042") comes from the agent or developer, who already understands the code. Resonance's job is to remember that judgment and keep it attached.
- **Code structure as a parallel graph, maybe.** The graph Resonance stores is the **primary graph**. Structure derived by a separate tool (Mache, tree-sitter or compiler data, for example) lives in a **parallel graph** that is derived from the primary graph and linked to it in both directions. Queries may roam freely across both graphs, but operations on a parallel graph can never modify the primary graph (§6.7, §8.9). Before any tool integrates that closely, existing tools can be adapted to emit stand-alone **disconnected graphs** of useful semantic information, which Resonance loads and serves alongside the primary graph. That is the expected starting point for tooling that later grows Resonance-native support (§6.7). Mark is not yet sold on building or integrating a code-structure tool, and Phase 0 tests whether it would be worth it (§11).
- **Value is in intent and verification.** The product's value is concentrated in the intent and verification layers (§6).
- **An alternative to files, eventually.** As an eventual goal, Resonance offers an alternative to files for languages that want to store their source in another format (G15). Even then, understanding that source remains the language's job, not Resonance's.

To the extent code structure is meaningful, it will need to be replaced by tooling that produces views that fit an "agent's mind". It is not yet clear how that tooling is to be created, or whether it belongs in Resonance.

### 2.6 Why this can work now when earlier attempts stalled  [Decided]

Requirements traceability tools, model-driven architecture, Intentional Programming, Smalltalk images, JetBrains MPS and similar systems mostly stalled because **humans had to maintain the links**, the links drifted from reality, and people stopped trusting them. Agents change the economics: keeping links current is cheap and continuous for an agent. Two conditions follow, and they shape the design:

1. **Link maintenance is the core product**, not a side feature.
2. **Links must be checkable.** A `verifies` link backed by a passing test is evidence; a `satisfies` link nothing checks will eventually be wrong. The design prefers links that execution can confirm (§6.5).

### 2.7 Core tooling, like source tooling, will eventually live in Resonance and be replaced by it [Decided]

Software development is collaborative, and artifacts of that collaboration leave a trail of essential context. Tickets in JIRA, commit messages in git, and comments on pull/merge requests are essential context that document both the history of a system, but the reasons why some particular set of decisions are suitable or not.

As such, Resonance offers a core data model that supports capturing this context as part of a larger set of collaborative tools. The leverage, however, is not that we can just capture particular messages, but that we can capture those messages *within and linking to the wider context* of a point-in-time snapshot of a (presumably) working system. Said another way, each type of message (ticket comment, source change, merge comment) is meta-commentary about the design and requirements that capture the evolution of those entities in relation to the solution/output, and is essential and easy to capture.

While Resonance will have to support living inside `git` for the foreseeable future (every team that adopts Resonance starts in git and keeps using git throughout adoption; see G9 and FR-071), Resonance will also simultaneously offer its own native change control system, mimicking the git command line interface though diverging in implementation and semantics.

Speculating ever further, Resonance-built solutions could someday support a mechanism of producing data packets that capture behavior of a running solution. This would give insight back to Resonance internal tools on how to optimize against real usage, but also could capture developer (or user) commentary on things to change or improve.
