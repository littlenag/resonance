# Inventory service requirements

## Functional

- **FR-001** The service records an item with a name and a quantity. [Decided]
- **FR-002** The service rejects negative quantities. [Proposed]
- **FR-003** Item names are normalized: trimmed and lower-cased.
  Whitespace inside a name is collapsed to a single space.
- **FR-004** The web client shows the total quantity of all items. [Open]
- **FR-005** The command-line tool prints the inventory as a table.

## Non-functional

| ID | Requirement | Target | Status |
| --- | --- | --- | --- |
| **NFR-001** | Recording an item is fast | p95 under 5 ms | [Open] |
| **NFR-002** | No item is ever lost on restart | always | [Proposed] |
