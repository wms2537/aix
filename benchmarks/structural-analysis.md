# E-structural: surgical structural edits vs the status quo

**Operation:** insert 1 row at row 5 of sheet `Data` in `full.xlsx` — a workbook
with a bar chart, a straddling `=SUM(B2:B7)`, an absolute `=$B$8`, a merged
region, a cross-sheet `Report!A1 =Data!B8`, a defined name `Total = Data!$B$8`,
and a chart data reference `Data!$B$2:$B$7`.

A structural edit is the case where fidelity-preservation and correctness appear
to CONFLICT: a correct edit MUST rewrite reference-bearing parts (a chart on
`B2:B7` must grow to `B2:B8`), so byte-identity of those parts is impossible.
The right invariant is the *minimal reference-shift patch*: the changed parts
differ ONLY in reference coordinates; every non-reference part is byte-identical.

## Results (benchmarks/structural.json)

| Tool | Correctness (references shifted right) | Fidelity | Recompute |
|---|---|---|---|
| **xlq restructure** | **6 / 6** | 10/14 parts byte-identical; the 4 changed parts differ ONLY in coordinates | correct (IronCalc, unit-tested) |
| openpyxl `insert_rows` | **0 / 6** | 12/14 parts, but references unshifted | **silently wrong** |
| LibreOffice round-trip | correct-by-engine | **0/14** parts byte-identical | correct-by-engine |

### xlq: correct AND minimal
All six reference classes shift to their documented target:
- `=SUM(B2:B7)` → moves to B9, **grows** to `=SUM(B2:B8)` (straddle);
- `=B5*2` → moves to B10, `=B6*2` (single ref at the insert row shifts);
- `=$B$8` → `=$B$9` (absolute is NOT exempt from a structural shift);
- `Report!A1 =Data!B8` → `=Data!B9` (cross-sheet, into the edited sheet);
- defined name `Total = Data!$B$8` → `Data!$B$9`;
- chart data `Data!$B$2:$B$7` → `Data!$B$2:$B$8` (grows).

Only 4 parts changed — the edited sheet, the cross-referencing sheet, the chart,
and workbook.xml (the defined name). Every other part (styles, theme,
sharedStrings, the merged-cell structure, drawing relationships) is
**byte-identical**. The three non-sheet changed parts are *digit-stripped
identical* to the original — i.e. the ONLY bytes that changed are numeric
coordinates. The edited sheet legitimately grows (a `<row>` was inserted). This
is the minimal-patch invariant, verified.

### openpyxl: fidelity is moot when every reference is wrong
`insert_rows` moves the cells but does NOT rewrite a single formula reference.
After the edit: `=SUM(B2:B7)` still reads the pre-insert range (now shifted data
→ wrong total); `=B5*2` now multiplies the **blank inserted row** (→ 0); the
cross-sheet ref, defined name, and chart all point at stale cells. The file
opens without complaint and computes **silently wrong** values — the
worst failure mode. Its 12/14 "fidelity" is irrelevant: the numbers are wrong.

### LibreOffice: correct engine, destroyed provenance
A real engine shifts references correctly, but even a bare load-save round-trip
(before any edit) rewrites **every one of the 14 parts** — 0/14 byte-identical.
Byte-provenance, and any part LibreOffice does not fully model, is lost on every
save. Correct arithmetic, no fidelity.

## The contribution, measured
xlq is the only tool that is simultaneously **correct** (6/6 references shifted
to their documented target, recompute-equivalent) and **fidelity-preserving**
(minimal-patch: only coordinate bytes change). This is the case prior work could
not do: Excelsior shifts references but regenerates the file (fidelity lost);
openpyxl preserves bytes but breaks references (correctness lost); LibreOffice is
correct but rewrites everything. The reference-shift algebra σ + minimal-patch
OOXML surgery resolves the conflict.

## Coverage (after adversarial review — research-log/014)
A 28-agent adversarial review found 9 confirmed defects, all now fixed. The
invariant is: for every reference-bearing construct, xlq EITHER shifts it
correctly OR refuses the edit — never silently wrong.

**Shifts correctly:** cell `<f>` formulas, `<c r>`/`<row r>` coordinates,
cross-sheet refs (sheet-scoped), defined names, chart `<c:f>`, pivot
`worksheetSource`, mergeCell/CF/DV/dimension/selection `sqref`, CF `<formula>` +
DV `<formula1/2>` bodies, whole-column/whole-row refs, absolute refs, 3D spans
anchored on the edited sheet. Recompute is EXECUTED in the eval (`xlq calc` →
SUM = 760, matching the pre-edit total).

**Refuses (residual — never silently wrong):** shared-formula groups, array
formulas, workbooks containing table parts, 3D spans not anchored on the edited
sheet.

## Honest scope / limitations
- **Shared formulas are the key limitation.** They are common (Excel/autofill
  emit them), and xlq REFUSES any edit touching them — safe but limiting.
  Shared-formula EXPANSION (materialize → shift) is the clear next contribution.
- One end-to-end fixture, one operation (insert-row); delete-row and column ops
  share the same σ (unit-tested, incl. the whole-column column-op cases) but are
  not yet in this end-to-end table.
- LibreOffice's insert-row (vs round-trip) needs a Basic macro; its
  reference-shift correctness is stated by engine, not measured here.
