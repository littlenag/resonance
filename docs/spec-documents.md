# Your spec documents

Most projects keep requirements, decisions and designs in documents: a `requirements.md`, a Spec Kit or Kiro spec, EARS statements, a directory of ADRs. Resonance doesn't read any of them. Your agent does, and brings what it finds into the graph, where it can be linked to code and tests, traced and verified.

## Bringing a document in

Ask your agent:

> Bring docs/requirements.md into Resonance.

> We keep our decisions in docs/adr. Bring them in.

The agent:

1. **Reads the document** and works out the items: their type (requirement, decision, design, constraint), keys, fields, and the lines each came from. It keeps the keys the document uses (`FR-042`, `ADR-0007`). Where an item has none, it proposes one and tells you.
2. **Shows you a preview** before anything is written: what will be created, updated or left unchanged, and anything that's wrong, such as a key already used by an item of another type. Resonance checks the whole set, and writes nothing unless every item is valid.
3. **Proposes reformatting**, when it would help the document work well with the graph: one item per list entry, each starting with its key in bold. It shows you the change first, and makes it only with your agreement.
4. **Brings the items in.** Each one is linked to the region of the document it came from.

Any format your agent can read works. It knows the common ones: Markdown with keyed list items or tables, GitHub Spec Kit `spec.md`, Kiro `requirements.md` and `design.md`, EARS-style statements, and ADR directories (each record becomes a decision keyed `ADR-NNNN`, with its status, context and consequences).

## The graph is the source of truth

Once an item is in the graph, the graph owns it. The document stays, as the item's readable companion, and the link between them tells Resonance when the two drift apart:

| What changed | What happens |
| --- | --- |
| You edit the item in `resonance tui` (or your agent changes it) | The link to the document becomes stale. Your agent updates the document to match. |
| You (or a teammate) edit the document | The link becomes stale, and the item stays as it was. Your agent shows you the difference and, once you agree, applies it to the item. |
| An item disappears from the document | Your agent retires it, with your agreement. Items are never deleted. |

A stale link to a document is also on the repair list, so nothing drifts silently. You can ask your agent at any time:

> Are our spec documents in step with Resonance?

## Writing a document from the graph

The other way works too. Ask your agent to write the requirements you created in the TUI, or in conversation, into a document: it writes the file, shows it to you, and links each item to its region, after which the document is kept in step like any other.

## Tags in code

Some teams mark code with tags such as `// @satisfies FR-003`. Resonance doesn't scan for them, but your agent can read them and turn them into links when you ask it to.
