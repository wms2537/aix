# Receipt Journal Format — Open Spec v0.1 (DRAFT)

Status: **DRAFT.** This format is specified ahead of implementation; it ships
with `xlq apply` and `xlq calc --write` in **v0.2**. Nothing in xlq v0.1
reads or writes a journal. The draft is published now so the write path can
be reviewed before it exists, and so other tools can weigh in on the format.
Field names and semantics may change until the v0.2 release; after that, the
same versioning discipline as the census spec applies.

## Purpose

Every mutation of a workbook produces a **receipt**: a record binding the
exact file bytes before the change (`base_hash`) to the exact bytes after
(`result_hash`), plus the operations performed and the conditions they ran
under. Receipts accumulate in an append-only sidecar **journal**, one per
workbook, forming a hash chain. The chain makes two things mechanical:

1. **Auditability** — what changed, when, by which actor, under which engine.
2. **External-edit detection** — any edit made outside the tool breaks the
   chain in a detectable, recordable, non-fatal way.

The `.xlsx` file itself stays authoritative. The journal is bookkeeping
beside it, never a competing copy of the data.

## Files on disk

For a workbook `book.xlsx`, all artifacts live beside it:

| Path | Role |
|---|---|
| `book.xlsx` | The one authoritative working file. |
| `book.xlsx.xlq.jsonl` | The journal: append-only JSON Lines, one receipt per line. |
| `book.rev-N.xlsx` | Immutable history: the full workbook content produced by revision N. |
| `book.xlsx.xlq.lock` | Advisory lock file, present only while a mutating command runs. |

All artifacts are plain files with no absolute paths inside them, so the set
can live in git, Dropbox, or S3 without the tool knowing or caring.

## Receipt fields

Each journal line is one JSON object:

```json
{
  "rev": 4,
  "kind": "apply",
  "base_hash": "9f2c51…",
  "result_hash": "1d40ab…",
  "ops": [
    { "op": "set_formula", "sheet": "Consolidated", "cell": "D14",
      "formula": "=SUM(D2:D13)" }
  ],
  "timestamp": "2026-07-02T09:41:07Z",
  "actor": "claude-code/session-8c1f",
  "engine_version": "ironcalc 0.7.1+e50ccea8 (vendored master)",
  "clock": "2026-07-02T09:41:07Z",
  "seed": 1719913267
}
```

| Field | Type | Meaning |
|---|---|---|
| `rev` | integer | Revision number, starting at 1, strictly increasing by 1 per receipt. |
| `kind` | string | `"apply"`, `"recalc"`, or `"external_edit"` (see below). |
| `base_hash` | string | SHA-256 (lowercase hex) of the workbook bytes the operation started from. |
| `result_hash` | string | SHA-256 of the workbook bytes the operation produced. |
| `ops` | array | Typed operations performed. Empty array for `recalc` and `external_edit` receipts. |
| `timestamp` | string | ISO-8601 UTC instant the receipt was written. |
| `actor` | string | Provenance: the `--actor` flag, else the `XLQ_ACTOR` environment variable, else `"unknown"`. |
| `engine_version` | string | Engine name and version that performed the evaluation. |
| `clock` | string | The pinned instant used for volatile time functions (NOW, TODAY) during this operation. |
| `seed` | integer | The pinned seed used for volatile random functions (RAND, RANDBETWEEN). |

All hash checks are computed against the file path passed on the command
line.

### Hash chain invariant

Each receipt's `base_hash` MUST equal the previous receipt's `result_hash`.
A journal where this fails for any adjacent pair is corrupt and mutating
commands MUST refuse until it is repaired or replaced.

For the chain to be meaningful, `result_hash` must be reproducible: writing
the same logical workbook twice must yield identical bytes. Implementations
MUST normalize zip output — fixed entry mtimes, stable part ordering, fixed
compression level — otherwise the hash differs per run and the chain and all
reproducibility claims break.

### Genesis

A missing or empty journal means fresh start. The first apply records the
file's current hash as the genesis `base_hash` and proceeds — no
initialization command, no refusal on a journal-less file.

Rev files are never overwritten. After journal loss, numbering continues
from the highest existing `book.rev-N.xlsx` plus 1; a collision with an
existing rev file is an **error, not an overwrite**.

### External edits (`external_edit` receipts)

`external_edit_detected` fires whenever a journal exists and the file's
current hash differs from the **last** receipt's `result_hash`. This
includes the restore-an-old-rev case where the current hash matches some
*older* receipt — only the last one counts.

On detection, apply refuses the patch AND appends an external-edit marker
receipt that **adopts** the current hash as legitimate:

- `kind`: `"external_edit"`
- `base_hash`: the last receipt's `result_hash` (the expectation that broke)
- `result_hash`: the file's current observed hash (now adopted)
- `ops`: `[]`

A re-issued patch whose `base_hash` targets the adopted hash then proceeds.
An externally edited workbook is a broken chain to record, never a wedged
file.

## The apply contract (v0.2)

`xlq apply book.xlsx patch.json [--dry-run]`

### Patch file

The patch carries typed operations (`set_cell`, `set_formula`) and a
required `base_hash`. If the file's current hash differs from `base_hash`,
apply refuses with a `revision_mismatch` error whose payload is
`{expected, actual}`.

JSON→Excel type mapping for `set_cell` values — defined, never inferred:

| Patch value | Excel result |
|---|---|
| number | number |
| string | text |
| boolean | boolean |
| `null` | clear the cell |
| `{"type": "date", "iso": "YYYY-MM-DD"}` | date, converted to the workbook's date serial system |

Dates go through the typed wrapper so the number-vs-text and date-serial
traps are handled explicitly.

Optional patch inputs:

- `watch`: a list of cells (report-only). Dry-run and apply report
  before/after values for them, so a human can sanity-check magnitudes on
  key outputs — a plausible-but-wrong formula produces no error, so error
  detection alone is not enough.
- `clock`, `seed`: pinned values for volatile functions. When the patch
  supplies them, dry-run and apply share them and volatiles do not degrade
  the coverage flag. When absent, apply generates them (recorded in the
  receipt) and dry-run marks volatile-affected predictions unreliable.

### Dry-run report

`--dry-run` writes nothing and reports:

- affected cells;
- any **new** formula error, from the full set `#REF!`, `#VALUE!`,
  `#DIV/0!`, `#NAME?`, `#N/A`, `#NUM!`, `#NULL!`, `#SPILL!`, `#CALC!`;
- before/after values for the patch's `watch` cells;
- a `coverage` flag: when an unsupported OR volatile function appears in the
  affected dependency graph, predictions are marked unreliable rather than
  reported as fact.

### Real apply

1. Acquire the lock (below).
2. Verify `base_hash` against the file; verify the journal chain; run
   external-edit detection.
3. Write `book.rev-N.xlsx` beside the original as immutable history, where
   N = last receipt's rev + 1.
4. Atomically replace `book.xlsx` with the new content after fsync — the
   original path stays the one authoritative working file; rev files are
   history, not competing copies.
5. Append the receipt to the journal.
6. Release the lock.

`xlq calc --write` routes through the same rev-file + atomic-swap + receipt
path — there is never a bare in-place write. Recalc receipts have
`kind: "recalc"` and an empty `ops` array.

## Concurrency: the lock file

Every mutating command takes an advisory lock — `book.xlsx.xlq.lock`,
created with O_EXCL / flock — held across the whole
check → write → swap → journal-append sequence, and fails fast with a
defined `lock_held` error if it cannot be acquired. No waiting, no queueing:
the caller decides whether to retry.

## Error vocabulary

| Error | Meaning |
|---|---|
| `revision_mismatch` | Patch `base_hash` does not match the file's current hash. Payload: `{expected, actual}`. |
| `external_edit_detected` | Journal exists and the file's hash differs from the last receipt's `result_hash`. A marker receipt adopting the new hash is appended. |
| `lock_held` | Another mutating command holds the lock. |

## Draft status and known open items

- The exact JSON shape of `ops` entries and of the patch file envelope is
  the least settled part of this draft.
- The write strategy (surgical OOXML zip patching vs. verified lossless
  engine round-trip) is a prerequisite decision: naive re-serialization
  would destroy pass-through parts (VBA, pivot caches, charts, styles,
  external connections), violating the preserve-never-execute constraint.
  Whatever is chosen must keep untouched parts byte-identical and satisfy
  the zip-normalization requirement above.
- The source design document's journal-integrity and apply-contract sections
  were adversarially reviewed once, not to convergence; treat this spec
  accordingly until the v0.2 implementation lands with its test corpus.
