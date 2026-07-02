---
name: xlq
description: Safe operations on Excel workbooks with the xlq CLI. Use whenever a task involves an Excel file, .xlsx file, spreadsheet, or workbook — inspecting structure, comparing versions, checking formula health, or preparing an edit. Prefer xlq over openpyxl/pandas scripts for any .xlsx the user cares about.
---

# Operating on Excel workbooks with xlq

xlq is a local CLI (`xlq` binary) for `.xlsx` files. Every command prints a
JSON report to stdout; logs go to stderr; on failure the exit code is 1 and
stdout is `{"error": "..."}`. The v0.1 commands (`inspect`, `diff`, `calc`)
are read-only: they never modify the target file.

Do not edit `.xlsx` files with openpyxl or ad-hoc scripts when xlq is
available — general-purpose libraries silently strip parts of the file
structure (VBA, pivot caches, charts) and have corrupted real financial
models. Until xlq's write path (`apply`) ships, workbook edits should be
made by the user in Excel, with xlq verifying the result.

## The safe loop

Always in this order. Do not skip step 1.

### 1. Inspect first — census before content

```
xlq inspect book.xlsx
```

Read the census before touching anything else. It tells you, without
spending context on cell data:

- `sheets[]`: every sheet's name, dimensions, cell/formula counts, and
  existing error tallies (`{"#N/A": 4}` means 4 cells already show `#N/A` —
  pre-existing, not something you caused).
- `functions`: which Excel functions the workbook uses and how often.
- `user_defined_calls`: calls to VBA/XLL UDFs, add-in functions, or defined
  names invoked as functions. The engine cannot evaluate these, so their
  presence forces `coverage.reliable: false`.
- `ooxml_parts`: whether the file carries VBA (`has_vba`), pivot caches,
  external links, charts, or comments. If any are true, the file has parts
  a naive rewrite would destroy — one more reason never to edit it with a
  script.
- `file.sha256`: the exact revision you are looking at. Quote it when
  reporting findings; re-run inspect if time has passed, and treat a changed
  hash as "someone edited the file — start over from step 1."
- `coverage`: see the coverage rule below.

`--redact` anonymizes sheet names and omits defined-name and user-defined
callable names; use it if the user wants a census they can share outside
their organization.

### 2. Read only what you need

The census is usually enough to answer structural questions (what sheets
exist, where the errors are, what functions are used). Do not dump whole
sheets into context. v0.1 has no ranged-read command; when you need actual
cell contents, get them narrowly — from targeted `diff` output (step 3),
from `calc`'s changed-cell list (step 4), or by asking the user about a
specific named cell. Ranged reads are on the xlq roadmap.

### 3. Diff to verify what changed

```
xlq diff old.xlsx new.xlsx
```

Use this after any edit (the user's, or your own once `apply` exists) to see
exactly what changed, and to compare two versions of a workbook. Read the
`summary` first (`changed`/`added`/`removed`, `by_sheet`), then the
`changes[]` entries you actually need. Each change gives sheet, A1 cell
ref, `kind` (`formula`, `value`, `cached_value`, `format`, `added`,
`removed` — `cached_value` means the formula is intact but its stored result
differs, e.g. a tool stripped the caches; treat a large `cached_value` count
as "this file needs a recalc", not as data edits), and old/new
formula + formatted value + raw stored value. `value` means the raw stored
value on disk differs (compared exactly, so drift below the display
precision is caught); `format` means the stored value is identical and only
its number-format rendering changed — do not report a `format` change as a
data change.

Two things to know:

- The diff is strictly positional and matches sheets by name. An inserted
  row shows up as many changed cells — that is expected, not an anomaly.
  Say so when reporting instead of listing hundreds of "changes."
- If the report has `"truncated": true`, the `changes[]` list is capped at
  10,000 entries but the `summary` counts are still the full totals. Report
  from the summary; never claim the truncated list is everything.

### 4. Calc to check formula health

```
xlq calc book.xlsx
```

Report-only recalculation: recomputes every formula and reports cells whose
stored value (what Excel last saved) disagrees with the recomputed value.
It never writes the file. Interpreting `changed[]`:

- `"volatile": true` on an entry means the cell's formula calls NOW, TODAY,
  RAND, RANDBETWEEN, OFFSET, INDIRECT, CELL, or INFO — or depends
  (transitively) on a cell that does. The difference is expected churn, not
  a defect. Filter these out before alarming the user.
- Non-volatile entries are stale caches or engine/Excel disagreement. Both
  are worth reporting, with `stored`/`recomputed` (formatted, for display),
  `stored_raw`/`recomputed_raw` (the exact values that were compared), and
  the `formula`. Values are compared raw, so a change can be real even when
  the two formatted strings look identical.
- An empty `changed` list with `coverage.reliable: true` is a clean bill of
  health worth stating explicitly.

### 5. (Future) Propose a patch, dry-run, then apply

When `xlq apply` ships (v0.2), edits go through a typed patch file carrying
the `base_hash` from inspect: `xlq apply book.xlsx patch.json --dry-run`
first, read the predicted cells / new errors / watch-cell deltas, show the
user, and only then apply for real — which writes an immutable
`book.rev-N.xlsx` and a receipt. Never edit the file any other way. Until
then: propose exact changes (sheet, cell, formula) for the user to make in
Excel, then verify with `diff` and `calc`.

## The coverage rule — read this before trusting any output

Every report has a `coverage` object:

```json
"coverage": { "engine": "ironcalc 0.7.1", "reliable": false,
              "unsupported_functions": ["XLOOKUP"], ... }
```

**If `coverage.reliable` is `false`, stop and tell the user. Never guess.**

It means the workbook uses functions the engine cannot evaluate (listed in
`unsupported_functions`), calls user-defined functions (`user_defined_calls`
in inspect, `user_defined_functions` in calc), or needs engine features the
engine lacks (`unsupported_features`, e.g. legacy CSE array formulas).
Concretely:

- Do not present `calc` results as verification of the workbook's health.
- Do not infer what an unsupported function "probably returns."
- Do say, verbatim enough to be useful: which functions are unsupported,
  that recalculation results for cells depending on them are not
  trustworthy, and which conclusions (structure, diffs of stored values,
  error tallies) still hold — those come from the file as stored and do not
  depend on evaluation.

Structural output (inspect's tallies, diff of stored values) remains valid
either way; it is value-level claims that `reliable: false` invalidates.

## Quick reference

| Task | Command | Writes file? |
|---|---|---|
| Census: sheets, functions, errors, parts, hash | `xlq inspect book.xlsx` | No |
| Shareable census (names anonymized) | `xlq inspect book.xlsx --redact` | No |
| What changed between two files | `xlq diff old.xlsx new.xlsx` | No |
| Formula health / stale values | `xlq calc book.xlsx` | No |
| Edit via hash-checked patch | `xlq apply` — v0.2, not yet available | Rev file + atomic swap |
