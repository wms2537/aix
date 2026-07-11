---
name: xlq-excel-edits
description: Use when reading, diffing, recalculating, or editing existing .xlsx/.xlsm Excel workbooks — changing cells or formulas, inserting/deleting/moving rows or columns, or validating a foreign edit — where formulas, charts, pivots, or number formats must survive intact. Especially when an agent would otherwise reach for openpyxl or pandas and risk silently corrupting the workbook.
---

# Editing Excel safely with xlq

## Overview

`xlq` is a command-line tool for **agent-safe** operations on `.xlsx`/`.xlsm`
workbooks. Its one guarantee: **never silently wrong.** An edit it cannot perform
faithfully is *refused* with a reason — never approximated. Every command prints
**JSON on stdout**; diagnostics go to stderr. Writes are transactional (receipts +
undo).

The default tool (openpyxl / pandas) rewrites the whole file and, on a real
workbook, **silently drops charts, pivots, and VBA, mangles number formats, and
does NOT shift formula references when you insert/delete rows** — corruption you
won't notice until the numbers are wrong. Use `xlq` whenever those must survive.

## The golden rule

**Check the exit code and the JSON `status`/`error`. A refusal is a feature.**

| Exit | Meaning |
|------|---------|
| `0` | The operation produced its effect/answer (JSON has the result). |
| `1` | An operational **refusal or failure** — a `REFUSED`, a residual it can't shift, a verification failure. |
| `2` | Malformed invocation (bad `--op`, missing args). |
| `70` | Internal error (rare; JSON `internal_error: true`). |

When `xlq` refuses (exit 1, or a `residuals`/`REFUSED` field), the edit **cannot be
made without corrupting the workbook.** Report that to the user and stop. **Do NOT
fall back to openpyxl/pandas to force it through** — that reintroduces exactly the
silent corruption `xlq` exists to prevent.

## Commands

| Command | Use |
|---------|-----|
| `xlq inspect <file>` | Census: sheets, formula/function tallies, error counts, chart/pivot/VBA presence, hash. No cell values. **Run this first.** |
| `xlq diff <old> <new>` | Cell-level diff of two workbooks (values + formulas). |
| `xlq calc <file>` | Recompute and report stored-vs-recomputed values. Read-only. |
| `xlq apply <file> <patch.json>` | Set cell values/formulas surgically (only touched sheet parts change). `--dry-run` predicts; `--schema` prints the patch format. |
| `xlq restructure <file> --sheet S --op OP --at N [--count K] [--dest D]` | Insert/delete/move rows/columns, shifting every reference. `--dry-run` predicts. |
| `xlq certify <orig> <edited> --sheet S --op OP --at N …` | Prove a *foreign* edit (e.g. openpyxl output) equals xlq's own faithful transform. |
| `xlq verify <file>` | Recompute the hash vs the latest receipt + the whole chain — detects out-of-band tampering. |
| `xlq undo <file>` | Restore the previous committed snapshot (records an `undo` receipt). |
| `xlq log <file>` | Print the receipt history. |

`OP` ∈ `insert-rows | delete-rows | insert-cols | delete-cols | move-rows`.
`--at` is **1-based**.

## Core workflows

Always **inspect first**, **dry-run before a real write**, and **verify after**.

### Insert / delete / move rows or columns (references must follow)
```sh
xlq inspect book.xlsx                                    # what's in it
xlq restructure book.xlsx --sheet Sheet1 --op insert-rows --at 5 --count 1 --dry-run
xlq restructure book.xlsx --sheet Sheet1 --op insert-rows --at 5 --count 1 --actor agent
xlq verify book.xlsx                                     # exit 0 = intact
```
If the dry-run/real run returns `error: residual_unreachable` with a `residuals`
list, the edit crosses something the shift algebra can't preserve (a shared/array
formula spanning the cut, a table, a CDATA formula body, a defined-name collision).
**That refusal is correct** — surface it; do not route around it.

### Change a cell value or formula
```sh
xlq apply --schema                                       # the patch JSON format
# write patch.json: {"base_hash": "<sha256 from inspect>", "ops": [ {"type":"set_cell","sheet":"Sheet1","cell":"B7","value":42} ]}
xlq apply book.xlsx patch.json --dry-run                 # predicts affected cells + new errors
xlq apply book.xlsx patch.json --actor agent
```
`base_hash` is the file's current sha256 (from `inspect`); the write refuses on
mismatch. Values: number/string/bool/null (null clears); a date is
`{"type":"date","iso":"YYYY-MM-DD"}`. Formulas: `{"type":"set_formula", ... ,"formula":"=A1+1"}`.

### Recover a mistake
```sh
xlq undo book.xlsx        # restores the prior snapshot; fails closed if no backup
xlq log book.xlsx         # see the receipt history
```

### Validate a foreign edit (e.g. something openpyxl produced)
```sh
xlq certify original.xlsx edited.xlsx --sheet Sheet1 --op insert-rows --at 5 --count 1
```
`status: CERTIFIED` (exit 0) means the foreign edit equals xlq's proven transform;
`REFUSED` (exit 1) means it differs (a formula/value/added/removed change) — do not
trust it.

## Common mistakes

- **Reaching for openpyxl/pandas to edit an existing workbook.** They silently drop
  charts/pivots and don't shift formula refs. Use `xlq`.
- **Treating a refusal as a blocker to work around.** It's the safe answer. Report it.
- **Skipping the exit-code / JSON check.** Parse stdout; branch on exit code.
- **Not dry-running, or not verifying after a write.**
- **Wrong verb:** cell values/formulas → `apply`; row/column insert/delete/move →
  `restructure`.

## Notes

- Reads never modify the file. A real write creates `<file>.xlq.jsonl` (journal) and
  `<file>.rev-N.xlsx` (backups) alongside it — that's normal and enables undo/verify.
- Decompression is bounded against zip bombs (override with `XLQ_MAX_PART_BYTES` /
  `XLQ_MAX_TOTAL_BYTES` only if a legitimate workbook is refused as too large).
- `xlq` is not for building a workbook from scratch — it edits existing ones.
