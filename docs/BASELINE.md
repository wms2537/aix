# BASELINE: Existing Tools for Programmatic / Agent Operations on .xlsx

Date: 2026-07-02. Star counts and release data pulled from the GitHub API on this date.
Method: primary docs and repos fetched directly; claims verified against source where possible;
two behaviors verified empirically with local probes (marked "verified locally").
This is a capability baseline, not a review. Where a tool is good, that is stated; where a
claim is inference rather than documentation, it is marked.

---

## 1. openpyxl (Python)

The default read/write library for xlsx in the Python ecosystem, and the engine underneath
most "AI edits your spreadsheet" tooling (including Anthropic's own xlsx skill and the most
popular Excel MCP server, see §5).

- **No formula evaluation, by design.** The docs are explicit: "openpyxl **never** evaluates
  formula" ([simple_formulae docs](https://openpyxl.readthedocs.io/en/stable/simple_formulae.html)).
  `load_workbook(data_only=True)` returns only the value cached by Excel the last time the file
  was opened in Excel ([tutorial](https://openpyxl.readthedocs.io/en/stable/tutorial.html)); if the
  file was written programmatically and never opened in Excel, cached values are `None`
  (verified locally with openpyxl 3.1.5: `data_only=True` on a freshly written file returns
  `None` for a `=SUM(...)` cell).
- **Lossy round-trip on real-world files.** The docs warn: "openpyxl does currently not read all
  possible items in an Excel file so shapes will be lost from existing files if they are opened and
  saved with the same name" ([tutorial](https://openpyxl.readthedocs.io/en/stable/tutorial.html)).
  Pivot tables have read-support only — "it is not intended that client code should be able to
  create pivot tables" ([pivot docs](https://openpyxl.readthedocs.io/en/stable/pivot.html)).
  VBA survives only if you pass `keep_vba=True`, and even then it is "still not editable"
  ([tutorial](https://openpyxl.readthedocs.io/en/stable/tutorial.html)).
- **Documented real-world corruption in agent use.**
  [anthropics/claude-code#22044](https://github.com/anthropics/claude-code/issues/22044) reports
  Claude Code's xlsx skill (openpyxl-based) corrupting ~50 investment models: .xlsm files
  unopenable, named ranges stripped, conditional formatting lost, Excel showing the
  "We found a problem with some content" recovery dialog. The user recovered via Dropbox
  version history. The issue asked for warnings, non-destructive defaults, and refusal to write
  complex workbooks; it was **closed as not planned**.
- Nuance (verified locally, openpyxl 3.1.5): a *simple chart that openpyxl itself created* does
  survive a load/save round-trip (`xl/charts/chart1.xml` present after re-save). The practical
  failure mode is Excel-authored files: openpyxl re-serializes everything through its own object
  model, so any feature it does not model (shapes, chart styles, slicer/pivot details, extension
  parts) is silently dropped or mangled.
- No dry-run, no change log, no backup, no versioning. `wb.save()` "will overwrite existing
  files without warning" ([tutorial](https://openpyxl.readthedocs.io/en/stable/tutorial.html)).
- License: MIT.

## 2. xlwings (Python)

Remote-controls a real Excel instance, so calculation and fidelity are Excel's own.

- "xlwings (Open Source) requires an installation of Excel and therefore only works on Windows
  and macOS" ([installation docs](https://docs.xlwings.org/en/stable/installation.html)). A file
  reader added in v0.28 works on Linux without Excel, but it is read-only.
- Because Excel itself performs the write, VBA, pivots, charts, and everything else are preserved
  and formulas genuinely recalculate. The costs: an interactive Excel process (slow, stateful,
  licensing), no headless/CI story on Linux, and none of Excel's UI safety (undo) is exposed
  programmatically as a transaction model.
- No dry-run, receipts, or versioning.
- License: BSD 3-clause core; the `xlwings.pro` subpackage is commercial
  ([LICENSE.txt](https://github.com/xlwings/xlwings/blob/main/LICENSE.txt)). ~3.4k stars.

## 3. Excel via COM (pywin32 / VBA automation)

The maximum-fidelity option: full recalculation, full feature preservation, because it *is* Excel.

- Windows-only, requires a licensed Excel installation, single-threaded STA automation.
- Microsoft explicitly does not support it for server/agent use: "Microsoft does not currently
  recommend, and does not support, Automation of Microsoft Office applications from any
  unattended, non-interactive client application or component" — modal dialogs on a
  non-interactive desktop hang the thread indefinitely
  ([Considerations for server-side Automation of Office](https://support.microsoft.com/en-us/visio/considerations-for-server-side-automation-of-office);
  see also the [unattended RPA variant](https://learn.microsoft.com/en-us/office/client-developer/integration/considerations-unattended-automation-office-microsoft-365-for-unattended-rpa)).
- The cloud alternative (Microsoft Graph workbook API) avoids local Office but requires files in
  M365/SharePoint and a network round-trip per operation.
- No dry-run, receipts, or tool-level versioning (OneDrive/SharePoint provide storage-layer
  version history, external to the tool).

## 4. LibreOffice headless (Calc)

Free, scriptable (`soffice --headless --convert-to`, UNO API), runs on Linux servers.

- **Real calc engine, but not Excel's.** Function coverage lags Excel by years: XLOOKUP, XMATCH,
  FILTER, LET, SORT, UNIQUE, etc. only landed in LibreOffice 24.8, August 2024
  ([release coverage](https://9to5linux.com/libreoffice-24-8-open-source-office-suite-officially-released-heres-whats-new)) —
  roughly five years after Excel. Semantics differ at the edges (text/number coercion, regex vs
  wildcards, date handling).
- **Stale-value trap:** for Excel 2007+ files the default "Recalculation on File Load" setting is
  "Never recalculate" ([Formula options help](https://help.libreoffice.org/7.0/en-US/text/shared/optionen/01060900.html)),
  so a headless conversion can silently emit the *cached* values from the last Excel session
  rather than recomputing.
- **Speed / server use:** each `--convert-to` invocation boots the whole suite. The
  [unoserver](https://github.com/unoconv/unoserver) project exists specifically to keep a
  listener process alive, claiming 50–75% CPU reduction (2–4x throughput) versus cold starts.
  Concurrency requires managing separate user-profile instances.
- Fidelity on round-trip is better than openpyxl (it models charts, pivots, and most OOXML), but
  files are re-serialized through Calc's model, so Excel-specific details can shift. VBA is
  preserved on re-save via the "Save original Basic code" option
  ([VBA properties help](https://help.libreoffice.org/latest/en-US/text/shared/optionen/01130100.html)).
- No dry-run, receipts, or versioning. License: MPL-2.0.

## 5. haris-musa/excel-mcp-server

The most popular Excel MCP server (~3,978 stars, MIT, last push 2026-04).

- Tools: workbook/worksheet CRUD, read/write ranges, formatting, `apply_formula`,
  `validate_formula_syntax`, chart creation, pivot creation, tables, row/column ops
  ([TOOLS.md](https://github.com/haris-musa/excel-mcp-server/blob/main/TOOLS.md)).
- **It is openpyxl underneath** (`openpyxl>=3.1.5` in
  [pyproject.toml](https://github.com/haris-musa/excel-mcp-server/blob/main/pyproject.toml)), so
  it inherits every limitation in §1: no evaluation, lossy round-trip on complex files.
- Worse: its workbook module calls `load_workbook(filepath)` **without `keep_vba`**
  ([src/excel_mcp/workbook.py](https://github.com/haris-musa/excel-mcp-server/blob/main/src/excel_mcp/workbook.py)),
  so editing an .xlsm through it silently drops the VBA project.
- Safety model: none. No dry-run, backup, undo, audit, or versioning appears anywhere in
  TOOLS.md (verified by grep). Writes go directly to the target file.

## 6. iOfficeAI/OfficeCLI

The strongest new entrant (~8,299 stars, Apache-2.0, C#, actively pushed as of 2026-07-02).

- Positioning: "purpose-built for AI agents... Free, open-source, single binary, no Office
  installation required" ([repo](https://github.com/iOfficeAI/OfficeCLI)). Ships as a single
  self-contained binary with the .NET runtime embedded.
- **Evaluates formulas**: "350+ built-in Excel functions evaluated automatically on write,"
  including spilling dynamic arrays (FILTER/SORT/UNIQUE/LET/LAMBDA), financial/bond math, and
  statistical functions — no Office round-trip ([README](https://github.com/iOfficeAI/OfficeCLI#readme)).
- Creates native OOXML pivot tables (cache + definition written so Excel opens pre-aggregated),
  charts (incl. box-whisker, Pareto), conditional formatting, slicers, sparklines, images, OLE.
  Formula references are rewritten on row/column insert. Built-in HTML/PNG rendering gives
  agents visual feedback in headless environments.
- Safety model: thin. `batch` with `--stop-on-error`, and `dump` (replay a file as batch JSON).
  **No dry-run, no backups, no change receipts, no audit log, no versioning** are documented.
  VBA preservation is not documented either.
- Commands: `create`, `add`, `set`, `remove`, `get`, `query`, `view`, `batch`.

## 7. Headless calculation engines

### IronCalc
- Rust engine with xlsx reader/writer in the main repo; Python/JS(wasm)/node bindings
  ([repo](https://github.com/ironcalc/IronCalc), ~3,976 stars, MIT OR Apache-2.0).
- Self-described "work-in-progress"; docs say "if a formula is evaluated differently in Excel
  than in IronCalc it is most likely a bug" but also that "you might find some important features
  missing" ([docs.ironcalc.com](https://docs.ironcalc.com/)). ~200 functions at the Nov 2024 MVP
  ([Road to 1.0](https://blog.ironcalc.com/2024/11/06/IronCalc-1.0.html)), "300+" claimed now
  ([ironcalc.com](https://www.ironcalc.com/)). Still pre-1.0: latest release v0.7.1, Jan 2026
  (GitHub releases API).
- Writes from its own internal model; features outside that model (VBA, most chart/pivot
  richness) do not round-trip (inference from architecture; the write path is model → xlsx).

### HyperFormula (Handsontable)
- Headless JS/TS calc engine, **418 built-in functions**
  ([built-in functions](https://hyperformula.handsontable.com/guide/built-in-functions.html));
  ~2.7k stars. In-memory CRUD, undo-redo, clipboard.
- **No file I/O at all**: xlsx must be parsed to JS arrays with a third-party library (ExcelJS
  et al.) before loading ([file import guide](https://hyperformula.handsontable.com/guide/file-import.html)).
  It is an engine component, not an xlsx tool; write-fidelity is whatever your exporter does.
- Dual-licensed **GPLv3 or commercial** ([LICENSE.txt](https://github.com/handsontable/hyperformula/blob/master/LICENSE.txt)) —
  the GPL side is viral for embedding.

### GRID engine
- Proprietary TypeScript engine from GRID (grid.is), now sold as an npm SDK
  (`@grid-is/spreadsheet-engine`) explicitly positioned as "The Spreadsheet Engine for Agentic
  Products," with a published skill for Claude Code/Cursor ([grid.is/engine](https://grid.is/engine)).
- Deep Excel/Sheets compatibility ("existing spreadsheets run as-is"), ~200k automated unit
  tests per build; 398 functions as of their 2021 engineering write-up
  ([blog](https://medium.grid.is/we-built-a-spreadsheet-engine-from-scratch-heres-what-we-learned-e4800ab9edf1) — count is likely higher today).
- Free evaluation license; commercial for production. Closed source. Calculation-focused; not a
  fidelity-preserving file editor.

## 8. formulas (Python)

- Interprets/compiles Excel formulas in pure Python and can execute whole workbooks as a
  dependency graph "without using the Excel COM server. Hence, **Excel is not needed**"
  ([README](https://github.com/vinci1it2000/formulas)). CLI accepts .xlsx/.ods/.json, supports
  input overrides. v1.3.4, March 2026; ~490 stars.
- Partial function coverage (a subset of Excel's ~500+); performance degrades on large models;
  it is a calculator over a workbook, not a fidelity-preserving editor.
- License: **EUPL 1.1+** — unusual, copyleft-ish, a due-diligence flag for commercial embedding.

## 9. Adjacent: Rust xlsx crates (context for xlq)

- [calamine](https://github.com/tafia/calamine) (~2.3k stars, MIT): fast pure-Rust **reader** only.
- [rust_xlsxwriter](https://github.com/jmcnamara/rust_xlsxwriter) (~570 stars, Apache-2.0):
  **writer** only, no read/modify.
- [umya-spreadsheet](https://github.com/MathNya/umya-spreadsheet) (~461 stars, MIT): read/write,
  but re-serializes through its own model with the same class of fidelity risk as openpyxl.
- None evaluate formulas; none have a safety model.

---

## Capability matrix

"Preserves on write" = VBA + pivots + charts survive an edit-and-save of an Excel-authored file.
"Dry-run" = can show what a write would change without touching the file.
"Receipts" = machine-readable record of what an operation actually changed.
"Revisioning" = built-in version history of the file with rollback.

| Tool | Evaluates formulas | Preserves VBA/pivots/charts on write | Dry-run before write | Change receipts / audit | Revision-versioning | Works without Office | Single binary | License |
|---|---|---|---|---|---|---|---|---|
| openpyxl | No (never; cached values only) | Partial: pivots read-only, VBA opt-in & frozen, shapes lost, complex files corrupt in practice | No | No | No | Yes | No (Python pkg) | MIT |
| xlwings | Via Excel (real recalc) | Yes (Excel does the write) | No | No | No | **No** (needs Excel; Win/macOS) | No | BSD-3 + commercial PRO |
| Excel COM (pywin32) | Yes (Excel) | Yes | No | No | No (storage-layer only) | **No** (Windows + licensed Excel; unsupported unattended) | No | Excel EULA / PSF |
| LibreOffice headless | Yes (Calc engine; xlsx **not recalculated by default on load**) | Mostly; VBA via save-original option; fidelity drift on complex files | No | No | No | Yes | No (full suite, ~100s MB) | MPL-2.0 |
| excel-mcp-server | No (openpyxl inside) | No — drops VBA (no `keep_vba`), inherits openpyxl losses | No | No | No | Yes | No (Python + MCP host) | MIT |
| OfficeCLI | Yes (350+ fns, own engine) | Charts/pivots it creates: yes; VBA/edit-preservation: undocumented | No | No | No | Yes | **Yes** | Apache-2.0 |
| IronCalc | Yes (300+ fns, pre-1.0) | No (writes from own model) | No | No | No | Yes | Lib/crate | MIT / Apache-2.0 |
| HyperFormula | Yes (418 fns) | N/A — no file I/O | No | No | No (in-memory undo only) | Yes | No (JS lib) | GPLv3 or commercial |
| GRID engine | Yes (~400 fns) | Unknown; calc-focused | No | No | No | Yes | No (npm lib) | Proprietary |
| formulas | Yes (subset) | No | N/A (compute-oriented) | No | No | Yes | No (Python pkg) | EUPL 1.1+ |

---

## Gaps

Three columns of the matrix are empty for every tool surveyed:

1. **Dry-run before write.** No tool — not openpyxl, not the 8k-star OfficeCLI, not any MCP
   server, not Excel itself via COM — can answer "what would this edit change?" before mutating
   the file. Every write is a leap.
2. **Change receipts / audit.** No tool emits a machine-readable record of what an operation
   actually did (cells touched, formulas rewritten, parts dropped). When openpyxl silently
   strips a slicer or excel-mcp-server drops a VBA project, nothing records that it happened.
   The failure is discovered later, by a human, in Excel's recovery dialog.
3. **Revision-versioning.** No tool keeps file history or offers rollback. The only recovery
   path in the documented corruption case
   ([claude-code#22044](https://github.com/anthropics/claude-code/issues/22044)) was Dropbox's
   storage-layer version history — external, coarse, and after the fact. Storage sync (OneDrive,
   SharePoint, Dropbox) is the de facto versioning layer, and it knows nothing about workbook
   semantics.

The ecosystem has solved, separately: reading xlsx (calamine, openpyxl), writing xlsx
(rust_xlsxwriter, OfficeCLI), and evaluating formulas headlessly (HyperFormula, IronCalc, GRID,
OfficeCLI). It has not solved *safe mutation*: no tool wraps agent writes in
preview → apply → receipt → revision. That absent layer — a transactional safety model for
workbook edits, independent of which calc engine or writer is used — is the gap xlq targets.
