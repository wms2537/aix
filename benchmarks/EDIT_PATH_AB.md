# Interventional A/B: does the guard eliminate silent corruption? (independent oracle)

The gating experiment the reviewers named since round 1: not "does the certifier
fire on cases I wrote," but "does routing a real edit through the certify-or-refuse
guard prevent silent corruption, judged by an engine INDEPENDENT of the tool?"

## Setup (`agent_ab.py`)
- **Corpus:** 172 real formula-bearing workbooks (the vendored calc-test corpus:
  FINANCIAL, LOOKUP, MATH, … — not authored by us).
- **Task:** insert a blank row at row 2 — the canonical *invisible-damage*
  structural edit (every formula that references below the insert must shift; a
  wrong shift changes recomputed values with no visible symptom).
- **ARM A — UNGUARDED:** edit with `openpyxl` (`insert_rows`), the library an
  agent reaches for. The file opens fine; correctness is invisible without an engine.
- **ARM B — GUARDED:** the same structural intent through xlq's certify-or-refuse
  (σ-shift + residual gate). Certified → commit; can't certify → REFUSE.
- **INDEPENDENT ORACLE:** LibreOffice recomputes each edited file; a formula value
  at its shifted position that diverges from the original Excel-authored cache =
  corruption. LibreOffice is independent of both openpyxl and xlq's IronCalc, and
  the ground-truth caches were written by Excel. Position-dependent functions
  (OFFSET, INDIRECT, ROW, COLUMN, NOW, …) are excluded — their value legitimately
  changes on a row insert, so value-preservation does not apply to them.

## Result

| | silent corruption | notes |
|---|---:|---|
| **UNGUARDED** (openpyxl) | **149 / 172 = 86.6%** | 23 faithful — files whose checkable formulas happened not to cross the insert |
| **GUARDED** (xlq) | **0 / 172 = 0%** | 150 certified-faithful (engine-confirmed), 22 refused (explicit) |

The naive edit path silently corrupts ~7 of every 8 real structural edits; the
guarded path silently corrupts none — every commit is engine-confirmed faithful
or explicitly refused. This is the certify-or-refuse contract holding on real
files against an independent engine.

## An oracle false-positive I found and fixed (transparency)
The first full run reported **1** guarded "certified-but-WRONG"
(`ROW_COLUM.xlsx`). Investigated: the file is almost entirely `ROW()`/`COLUMN()`
formulas, which resolve by absolute position — `=ROW()` at row 2 *correctly*
returns 3 after the insert, so its cached value legitimately changes. xlq shifted
correctly; **my oracle's exclusion list was incomplete** (it had OFFSET/INDIRECT
but not ROW/COLUMN). Adding them flipped the file to certified-faithful and the
guard-failure count to 0. The "1 guard failure" was my measurement, not xlq.

## Honest scope — what this is and is NOT
- It IS a real interventional A/B on the corruption *mechanism*: guarded vs
  unguarded edit paths, 172 real workbooks, independent engine oracle, n≫ the
  earlier 6-case harness.
- ARM A is `openpyxl` — the standard programmatic edit path — **not a live LLM
  writing varied edits.** The corruption is that tool's systematic failure to
  shift references. A live-LLM slice (an agent making its own varied mistakes,
  gated by the router) is the stronger version and remains the next step; this
  establishes the mechanism and the independent-oracle methodology it needs.
- One structural operation (row insert) and one oracle engine (LibreOffice). The
  22 refusals are the guard declining files it cannot certify (shared/array
  formulas, tables) — correct behavior, not corruption.

Reproduce: `abenv/bin/python agent_ab.py 0 207` (needs openpyxl + libreoffice).
