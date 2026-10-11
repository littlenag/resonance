## 0. How to read this document

This specification is meant to be expanded over time and to be something an agent can implement from. To keep it honest about what has actually been decided, every significant statement carries one of three status labels:

- **[Decided]** — stated by Mark as a decision or position.
- **[Proposed]** — recommended during design discussion but not yet confirmed by Mark. Treat as the default until reviewed.
- **[Open]** — unresolved. Listed again in §14.

Requirements use stable IDs (`FR-###` functional, `NFR-###` non-functional, `OQ-##` open questions) and RFC 2119 keywords (MUST, SHOULD, MAY). Stable IDs matter here for a second reason: this document is intended to be the first body of intent the system ingests about itself (see §8.10), so its requirement IDs should never be renumbered. Retire an ID rather than reuse it.

Appendix A checks this specification against Mark's own statements and lists the tensions found between them.
