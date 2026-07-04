# Literature Review — Surgical Structural Edits (Phase 1)

**Date:** 2026-07-04 · **Phase:** 1 · **Status:** completed
Sources: arXiv MCP (alphaXiv), full-text reads of the four load-bearing papers.

## The four papers that bound the novelty

### 1. Excelsior (Paine, Tek & Williamson, EuSpRIG 2008; arXiv 0803.0163) — CLOSEST on the OPERATION, opposite on FIDELITY
Excelsior restructures spreadsheets — resize tables, insert/delete rows and
columns, flip orientation, move tables across sheets — such that "each change
generated a spreadsheet with different structure but identical outputs." This
is exactly our structural-edit operation set, and it correctly shifts
dependent formulae. BUT it works by converting the original into a HIGH-LEVEL
MODEL ("object = tables + equations") via structure discovery, then
REGENERATING the spreadsheet from the model. It explicitly SACRIFICES FIDELITY:
"we did not try to reproduce properties such as cell formats and colours, input
menus, and charts"; "Properties such as cell styles were lost by the structure
discovery stage"; "S was read from a saved XML file into an Excelsior object,
losing the styles and other presentational information." **Excelsior IS the
load-into-a-model-and-regenerate approach whose fidelity loss our whole thesis
targets.** It does the semantic structural edit; it destroys everything else.
Our contribution is precisely the byte-fidelity-preserving version Excelsior
could not do. (Also: semi-automatic structure discovery, ~2 days of manual
work per sheet — not an automated safe primitive.)

### 2. Spreadsheet Refactoring (O'Beirne, EuSpRIG 2010; arXiv 1009.1412) — a human PRACTICES catalog
A catalog of refactoring patterns for worksheets/data/VBA (style conventions,
extract-expression, convert-relative-to-absolute, guard clauses). Human-guided
best practices, no automated fidelity-preserving structural-edit engine. Not
the same problem; cite for the "spreadsheets are code and deserve SE-grade
transformation" framing and the reference-integrity concerns it raises.

### 3. TACO — Efficient & Compact Spreadsheet Formula Graphs (Tang et al., 2023; arXiv 2302.05482) — the REFERENCE-SEMANTICS FOUNDATION to build on
Formalizes formula dependencies as a graph and, crucially, the exact
relative/absolute reference algebra we need: the fixed vs relative
relationship of a formula cell to the HEAD and TAIL of each referenced range,
yielding the four patterns RR / RF / FR / FF (relative-relative,
relative-fixed, etc.), keyed to the `$` absolute markers autofill produces. It
also handles INCREMENTAL MAINTENANCE under insert/clear/update. This is the
theoretical grounding for our reference-shift algebra — but TACO maintains an
IN-MEMORY dependency graph for fast recalculation, NOT the OOXML file. It never
edits the file, never shifts references in charts/pivots/named-ranges, never
touches fidelity. We CITE it as the reference-semantics foundation and extend
it from in-memory graph maintenance to on-disk minimal-patch OOXML surgery.

### 4. PPTArena / PPTPilot (Ofengenden et al., 2025; arXiv 2512.03042) — CLOSEST on FIDELITY-AWARE AGENTIC OOXML EDITING, but a sibling format + a benchmark
The strongest recent confirmation that our problem is real and general: a
benchmark for AGENTIC OOXML (PowerPoint) editing that explicitly targets
"non-destructive modification" and "modify it without collateral changes
elsewhere," notes "OOXML ... is highly intolerant to malformed VLM outputs,"
and shows generation-driven agents "diverge substantially from the original
structure, breaking fundamental preservation requirements." Their PPTPilot
routes between python-pptx and "deterministic OOXML patching." STRENGTHENS our
motivation (fidelity-loss is a recognized OOXML-agent problem beyond
spreadsheets). But it differs on every axis that matters: (a) PowerPoint, which
has NO formula-reference-shift problem — slides don't cross-reference cells, so
the hard part of structural spreadsheet edits (shifting all references
correctly) is absent; (b) preservation measured by VLM-as-judge + screenshots +
structural diffs, NOT byte-identity or a provable minimal-patch invariant; (c)
XML patches are VLM-generated, with no formal reference-shift algebra and no
proof-carrying verification; (d) it is a benchmark + agent, not a mechanism
with a guarantee.

## The precise gap (frozen claim wording)
"Semantic structural editing of spreadsheets exists but regenerates the file
from a model, sacrificing fidelity [Excelsior]; the relative/absolute
reference-shift algebra is formalized but only for in-memory dependency-graph
maintenance, not file surgery [TACO]; and fidelity-aware agentic OOXML editing
exists but for PowerPoint — which has no formula-reference-shift problem —
measured by VLM-judges rather than a provable invariant [PPTArena]. No prior
work performs SURGICAL, byte-fidelity-preserving STRUCTURAL edits
(insert/delete row/column) on OOXML SPREADSHEETS with a formal reference-shift
algebra applied uniformly across all reference-bearing parts (formulas,
cross-sheet, named ranges, charts, pivot caches) and a checked minimal-patch
invariant."

## Why the spreadsheet structural edit is uniquely hard (the novelty core)
A structural edit is where fidelity-preservation and correctness genuinely
CONFLICT, and only in spreadsheets:
- Inserting a row shifts references EVERYWHERE: in-sheet relative refs,
  absolute refs (pinned rows don't shift, pinned cols do or don't), range
  endpoints (grow vs shift), cross-sheet refs INTO the edited sheet, defined
  names, merged-cell/CF/DV ranges, chart `<c:f>` data refs, pivot-cache source
  ranges. PowerPoint has none of this.
- So byte-identity is IMPOSSIBLE for parts that reference the edited region — a
  chart on A1:A10 MUST become A1:A11. The right invariant is the REFINED one
  (010-setup): minimal reference-shift patch — only reference coordinates
  change, by exactly the shift delta; every non-reference byte identical.
- This unifies the three prior results: TACO's reference algebra (the shift
  function) + our v0.2 byte-fidelity surgery (the patch mechanism) = the thing
  Excelsior couldn't do (keep fidelity) and PPTArena doesn't need (formula
  shift), with a proof-carrying check PPTArena lacks.

## Baselines to beat (strength audit)
- Excelsior: `strong` on the operation, but fidelity-destroying by design — the
  contrast baseline (does the edit, loses charts/styles). Not runnable (no
  public tool); cite + reproduce the failure mode via openpyxl/LibreOffice
  round-trip, which also lose fidelity on structural edits.
- openpyxl `insert_rows`/`insert_cols`: `strong` (the status-quo agent path) —
  DOES it shift formula references? Known: openpyxl does NOT rewrite formula
  references on insert/delete (a documented sharp edge). So the status quo is
  doubly broken on structural edits: loses fidelity AND breaks references. This
  is the E1-analog headline to measure.
- LibreOffice: shifts references correctly (real engine) but rewrites 100% of
  parts (fidelity destroyed) — the correctness-vs-fidelity split, measured.

## Decision archaeology (for Phase 6 writing)
- Excelsior's load-bearing assumption: that a high-level model is the right
  representation. It buys clean structural edits at the cost of everything not
  in the model. Our reframe: stay at the OOXML level, shift references in
  place, never leave the file.
- PPTArena's move to transfer: "measure preservation as a first-class axis" +
  "deterministic XML patching beats generation." We adopt both, add byte-
  identity + reference-shift correctness + proof-carrying (their VLM-judge
  can't guarantee).
- TACO's move to transfer: the fixed/relative head/tail reference algebra — our
  shift function is a total map over exactly these cases + `#REF!` on anchor
  deletion.

## Next
Phase 2: formalize the reference-shift algebra + the minimal-patch invariant as
a falsifiable hypothesis with a theory-review gate. Phase 3 PoC: insert-row on
a workbook with a cross-sheet ref + a named range + a chart, verify references
shift correctly AND the minimal-patch invariant holds (non-reference bytes
identical). Then build, evaluate vs openpyxl/LibreOffice, and write.
