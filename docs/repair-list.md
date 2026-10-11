# The repair list

Resonance follows your code through ordinary edits, renames and moves on its own. Some changes need judgment, though: a function rewritten beyond recognition, a requirement whose meaning changed. Resonance doesn't guess about these. It puts them on the **repair list** and leaves the decision to you or your agent.

The repair list is brought up to date with every change. See it with:

```
resonance status
```

Agents get it from the `resonance_repair_list` and `resonance_status` tools. Each item shows what happened, the last known location and text where there is one, and a suggested command:

```
Repair list (2):
  [orphaned unit] code unit inventory.py::negative_check could not be found; last seen at inventory.py:17
      | if quantity < 0:
      -> find what it now corresponds to and run `resonance unit edit 01M4PNTRY7ECXK30A0WH56QKFR <path:START-END>`, or `resonance unit remove 01M4PNTRY7ECXK30A0WH56QKFR`
  [orphaned link] inventory.py::negative_check satisfies FR-002 lost an endpoint
      | if quantity < 0:
      -> re-attach it with `resonance reattach 01M4PNTS7CDK61B0NX5KFZQ2FF <target>`, or remove it with `resonance unlink 01M4PNTS7CDK61B0NX5KFZQ2FF`
```

## Items and how to resolve them

### Orphaned unit

A code unit's region couldn't be found after a change (see [How Resonance follows your code](code-units-and-tests.md#how-resonance-follows-your-code)). Its links and annotations are orphaned with it.

- If the code still exists somewhere, re-anchor the unit there. Its orphaned links and annotations are restored with it:
  ```
  resonance unit edit negative_check stock.py:17-18
  ```
- If the code was split into several pieces, declare them and use `resonance unit split`.
- If the code is gone for good, remove the unit and deal with its links:
  ```
  resonance unit remove negative_check
  ```
- If the change is temporary (a branch without this code, a deletion you'll undo), do nothing. The unit is restored when its text comes back.

### Low-confidence anchor

After a change, Resonance found text similar to the unit's old region, but less than 90% similar, so the match may be wrong. Look at where the unit is now (`resonance show <unit>`).

- If it's right, confirm it:
  ```
  resonance unit edit record
  ```
- If it's wrong, re-anchor it to the right region:
  ```
  resonance unit edit record stock.py:16-24
  ```

### Orphaned link

A link lost one of its ends, usually because a code unit was orphaned or removed, or a file was deleted. Fix the unit (above) and the link comes back. Otherwise, re-attach the link to whatever now plays that role, or remove it:

```
resonance reattach 01M4PNTS7C stock.py::validate
resonance unlink 01M4PNTS7C
```

### Orphaned annotation

An annotation lost its target. Re-attach it with `resonance reattach <annotation-id> <target>`, or fix the unit it was on.

### Stale link

Something the link depended on changed: the requirement's text changed, or an end of a verified link changed. Check that the link still holds, then:

- for a `verifies` link, re-run its test, which returns it to `verified` if it passes:
  ```
  resonance test 01M4PNTSCN
  ```
- for any link, confirm it:
  ```
  resonance confirm 01M4PNTS8X
  ```
- if it no longer holds, remove it with `resonance unlink`, or change the code.

### Unreadable intent source

An imported requirements document is missing or isn't valid UTF-8. Its requirements are kept exactly as they were until it's readable again. Fix or restore the file, or stop importing it by removing it from `intent.sources` in `.resonance/config.toml`.

### Retired requirement with links

A requirement was retired (removed from its document, or with `req retire`) but code or tests still link to it. Move the links to the requirement that replaced it, or remove them:

```
resonance reattach 01M4PNVZP3 FR-007
resonance unlink 01M4PNVZP3
```

### Requirement key conflict

A document defines a key that was already created with `resonance req add`. The document's version is ignored. Rename one of them.

## Working through the list with an agent

The instructions Resonance gives your agent include keeping anchors healthy: working through the repair list, re-anchoring orphaned and low-confidence units, re-attaching links and annotations, and confirming stale links that still hold. Deciding what rewritten code now corresponds to is exactly the kind of judgment an agent is good at, but it should ask you when it's unsure. A useful prompt is:

> Check the Resonance repair list and fix what you can; ask me about anything you're not sure of.
