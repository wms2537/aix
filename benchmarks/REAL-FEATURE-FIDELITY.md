# Stratified real-corpus feature preservation

Date: 2026-08-23. This run extends the four hand-built T1 fixtures to a
deterministic sample from the already-acquired EUSES and Enron `converted_v2`
corpora. It answers a specific reviewer question: do the preservation results
survive outside fixtures authored by xlq/IronCalc?

## Sample

- Corpus: 5,447 converted `.xlsx` files (EUSES 4,648; Enron 799).
- Selection: seed `20260823`, up to 10 files per stratum.
- Strata: chart, pivot table/cache, external-link-only, comment/drawing-only,
  and plain control.
- Edit target: the first non-formula numeric data cell in workbook order. No
  cell content is printed or committed in the manifest.
- Manifest: [`real_feature_manifest.summary.json`](real_feature_manifest.summary.json)
  and the full 5,447-row JSONL manifest.

Corpus-wide structural inventory:

| Feature | Files |
|---|---:|
| Chart | 451 |
| Pivot table/cache | 35 |
| External link | 281 |
| Comment | 364 |
| Drawing | 1,633 |

No sampled workbook had VBA; this remains an open gap for `.xlsm` acquisition.

## Method

Each selected workbook receives one identical logical operation—assign its
selected numeric target back to its current value—through three paths:

1. **xlq apply**: typed patch, proof-carrying commit, fidelity receipt.
2. **openpyxl**: normal load/assign/save with default `keep_vba=False`.
3. **LibreOffice headless re-save**: same-format convert proxy. This is not a
   targeted cell edit and is therefore an upper bound on churn.

The metric is byte-identical zip members versus the untouched original. A part
that is semantically equivalent but rewritten still counts as rewritten.

## Results

The denominator is parts from files where that tool completed. Failures are
not discarded: they remain in
[`real_feature_fidelity.json`](real_feature_fidelity.json) and are classified
in [`real_feature_fidelity.summary.json`](real_feature_fidelity.summary.json).

| Stratum | xlq | openpyxl | LibreOffice proxy |
|---|---:|---:|---:|
| Chart | **212 / 230 = 92.2%** | 8 / 302 = 2.6% | 128 / 302 = 42.4% |
| Pivot | **174 / 186 = 93.5%** | 11 / 464 = 2.4% | 175 / 464 = 37.7% |
| External link only | **47 / 51 = 92.2%** | 10 / 221 = 4.5% | 73 / 221 = 33.0% |
| Comment/drawing only | **137 / 149 = 91.9%** | 8 / 149 = 5.4% | 51 / 117 = 43.6% |
| Plain control | **86 / 98 = 87.8%** | 8 / 110 = 7.3% | 39 / 110 = 35.5% |
| **Aggregate completed** | **656 / 714 = 91.9%** | 45 / 1,246 = 3.6% | 466 / 1,214 = 38.4% |

On the first real chart file, for example, xlq preserved 16 of 18 parts,
rewriting only the edited worksheet and a dependent worksheet; openpyxl
preserved 1 of 18 and dropped `xl/sharedStrings.xml`; LibreOffice preserved 11
of 18 while rewriting the chart XML.

## Failure policy

The 50-file sample intentionally retained hard cases:

- Seven xlq attempts refused error-valued targets.
- Three refused writes whose affected cone contained UDFs or volatile
  functions.
- One refused unsupported rich styling rather than risk reserialization.
- openpyxl failed outright on two files before comparison.
- LibreOffice had one local runner collision and is counted as incomplete for
  that file.

These are fail-closed outcomes, not silent corruption. The benchmark does not
credit a refusal as preservation; it records refusal separately so soundness
and coverage can be read independently.

## Honest limits

- The corpora were `.xls -> .xlsx` converted with LibreOffice, so native Excel
  serialization is still not represented.
- No sampled file had VBA; a dedicated macro-enabled real-corpus lane remains
  necessary.
- Excel itself was unavailable; preservation here is byte/provenance evidence,
  not live-Excel semantic arbitration.
- The aggregate mixes different completion sets across tools. Use the per-file
  artifact when comparing individual workbooks.
