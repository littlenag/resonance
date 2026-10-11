# Annotations

An annotation is a note attached to a code unit, a test, a file, a requirement or a decision. Annotations are how you tell the next person or agent what the code can't say for itself: what not to change, what is fragile, what matters for a task.

Annotations live only in the graph. They never appear in your source files.

## Kinds

| Kind | Use it for | How agents treat it |
| --- | --- | --- |
| `note` | Background and explanation. | Information. |
| `warning` | Something fragile or surprising. | A constraint on changes. |
| `instruction` | A rule for anyone changing this code. | A constraint on changes. |
| `call-out` | Code that matters for a particular task, optionally named with `--task`. | Pulled into context packs for that task. |

## Adding annotations

```
resonance annotate record --kind instruction \
  --text "keep record() idempotent: callers retry on timeout"

resonance annotate src/schema.sql --kind warning \
  --text "column order is relied on by the nightly export"

resonance annotate normalize --kind call-out --task unicode-names \
  --text "lower() is not casefold(); revisit for non-ASCII names"
```

The target can be any reference: a unit, a test, a file path, a requirement key or a decision. `--task` can only be used with call-outs. A task label is any text you choose; use the same label on every call-out for one piece of work.

## Where annotations appear

- `resonance why <unit>` shows the annotations on the unit, and on every unit or file that contains it. An instruction on a file applies to every unit in it.
- `resonance context <requirements…>` includes the annotations on those requirements, on the code that satisfies them (and what contains that code), and on the tests that verify them.
- `resonance context <requirements…> --task <label>` also lists every call-out made for that task, wherever it's attached.
- `resonance show <reference>` lists the annotations on one node.
- `resonance search` finds annotations by their text (`--type Annotation`).

## When the code changes

An annotation follows its code unit through edits, renames and moves, like a link does. If the unit is orphaned, the annotation is orphaned too and goes on the repair list. If the unit is restored, so is the annotation. If you know where the annotation belongs, re-attach it by its ID:

```
resonance reattach 01M4PNTXQ4 stock.py::record_item
```

When a unit is split or merged with `unit split` or `unit merge`, its annotations are copied to the new units.
