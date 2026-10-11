# Code units and tests

Resonance parses no source code. Instead, you and your agent **declare** the regions of files that matter, and Resonance keeps them attached as the files change. Because nothing is parsed, this works the same for every language and for any text file: source code, configuration, SQL, Markdown, shell scripts.

## Declaring a code unit

Give a region, a name and optionally a kind:

```
resonance unit add src/store.rs:40-95 --name open --kind function
```

A region is one of:

| Form | Meaning |
| --- | --- |
| `path:START-END` | Lines START to END, counted from 1, both included. |
| `path:LINE` | One line. |
| `path --match TEXT` | An exact snippet, which must occur exactly once in the file. The unit covers just that text, not whole lines. |

```
resonance unit add inventory.py --match 'if quantity < 0:' --name negative_check --kind block
```

Paths are relative to your current directory (or to the directory given with `-C`). The file must be a text file that Resonance reads: not ignored, not binary, and not larger than `ingest.maxBlobBytes` (see [Configuration](configuration.md)).

**Names** can't contain `::` or `#`, or start or end with spaces. They don't have to be unique, but a unique name is the easiest way to refer to a unit.

**Kind** is free text, for example `function`, `type`, `method`, `block`, `query` or `config`. It defaults to `block` for code units and `test` for tests. Resonance only stores it and shows it.

Each unit gets an **address**, `<path>::<name>`, such as `src/store.rs::open`. If a file has several units with the same name, the later ones are `src/store.rs::open#2`, `#3`, and so on.

## Listing units

```
resonance unit list            # everything
resonance unit list src/       # under a directory
resonance unit list src/lib.rs # in one file
```

Each line shows the current location, the address, whether it's a unit or a test, the kind, and its state:

| State | Meaning |
| --- | --- |
| `attached` | Resonance knows where the region is. |
| `orphaned` | The region couldn't be found after a change. Its last known location and text are kept. |
| `removed` | Removed with `unit remove`, or retired by a split or merge. |

## Declaring a test

A test is a code unit with a **run command**: the exact shell command that runs that one test.

```
resonance test add tests/store.rs:10-20 --name opens_existing \
  --run "cargo test --test store -- --exact opens_existing"
```

The command runs with `sh -c` from the repository root. Exit code 0 is a pass. Any other exit code is a fail. Make the command as narrow as you can, so that a pass means *this* test passed.

If many tests share a pattern, set `test.command` in `.resonance/config.toml` and leave out `--run`. `{name}`, `{path}` and `{address}` are replaced with the test's name, file path and address:

```toml
[test]
command = "cargo test -- --exact {name}"
```

See [Links and verification](links-and-verification.md) for running tests and what their results do.

## How Resonance follows your code

After every change to a file, Resonance re-locates every unit in it, trying these steps in order:

1. **Follow the edit.** Resonance compares the old and new file. If at least half of the unit's lines survive, the unit follows them, including lines inserted inside it. A line still counts as surviving if it was edited lightly (at least 60% of its characters are unchanged). Snippet units keep their exact columns. Edits elsewhere in the file just shift the unit up or down.
2. **Find the same text in the same file.** If the region's exact text is still in the file, for example because a function was moved further down, the unit moves to it. If the text occurs several times, the surrounding lines decide.
3. **Find the same text in another file.** If the region was moved to a different file, the unit moves there. This works whether or not the other file changed at the same time.
4. **Find similar text in the same file.** Resonance looks for the window most similar to the old text (at least 80% similar, with agreeing surroundings). The unit moves there, and the match is recorded with its confidence. A match below 90% goes on the repair list as a *low-confidence anchor*, for someone to check.
5. **Orphan.** If nothing matches, the unit becomes `orphaned`. Its links and annotations become orphaned too. None of them are deleted, and all of them go on the repair list with the last known location and text.

Steps 1 and 4 never re-attach a region that was rewritten beyond recognition. Resonance would rather orphan a unit and ask than guess wrong.

### Renamed and moved files

When a file is renamed or moved, its units' addresses follow it: `inventory.py::record` becomes `stock.py::record`. This works whether you rename with `git mv`, your editor or anything else. It also works when the file is edited in the same step, as long as the new file is at least half similar to the old one. Links and annotations on the file itself follow it too.

### Orphaning is reversible

If an orphaned unit's exact text comes back with matching surroundings, the unit is restored and its links return to the status they had before. Typical cases are switching to a branch without the code and back again, or undoing a deletion.

### Branches

Resonance keeps one graph for the repository, not one per branch. A branch switch is just a large change to your files, applied as a single batch once git finishes. Units whose code isn't on the new branch are orphaned until you switch back.

## Fixing units by hand

Most fixes are what the repair list suggests.

**Re-anchor** a unit to a new region, for example after a rewrite:

```
resonance unit edit record stock.py:16-24
resonance unit edit negative_check stock.py --match 'if quantity <= 0:'
```

A re-anchored unit keeps its ID, links and annotations. An orphaned unit becomes attached again, and its orphaned links are restored.

**Confirm** a low-confidence anchor that is actually correct by running `unit edit` with nothing to change:

```
resonance unit edit record
```

**Rename** a unit, change its kind, or change a test's run command:

```
resonance unit edit record --name record_item
resonance unit edit test_record --run "python3 -m unittest tests.test_inventory"
```

**Remove** a unit that no longer exists:

```
resonance unit remove negative_check
```

The unit is retired, not deleted, and its links and annotations become orphaned, so you can re-attach them elsewhere with `resonance reattach`.

## Splits and merges

When one piece of code becomes several, or several become one, tell Resonance so the links and annotations go to the right place.

**Split.** Declare the new units first, then split the old one into them:

```
resonance unit add src/store.rs:40-60 --name open_connection --kind function
resonance unit add src/store.rs:62-80 --name open_schema --kind function
resonance unit split open --into open_connection open_schema
```

**Merge.** Declare or pick the unit that remains, then merge the others into it:

```
resonance unit merge open close --into lifecycle
```

The old units' links and annotations are copied to the new units as `asserted`, and the old units are retired. The new units must be attached; the old ones may already be orphaned, which is common after a rewrite.

## Large and binary files

Files that contain NUL bytes, or that are larger than `ingest.maxBlobBytes` (5 MiB by default), are recorded with their hash and size only. You can link and annotate them as whole files. Code units can only be declared in text files. Resonance keeps no copy of a file over the size limit, so it can't follow a unit in one through an edit, and the unit is orphaned. Declare units only in files under the limit, or raise the limit.
