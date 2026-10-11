# Links and verification

Links connect your intent to your code. This page covers creating links, how their statuses change, and verifying them by running tests.

## Creating links

```
resonance link <source> <type> <target> [--confidence N]
```

| Type | Source | Target |
| --- | --- | --- |
| `satisfies` | code unit, test or file | requirement |
| `verifies` | test | requirement or code unit |
| `constrains` | decision | requirement, code unit, test, file or decision |

Examples:

```
resonance link record satisfies FR-001
resonance link src/schema.sql satisfies NFR-002
resonance link test_record verifies FR-001
resonance link test_record verifies record
resonance link "Store items in memory" constrains record
```

`--confidence` (0 to 1) records how sure you are that the link holds. Links made by agents always carry one. Linking the same source, type and target twice leaves the existing link alone and reports `Already linked`.

Both ends must be live: you can't link to a retired requirement or an orphaned code unit. Repair them first (see [The repair list](repair-list.md)).

Every command that creates or changes a link prints its ID, and a unique prefix of at least six characters is enough to refer to it later, like a short git hash:

```
Linked: inventory.py::record [asserted] satisfies FR-001 (Requirement)  link 01M4PNTS5X
```

## Link statuses

| From | Event | To |
| --- | --- | --- |
| (new) | `resonance link` | `asserted` |
| `asserted` or `stale` | a linked test passes | `verified` |
| `verified` or `stale` | a linked test fails | `asserted` |
| `asserted` or `verified` | the requirement's text changes | `stale` |
| `verified` | either end changes | `stale` |
| `stale` | `resonance confirm` | `asserted` |
| any other | an end disappears | `orphaned` |
| `orphaned` | the end comes back | the status it had before |
| `orphaned` | `resonance reattach` | `asserted` |

- **asserted**: someone said the link holds. Every link starts here, and editing the code under an asserted link leaves it asserted.
- **verified**: a passing test backs the link up. Only `verifies` links become verified.
- **stale**: something the link depended on changed. A link becomes stale when:
  - the requirement it points to has new text, whatever the link's status;
  - it was `verified`, and either end changed (the test's code, the code it verifies, or the requirement).
- **orphaned**: one end disappeared: its code unit couldn't be found, was removed, or its file was deleted. The link keeps the last known location.

Resonance records the content of both ends whenever a link is created, confirmed or verified, and compares against that record after each change.

## Resolving stale and orphaned links

Every stale or orphaned link is on the repair list, with a suggested command.

**Confirm** a stale link that still holds. It returns to `asserted`, and the current content of both ends becomes the new baseline:

```
resonance confirm 01M4PNTS8X
```

For a stale `verifies` link, re-running the test is usually better than confirming, since a pass returns it to `verified`:

```
resonance test 01M4PNTSCN
```

**Re-attach** an orphaned link (or one attached to the wrong thing) to a new target:

```
resonance reattach 01M4PNTS7C stock.py::validate
```

The new target replaces whichever end was lost. For a link whose ends are both still there, it replaces the code side of a `satisfies` or `verifies` link, and the target of a `constrains` link. A re-attached link starts again as `asserted`. To move the links of a retired requirement to the one that replaced it, re-attach each link to the new requirement.

**Remove** a link that no longer applies:

```
resonance unlink 01M4PNTS7C
```

Removed links are kept for history but no longer appear in queries.

## Verifying with tests

```
resonance test <reference>
```

| Reference | What runs |
| --- | --- |
| a requirement (`FR-001`) | every test linked to it by `verifies` |
| a code unit (`record`) | every test linked to it by `verifies` |
| a test (`test_record`) | that test |
| a `verifies` link ID | that link's test |

Each test runs once. A run updates *every* `verifies` link of that test, not only the ones you selected. In the example below, `test_record` verifies both FR-001 and FR-003, so testing FR-001 verifies both links:

```
$ resonance test FR-001
PASS  tests/test_inventory.py::test_record (174 ms)
      $ python3 -m unittest tests.test_inventory.InventoryTest.test_record
      verifies FR-001 -> verified
      verifies FR-003 -> verified
1 passed, 0 failed, 0 errors.
```

Every run is recorded as **evidence** with its result, exit code, the checked-out commit, the time, the Resonance version and the last 4 KiB of output. Evidence is linked to each `verifies` link the test supports, and the latest result appears in `trace`, `show` and context packs.

| Result | When | Effect on its `verifies` links |
| --- | --- | --- |
| **pass** | exit code 0 | become `verified`; their baseline is refreshed |
| **fail** | any other exit code | become `asserted` (a verified link loses its verification), with the failing evidence attached |
| **error** | the command couldn't start, it ran longer than `test.timeoutSeconds` (default 600), or its output matched `test.zeroTestsPattern` | stay as they are; an error is never a pass |

`test.zeroTestsPattern` defaults to `running 0 tests`, which catches a `cargo test` filter that matched nothing. Set it to whatever your test runner prints when nothing ran (see [Configuration](configuration.md)).

`resonance test` exits with status 1 if any run didn't pass, so you can use it in scripts.

## Finding gaps

```
$ resonance unverified
1 requirement with no verified test:
  FR-004    The inventory reports the total quantity.  [tests/test_inventory.py::test_total (asserted); 0 satisfying]
```

`unverified` lists every active requirement without a verified `verifies` link, with the tests linked so far and how many units satisfy it. To find everything with a link in a given status, use `resonance search --status stale` (or `asserted`, `verified`, `orphaned`).
