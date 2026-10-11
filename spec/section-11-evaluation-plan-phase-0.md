## 11. Evaluation plan (Phase 0)

Before building Resonance, test the hypothesis on real work.

### 11.1 Hypothesis  [Decided]

> Adding structure to an agent's repertoire decreases tokens used and wall-clock time, and, if fully leveraged in the way envisioned here, also reduces developer time.

Because Mark is also skeptical that *code* structure is a first-order benefit (§2.5), the evaluation separates two kinds of structure. [Proposed]

- **H1 — Code structure.** Symbol and reference structure (Mache-like) reduces tokens and wall-clock time on bug fixes and features. Expected by Mark to be a small effect.
- **H2 — Intent structure.** Linked requirements, decisions, tests and annotations reduce tokens, wall-clock time **and developer time**. This is the thesis behind Resonance.

### 11.2 Tasks  [Decided]

- **"Fix this bug"** and **"add this feature"** tasks.
- On an existing, fairly large codebase. The codebase at Mark's workplace is the ideal candidate, subject to employer policy on sending source code to model providers. A large open-source codebase in a similar language is the fallback.

Proposed task selection:

- Draw tasks from the codebase's own history (closed bugs and shipped features) so that there is a known-good outcome to compare against. Aim for about 10 bugs and 10 features.
- Phrase tasks the way colleagues actually write them, not as formal issue text. One study found that rewriting benchmark issues into realistic user-style requests cut measured pass rates by over 50% for some models.
- For each task, check out the commit before the historical fix and remove later commits from the sandbox's history, so the agent cannot find the answer in git.

### 11.3 Arms  [Proposed]

| Arm | Agent has | Tests |
| --- | --- | --- |
| **A — Baseline** | The coding agent with its normal tools (search, read, edit, shell) | — |
| **B — Code structure** | A + an off-the-shelf code-structure MCP server (CodeGraph, lxDIG MCP or Mache) | H1 |
| **C — Intent structure** | A + a hand-built intent graph for the tasks' areas: requirements, decisions, test links and annotations, served through a minimal MCP tool or even structured files | H2 |
| **D — Both** (optional) | B + C | Interaction |

Using off-the-shelf tools for arm B tests code structure itself, not the quality of a new implementation. Arm C needs no Resonance code: a small curated graph is enough to measure whether intent context changes outcomes.

### 11.4 Metrics

| Metric | Source | Status |
| --- | --- | --- |
| Tokens (input and output) | Agent logs | [Decided] |
| Wall-clock time to completion | Agent logs | [Decided] |
| Developer time | Count of developer interventions (redirections, corrections, clarifications) per task, plus time to review and accept | [Decided] as a metric; [Proposed] as a measurement method |
| Task success | Tests pass and the developer would accept the change | [Proposed] |
| Defects | Regressions found by the existing test suite, plus issues found in review | [Proposed] |
| Cost | Provider billing or token pricing | [Proposed] |
| Tool calls and files read | Agent logs | [Proposed] |

### 11.5 Protocol  [Proposed]

- Hold the model and model version fixed across arms.
- Run each task in each arm at least three times; results vary run to run.
- Use a fresh sandbox per run.
- Analyze paired results per task (same task across arms).
- Write down the decision thresholds before running (§11.6).

### 11.6 Decision gates  [Proposed]

| Outcome | Consequence |
| --- | --- |
| B shows little or no gain over A | Confirms §2.5. Keep §8.9 minimal. |
| C shows clear gains over A, especially in developer time | Proceed to Phase 1 with the intent layer as the priority. |
| B shows large gains | Revisit NG4 and invest more in §8.9, preferring compiler-grade data. |
| Neither B nor C helps | Revisit the thesis before building. |
