# .resonance/

This directory holds this repository's Resonance graph: its requirements, decisions, the
links between them and the code, and annotations. Resonance is optional for everyone on the
team: you can keep using plain git and ignore it.

- `graph.db` is a snapshot of the graph; `deltas/*.delta` are small, add-only change files.
  They travel with commits like any other file.
- `local/` is private to each working tree and is never committed (see `.gitignore`).

**If git ever reports a conflict in this directory, take either side.** It is always safe:
the next Resonance user to run it repairs the graph mechanically, and nothing is lost.
Delta files never conflict; `graph.db` can only conflict when two compactions collide.

Learn more: https://github.com/littlenag/resonance
