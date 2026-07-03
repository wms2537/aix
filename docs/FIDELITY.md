# E1 — Fidelity Preservation

**Claim under test:** when an agent makes a single-cell edit to a real
workbook, does the tool preserve everything it did not touch — charts, pivot
caches, VBA macros, shared strings, styles — or does it quietly rewrite (or
drop) parts of the file the user never asked to change?

We take one representative edit per fixture ("set this data cell to that
number" — the edit an agent actually makes), apply the **same logical edit
three ways**, and diff the resulting OOXML package against the untouched
original at the level of individual zip members (parts).

- **xlq apply** — surgical typed patch.
- **openpyxl** — `load_workbook` → set cell → `save` (the common agent path;
  for `macro.xlsm` this uses the **default `keep_vba=False`**).
- **LibreOffice** — `soffice --convert-to` re-save proxy (see caveat below).

Reproduce: `python benchmarks/fidelity.py` → writes `benchmarks/fidelity.json`.
Numbers below are copied from that file; parts are counted as **zip members**
of the package.

---

## Headline

- **xlq preserves every non-edited part byte-for-byte.** Across the four
  fixtures it rewrites **only** the worksheet part(s) that actually contain a
  changed cell value, and drops **only** `xl/calcChain.xml` (a derived cache
  Excel regenerates). Charts, pivot tables, pivot caches, VBA `vbaProject.bin`,
  shared strings, styles, drawings, printer settings, comments — all
  **byte-identical** to the input.
- **openpyxl silently drops features.** On `macro.xlsm` it drops
  `xl/vbaProject.bin` — **all VBA** — and re-saves as `.xlsx`. On
  `pivot-chart.xlsx` it drops **6 of the 8 chart-support parts** (chart
  relationships, colors, styles), drops `sharedStrings.xml`, renames the
  comment parts, and its output **fails to re-open in the ironcalc engine**.
  It drops `sharedStrings.xml` on **every** fixture.
- **LibreOffice rewrites 100%.** A headless re-save rewrites **every single
  part** (0 byte-identical on all four fixtures) and injects new parts. The
  features survive, but nothing is preserved verbatim.

---

## Per-fixture results

Legend: **identical** = parts byte-identical to the original / total original
parts. **rw** = parts rewritten. **drop / add** = parts dropped / added.
**ironcalc / soffice** = output re-opens in that engine.

### `pivot-chart.xlsx` — 2 charts, 1 pivot table, 2 pivot caches (50 parts)
Edit: `Sheet1!A2` 222 → 999.

| Tool | identical | rw | drop | add | charts | pivot | vba | ironcalc | soffice |
|------|-----------|----|------|-----|--------|-------|-----|----------|---------|
| **xlq** | **48 / 50** | 1 | 1 | 0 | ✅ byte-identical | ✅ byte-identical | — | ✅ | ✅ |
| openpyxl | 1 / 50 | 36 | 13 | 4 | ⚠️ degraded (6 support parts dropped) | ⚠️ rewritten | — | ❌ | ✅ |
| LibreOffice | 0 / 50 | 37 | 13 | 6 | ⚠️ degraded (same 6 support parts dropped) + rewritten | ⚠️ rewritten | — | ✅ | ✅ |

- **xlq** rewrites only `xl/worksheets/sheet1.xml` (the edited sheet) and drops
  the stale `xl/calcChain.xml`. **All 48 remaining parts — both charts, the
  pivot table, both pivot caches, shared strings, styles, drawings, comments,
  printer settings — are byte-identical.**
- **openpyxl** drops: `xl/charts/_rels/chart1.xml.rels`,
  `xl/charts/_rels/chart2.xml.rels`, `xl/charts/colors1.xml`,
  `xl/charts/colors2.xml`, `xl/charts/style1.xml`, `xl/charts/style2.xml`
  (chart theming + wiring), plus `sharedStrings.xml`, both `comments*.xml`,
  both `vmlDrawing*.vml`, `printerSettings1.bin`, and `calcChain.xml`. The core
  `chart1/2.xml` and pivot parts are *present but rewritten*, and the resulting
  file **does not load in ironcalc**.
- **LibreOffice** rewrites all 50 parts and adds 6 (renamed comments,
  `docProps/custom.xml`, new sheet rels). Its chart handling is **no better than
  openpyxl's on this axis**: it drops the same six chart-support parts
  (`chart1/2.xml.rels`, `colors1/2.xml`, `style1/2.xml`) — so "rewritten" in the
  table above understates it; the charts are degraded there too, just inside a
  package that still re-opens.

### `macro.xlsm` — VBA macros (11 parts)
Edit: `Data!B2` 100 → 200.

| Tool | identical | rw | drop | add | vba | ironcalc | soffice |
|------|-----------|----|------|-----|-----|----------|---------|
| **xlq** | **10 / 11** | 1 | 0 | 0 | ✅ `vbaProject.bin` byte-identical | ✅ | ✅ |
| openpyxl | 1 / 11 | 8 | 2 | 0 | ❌ **dropped** (saved as `.xlsx`) | ✅ | ✅ |
| LibreOffice | 0 / 11 | 11 | 0 | 1 | ✅ present (rewritten) | ✅ | ✅ |

- **xlq** rewrites only `xl/worksheets/sheet1.xml`; `xl/vbaProject.bin` is
  **byte-identical**. 10 / 10 non-edited parts preserved.
- **openpyxl** with its default `keep_vba=False` drops `xl/vbaProject.bin`
  (**all macros gone**) and `sharedStrings.xml`, and can only write `.xlsx` —
  the macro-enabled workbook is silently downgraded to a macro-free one.
- **LibreOffice** (VBA-preserving filter) keeps the macro but rewrites every
  part.

### `payroll.xlsx` — base: 3 sheets, cross-sheet formulas (13 parts)
Edit: `Rates!B2` 16 → 25 (a rate that feeds the `Payroll` sheet).

| Tool | identical | rw | drop | add | ironcalc | soffice |
|------|-----------|----|------|-----|----------|---------|
| **xlq** | **11 / 13** | 2 | 0 | 0 | ✅ | ✅ |
| openpyxl | 1 / 13 | 10 | 2 | 0 | ✅ | ✅ |
| LibreOffice | 0 / 13 | 12 | 1 | 1 | ✅ | ✅ |

- **xlq** rewrites exactly two worksheet parts — the edited `Rates` sheet **and**
  the `Payroll` sheet, whose formula cells recompute because they depend on the
  edited rate. Both rewrites are *necessary* (they carry changed cell values);
  nothing else moves.
- **openpyxl** drops `xl/sharedStrings.xml` and `xl/metadata.xml` and rewrites
  10 parts.

### `claims.xlsx` — base: 2 sheets, ~1.3k formulas (12 parts)
Edit: `Claims!D2` 13335 → 20000.

| Tool | identical | rw | drop | add | ironcalc | soffice |
|------|-----------|----|------|-----|----------|---------|
| **xlq** | **11 / 12** | 1 | 0 | 0 | ✅ | ✅ |
| openpyxl | 1 / 12 | 9 | 2 | 0 | ✅ | ✅ |
| LibreOffice | 0 / 12 | 11 | 1 | 1 | ✅ | ✅ |

- **xlq** rewrites only `xl/worksheets/sheet1.xml` (all downstream formulas are
  on the same sheet); the `Limits` sheet, styles, shared strings, and metadata
  are byte-identical.
- **openpyxl** again drops `sharedStrings.xml` + `metadata.xml`.

---

## The property, and what actually establishes it

> **For a single-cell edit, xlq's output differs from the input only in the
> worksheet part(s) whose cell values changed, plus the deliberately-dropped
> stale `calcChain.xml` cache; every other part is byte-identical.**

Two different kinds of support back this claim, and it is worth keeping them
apart rather than collapsing them into one word like "proof":

1. **Measured, on these fixtures.** `fidelity.py` compares every zip part
   byte-for-byte against the untouched original. On all four fixtures every
   non-edited part is byte-identical (48/50, 10/11, 11/13, 11/12). That is a
   demonstration on four hand-picked workbooks, not a corpus-wide proof, and it
   establishes only the *byte-identical-elsewhere* half. It does **not** verify
   that a rewritten sheet had to be rewritten: the benchmark's `rewritten` set
   is defined purely as "bytes differ" (`compare()` in `fidelity.py`), so it
   cannot by itself distinguish a necessary recompute (payroll's `Payroll`
   sheet, whose formulas depend on the edited rate) from a spurious
   reserialization.
2. **By construction.** `ooxml::surgical_write` streams every zip member of the
   input to the output unchanged and only re-serializes the sheet parts named
   in the edit set (dropping `calcChain.xml`, which a targeted edit makes
   stale). A part it does not touch is copied byte-for-byte, so charts, pivot
   caches, and `vbaProject.bin` cannot be altered by an edit that does not name
   their sheet. This is the architectural reason the measured numbers come out
   as they do — an asserted property of the writer, corroborated by (1) rather
   than independently proven by the benchmark, which never inspects the writer.

`xlq apply` re-derives the same tally in its own receipt
(`fidelity: { parts_total, parts_rewritten, parts_byte_identical }`). The
receipt agrees with the byte-level diff on the two load-bearing numbers —
`parts_total` and `parts_byte_identical` (50/48, 11/10, 13/11, 12/11) — with
one definitional wrinkle worth stating plainly: the receipt counts a **dropped**
part as "not byte-identical", so its `parts_rewritten` folds the dropped
`calcChain.xml` into the rewritten count. For `pivot-chart.xlsx` the receipt
therefore reads `parts_rewritten: 2` (one rewritten sheet + one dropped cache)
where the byte diff above reports `rw 1, drop 1`. The honest byte-identical
figure both agree on is 48/50. (Earlier builds also over-counted `parts_total`
by including zip *directory* entries; the receipt now counts on the same
files-only basis as the diff, which is why the totals match.)

Contrast: openpyxl and LibreOffice offer no such property. They deserialize the
model they understand and re-serialize the whole package, so any part they do
not model (chart theming, VBA, printer settings, shared-string layout) is
rewritten or dropped as a side effect — regardless of what the edit was.

---

## Methodology & caveats (stated straight)

- **Part diff is byte-level.** Two parts "match" only if their bytes are
  identical. This is deliberately strict: a semantically-equivalent
  re-serialization still counts as a rewrite, because a rewrite is exactly the
  risk we are measuring (lost round-trip fidelity, churned diffs, dropped
  extensions the writer did not model).
- **`calcChain.xml` is a cache, and dropping it is lossless.** It records
  formula evaluation order; Excel and LibreOffice regenerate it on open. xlq
  drops it precisely because a surgical edit makes the cached order stale. We
  still count it as "not byte-identical" in the tables above rather than hide
  it — the honest number for `pivot-chart.xlsx` is 48/50, not 49/50.
- **The LibreOffice column is a re-save proxy, not a targeted edit.**
  LibreOffice has no convenient CLI cell-editor, so we measure
  `soffice --convert-to` (open + write-back to the *same* format; the `.xlsm`
  fixture uses the VBA-preserving Calc filter). This over-states LibreOffice's
  churn relative to a true single-cell edit — its 100%-rewrite figure is an
  **upper bound on re-save churn**, not a like-for-like comparison. We include
  it because a re-save is what actually happens whenever a headless LibreOffice
  step touches a workbook in an agent pipeline.
- **`feature_survival` measures presence, not integrity.** A ✅ means a core
  part for the feature (`xl/charts/chartN.xml`, `xl/pivotTables/pivotTableN.xml`,
  `xl/vbaProject.bin`) is present in the output. It does **not** mean the
  feature is byte-identical — openpyxl's `pivot-chart.xlsx` charts are "present"
  yet stripped of their theming and relationship parts, and the file will not
  re-open in ironcalc. Only xlq's ✅s are also byte-identical.
- **No corpus totals.** Each fixture is reported on its own line. We do not sum
  "byte-identical parts across the corpus" into a single inflated headline —
  the interesting fact is per-workbook (charts survive here, VBA survives
  there), and averaging would hide exactly the failures this experiment exists
  to surface.

---

## Biggest surprise

openpyxl's "realistic agent path" does not merely *degrade* the pivot-chart
workbook — its output **fails to re-open in the ironcalc engine**
(`output_loads_in_ironcalc: false`). Two caveats keep this honest. First,
ironcalc is xlq's *own* vendored engine (see `meta.engine`), so this is xlq's
engine rejecting a competitor's output, not a neutral arbiter. Second, the one
neutral engine we measure — LibreOffice — **opens the same openpyxl file**
(`output_loads_in_soffice: true`). So the sharp claim is narrow and conditional:
openpyxl's output is no longer loadable by *this* engine, which bites when the
downstream automated step is ironcalc-based, but it is not a universal "the
file is broken."

We did not instrument *why* ironcalc rejects it — the benchmark records only the
load exit code, not a cause. The plausible explanation (dropped shared strings,
stripped chart relationship/theming parts, renamed comment parts) is a
hypothesis consistent with the part diff, not a measured attribution. What *is*
measured and unconditional is that openpyxl drops `sharedStrings.xml` on
**every** fixture — invisible in Excel, but it rewrites the entire
string-storage layout of the workbook on every save.
