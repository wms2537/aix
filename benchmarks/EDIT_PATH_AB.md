# Edit-path A/B (independent oracle) — what it does and does NOT show

This is a **programmatic edit-path** comparison, **not an agent study**. Adversarial
review was right that the earlier "agent" framing overclaimed; this is the honest
version.

## Setup (`agent_ab.py`)
- **Corpus:** 172 real formula-bearing workbooks (vendored calc-test corpus:
  FINANCIAL/LOOKUP/MATH — not author-written).
- **Task:** insert a blank row at row 2.
- **Path A:** `openpyxl.insert_rows` — the standard programmatic edit path.
- **Path B:** xlq certify-or-refuse (σ-shift + residual gate).
- **INDEPENDENT ORACLE:** LibreOffice recomputes each edited file; a formula value at
  its shifted position that diverges from the original Excel-authored cache =
  corruption. Independent of both openpyxl and xlq's IronCalc. Position-dependent
  functions (OFFSET, INDIRECT, ROW, COLUMN, NOW, …) are excluded — their value
  legitimately changes on a row insert.

## What is genuinely empirical (the result to trust)
- **xlq's edit is engine-confirmed faithful on 150/172 files, with 0 false
  certifications**, and **22 principled refusals** (shared/array formulas, tables —
  correctly declined, not corrupted). This is a real forward-correctness result for
  xlq's shifter on real workbooks against an independent engine, plus a working
  certify-or-refuse routing.

## What is NOT a clean interventional claim (do not headline)
- The **86.6%** openpyxl figure is a **corpus property × one known library bug**:
  `insert_rows` rewrites zero references, so 86.6% is the fraction of these files
  with a below-insert reference — not an agent-error rate. A competent ref-shifting
  engine (LibreOffice/Excel/IronCalc) also gets this op right, so the guard's real
  differentiator is auditability + explicit refusal + **engine-free certification**,
  which this experiment does not isolate.
- Path B has xlq **author** the edit and then **self-certify** it, so guarded
  "0% silent corruption" is partly **definitional** (a fail-closed gate cannot
  silently corrupt by construction). The certifier as a checker of **untrusted
  foreign** edits — the actual verifiability thesis — is tested in
  `foreign_certify.py`, not here.

## An oracle false-positive I found and fixed (transparency)
The first run reported 1 guarded "certified-but-WRONG" (`ROW_COLUM.xlsx`). It is
almost entirely `ROW()`/`COLUMN()` formulas, which resolve by absolute position —
`=ROW()` correctly returns a new number after the insert, so its cached value
legitimately changes. xlq shifted correctly; **my oracle's exclusion list was
incomplete** (missing ROW/COLUMN). Adding them → 0 false certifications. The "1
failure" was my measurement, not xlq.

## Honest standing
This delivers the reusable independent-oracle scaffold, xlq shifter
forward-correctness on 150 real files, and explicit refuse-routing — with disclosed
caveats. It does **not** close the interventional gate: a live agent's own varied
errors, certification of foreign edits, and task-completion scoring are the
remaining work (`foreign_certify.py` starts on foreign-edit certification).

## Correction (found by the coincidence-bound study, independently verified)

The 86.6% (149/172) unguarded silent-corruption rate contains **2 confirmed label
mislabels**: `ACCRINT.xlsx` and `ACCRINTM.xlsx` have formulas with **zero references
of any kind** (pure constant arguments), so a failure to shift references cannot
corrupt them — their "divergence" is LibreOffice recomputing ACCRINT differently from
Excel's cache after openpyxl drops `<v>`. Three further flagged files (`tables.xlsx`,
`defined_names*.xlsx`) carry only non-A1 reference classes (structured table refs,
defined-name targets) that openpyxl also fails to shift — corruption there is
plausible (name targets genuinely point at stale cells after the insert) but not
attributable by this value oracle. **Corrected headline: 147/172 = 85.5%
confirmed-genuine reference corruption** (144 A1-mediated + 3 via non-A1 reference
classes), 2/172 oracle mislabels. The deterministic multi-op result
(`shift_correctness_real`) is unaffected — it never used the value oracle.
