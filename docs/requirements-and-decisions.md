# Requirements and decisions

Requirements and decisions are your project's **intent**: what the software must do, and the choices made along the way. Resonance links them to the code and tests that realize them.

## Importing requirements from Markdown

Most requirements live in Markdown documents in your repository. Import a document once:

```
resonance import docs/requirements.md
```

You can import several documents at once (`resonance import spec/*.md`). Each imported path is added to `intent.sources` in `.resonance/config.toml`. From then on, every command picks up changes to those documents automatically, so you keep editing them as usual and never re-import.

### What Resonance recognizes

A requirement is a **bold key** at the start of a list item or a table row. A key is `FR-` or `NFR-` followed by a number, optionally with one lowercase letter (`FR-012a`).

List items:

```markdown
- **FR-001** The service records an item with a name and a quantity.
- **FR-003** Item names are normalized: trimmed and lower-cased.
  Whitespace inside a name is collapsed to a single space.
```

Indented lines that continue an item are part of its text.

Table rows, where the key is in the first cell and the text in the second:

```markdown
| ID | Requirement | Target |
| --- | --- | --- |
| **NFR-001** | Recording an item is fast | p95 under 5 ms |
```

Everything else in the document is ignored, including keys inside code blocks.

A trailing status label such as `[Decided]`, `[Proposed]` or `[Open]` (including qualified forms such as `[Proposed: needs review]`) is not part of the text. Resonance records it separately as the requirement's *spec status*:

```markdown
- **FR-002** The service rejects negative quantities. [Proposed]
```

Resonance records each requirement's document and line, shown as `source:` in `trace` and `show`.

### When documents change

- **Text changes.** When a requirement's text changes, the graph is updated and every link to it becomes `stale`, because whatever was linked to the old text needs checking against the new one. Changing only the status label doesn't make links stale.
- **New keys** become new requirements.
- **Removed keys.** A key that no longer appears in any imported document is **retired**, never deleted. Its links are kept, and if it still has links it goes on the repair list. If the key comes back, the requirement becomes active again.
- **The same key twice.** If a key is defined more than once, the first definition wins: documents are taken in the order they were imported, then by line.
- **Unreadable documents.** If an imported document is missing or isn't valid UTF-8, its requirements are kept as they were, and the document goes on the repair list until it's fixed.

To stop importing a document, remove it from `intent.sources` in `.resonance/config.toml`. The requirements that only it defined are then retired.

## Authoring requirements directly

Requirements don't have to come from a document. Create one with any key made of capital letters, a hyphen and a number:

```
resonance req add FR-004 --text "The inventory reports the total quantity."
```

The kind (`functional` or `non-functional`) follows from the key (`NFR-` keys are non-functional); override it with `--kind`.

Edit or retire it:

```
resonance req edit FR-004 --text "The inventory reports the total quantity of all items."
resonance req retire FR-004
```

Editing the text makes the requirement's links stale, as with imported requirements. Requirements are never deleted.

`req edit` and `req retire` work only on requirements created with `req add`. An imported requirement belongs to its document: edit the document instead, and Resonance picks up the change. If a document later defines a key that already exists from `req add`, the document's version is ignored and the conflict goes on the repair list so you can rename one of them.

## Decisions

Record a decision with a title and what was decided, optionally with its context and consequences:

```
resonance decision add \
  --title "Store items in memory" \
  --decision "Keep items in a dict; no database yet" \
  --context "Prototype stage" \
  --consequences "Restarting the service loses the inventory"
```

A new decision has the status `accepted`. Change any field later, including the status (for example `superseded` or `rejected`):

```
resonance decision edit "Store items in memory" --status superseded
```

Connect a decision to what it limits with a `constrains` link. A decision can constrain requirements, code units, tests, files and other decisions:

```
resonance link "Store items in memory" constrains record
```

Decisions that constrain a code unit, or any unit or file that contains it, appear in `resonance why` for that unit and in context packs. Refer to a decision by its title (ignoring case) or by its ID.
