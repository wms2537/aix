# Changelog

All notable changes to `xlq` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0]

### Added
- New read/recovery verbs over the transactional journal:
  - `xlq log <file>` — receipt history with per-entry chain-linkage.
  - `xlq verify <file>` — recompute the file hash vs the latest receipt and walk
    the whole hash chain; detects out-of-band tampering (exit 1 on failure).
  - `xlq undo <file>` — transactionally restore the previous committed snapshot,
    failing closed on a missing/corrupt backup, no prior snapshot, or an
    out-of-band edit.
- `xlq apply --schema` prints the patch format's JSON Schema (no file needed).
- Decompression caps against zip bombs (512 MiB/part, 2 GiB/workbook by default),
  overridable via `XLQ_MAX_PART_BYTES` / `XLQ_MAX_TOTAL_BYTES`, enforced on every
  xlq-controlled read plus a preflight before the engine loads an untrusted file.

### Changed
- **Security:** upgraded `quick-xml` 0.36 → 0.41, clearing two HIGH RUSTSEC
  advisories (RUSTSEC-2026-0194, -0195). Formula bodies are now reassembled across
  the `Text`/`GeneralRef` events quick-xml ≥0.38 emits, so entity-bearing formulas
  (`A5&gt;0`) shift correctly instead of being silently corrupted.
- Uniform exit-code contract: `0` effect/answer, `1` refusal/failure, `2` bad
  invocation, `70` internal error. `certify` REFUSED now exits `1` (was `0`).
- Engine-provenance string is single-sourced from the vendored engine
  (`ironcalc_base::ENGINE_PROVENANCE`) so it can never drift from the linked build.
- The receipt journal recovers a single crash-torn trailing line and fails loudly
  on interior corruption (previously a torn tail wedged every future write).

### Reference-completeness (fail-closed hardening)
Adversarial review found the structural edit and `certify` under-covered several
reference-bearing constructs — leaving a stale reference or falsely certifying a
foreign edit. The residual scan and `certify`'s compare surface were reworked to be
**fail-closed by default**: a reference the engine cannot prove it shifts faithfully
is refused, not committed.
- **Foreign cross-references now use the shift algebra as the oracle, not a substring
  scan.** The old gate matched the edited sheet's name by raw, case-sensitive,
  still-escaped substring, so a foreign sheet's formula referencing the edited sheet
  via an entity-encoded qualifier (`Data&#33;A5`), a case variant (`SHEET1!`), or a
  3D span whose first endpoint is the edited sheet (`Sheet1:Sheet3!`) was left
  **silently unshifted**. Both the shift gate and the cross-reference residual guard
  now delegate to σ (`shift_formula`), which is case-insensitive, quote/apostrophe- and
  entity-aware, and 3D-span-correct.
- A foreign **array formula** (`<f t="array">`) referencing the edited sheet — which
  the shift path cannot rewrite — is now refused instead of committed stale.
- The edited sheet's body is scanned fail-closed for coordinate-bearing constructs the
  engine copies verbatim: `<protectedRange sqref>` (a security reference),
  `<scenario><inputCells r>`, `<dataConsolidate><dataRef ref>`, `<ignoredError sqref>`,
  and `<sortState ref>` now refuse rather than leave a stale coordinate.
- A foreign **shared-formula dependent** can cross the edit boundary even when its
  master does not; the foreign-sheet gate now runs over shared-*expanded* formulas so
  every dependent's real reference is shifted.
- **`<col>` column definitions are now shifted on column edits** (they were copied
  verbatim, leaving the wrong column hidden/styled), clamped to the last column
  (XFD/16384), with an emptied `<cols>` container omitted rather than left
  schema-invalid.
- **Pivot caches fail closed:** a pivot source other than a `<worksheetSource>` (e.g. a
  consolidation `<rangeSet ref sheet>`) that names the edited sheet is refused rather
  than committed with a stale grid range.
- **move-rows no longer silently enlarges a straddling range.** A range whose endpoints
  stayed ordered under the move permutation but whose *size* changed (a non-inverting
  straddle, e.g. `SUM(A4:A6)` → `SUM(A4:A18)`) was committed with wrong recomputed values;
  the move now requires the span to move rigidly or it fails closed (`#REF!` → residual).
- **Worksheet-scoped defined names are shifted in their own scope.** A `localSheetId`
  name with an unqualified refers-to (`$A$8` scoped to the edited sheet) was left stale
  (the shift used an empty host for every name); it now resolves against its scoped sheet.
- **Whitespace around a range colon is handled.** `SUM(A2 : A8)` (which Excel/IronCalc
  read as the range `A2:A8`) tokenized as two independent single cells and bypassed the
  range straddle/clamp logic, silently corrupting the value; it now shifts as a range.
- **Insert clamps to the grid.** A single-cell reference to the last row/column
  (`A1048576`, `XFD1`) shifted past the grid to an out-of-range reference that recomputed
  to an error; an overflow is now `#REF!`, mirroring delete. A full-height *range*
  (`SUM(A2:A1048576)`) instead clamps its tail to the last line — Excel keeps it valid.
- **delete-cols now removes the deleted columns' content.** It shifted cell coordinates
  but never dropped the cells inside the deleted band, so a rightmost delete retained
  stale data and an interior delete emitted duplicate coordinates (invalid OOXML); the
  deleted cells are now dropped, mirroring delete-rows.
- **Non-ASCII sheet names are handled fail-closed.** The reference tokenizer is ASCII-only,
  so an unquoted non-ASCII sheet qualifier (`集計!A11`) referencing the edited sheet was
  left stale; such a cross-reference is now refused rather than committed wrong.
- **Internal hyperlinks are no longer over-refused.** A link whose in-workbook target is
  above/left of the edit (unaffected) previously refused the whole edit; it now refuses
  only when the target would actually move.
- **`certify`'s part check is now a fail-closed allowlist** (was an enumerated
  denylist). Any part outside the known-safe/compared set — worksheets, workbook,
  styles, theme, sharedStrings, calcChain, metadata, media, printer settings, docProps,
  vbaProject, packaging — is refused, closing the long tail (tables, drawings, comments,
  form controls, volatile dependencies, query tables, slicer/timeline caches,
  connections, customXml) in one rule instead of chasing each construct.
- **`certify` now compares more of each reference's semantics:** a hyperlink's
  destination (internal `location` and external `r:id`→relationship Target, resolved
  namespace-prefix-insensitively — catching an in-workbook mispoint or a phishing-URL
  swap), the owning **sheet** of every mergeCell/hyperlink/autoFilter (catching a
  cross-sheet relocation), a defined name's **scope** (`localSheetId`, catching a
  re-scope), and the workbook's **sheet order and `<calcPr>`** (both value-affecting). A
  foreign cross-sheet ref/sqref attribute (e.g. a consolidation `<dataRef ref="Sheet!…">`)
  is likewise refused by the restructure scan.
- **Namespace-blind parsers fixed.** `certify`'s defined-name comparison and the
  defined-name/cell collision check used a `<definedName` substring that missed a
  namespace-prefixed `<x:definedName>` the shifter *does* rewrite (a false
  certification and a missed collision). Both now use one namespace-aware,
  entity-resolving parser. `certify`'s sheet-construct scan (x14 conditional
  formatting, data validation, sparklines), OPC part-name resolution, and worksheet
  enumeration are likewise matched by local name / case-insensitively so a rebound
  prefix or re-cased part cannot evade them.

- **Absolute/mixed whole-row references shift.** `$5:$10` (and `5:$10`, `$5:$5`) were
  left stale by a row edit — `parse_endpoint` mis-read the row's `$` as the column's — so
  a stored reference silently pointed at the wrong rows; fixed.
- **sortState / sortCondition ranges shift** instead of being refused, so a common
  openpyxl autoFilter-with-sort is no longer over-refused.
- **A fully-consumed range is dropped, not emptied.** A delete that consumes an entire
  mergeCell/dataValidation/conditionalFormatting range emitted a malformed `ref=""`
  (Excel repair); the element is now dropped.
- **certify catches a dropped `_xlfn.` prefix.** Post-2007 functions (CONCAT, XLOOKUP, …)
  require the `_xlfn.` prefix in stored XML, which the engine strips on load; a foreign
  edit dropping it (→ Excel `#NAME?`) was invisible to the normalized cell diff and is now
  caught by comparing the stored prefixed function tokens.
- **certify compares drawing shape/image hyperlink targets.** A phishing retarget of an
  `<a:hlinkClick>` on a shape (resolved through the drawing's own rels) was outside the
  compared surface; now compared alongside worksheet hyperlinks.
- **Fail-closed scans are AFFECT-based, not presence-based.** The edited-sheet body scan
  and the x14 `<extLst>` scan refused an edit whenever an unshiftable coordinate construct
  was merely PRESENT — but Excel writes a data bar / color scale / icon set / sparkline as
  an x14 extLst on nearly every real workbook, so this refused almost every legitimate
  edit. They now refuse only when the edit would actually MOVE the construct's range.
- **A 3D span with quoted special-character sheet names is recognized.** The interior-tab
  3D-span guard's backward walk stopped at a special char inside a quoted endpoint name
  (`'A-Sheet:B-Sheet'!`), so an interior edit committed a stale 3D reference; the walk now
  handles quoted names.
- **Form-control / OLE data bindings are checked.** A control's `linkedCell`/`fmlaLink`/
  `listFillRange` (the cell it reads/writes) was left stale when the control's relationship
  was dangling; these are now shifted-or-refused.
- **The VBA macro binary is compared byte-for-byte.** `xl/vbaProject.bin` was allowlisted
  as safe but never diffed, so a foreign edit that injected or swapped the auto-executing
  macro (arbitrary code) was certified — a security laundering. Its bytes and presence are
  now compared; a structural edit never alters executable code.
- **Sheet/workbook protection is compared.** Stripping or weakening a password-backed
  `<sheetProtection>`/`<workbookProtection>`/`<protectedRange>` (a security control the
  transform preserves) was certified; its attributes are now compared.
- **Charts and drawings are COMPARED, not refused on presence.** certify refused *any*
  workbook containing a chart or image — including xlq's own transform — while restructure
  accepts and shifts chart data references. Chart `<f>` data ranges and drawing cell
  anchors are now compared semantically instead.
- **move-rows no longer over-refuses an invariant range.** A range that fully contains the
  moved block (so the move only permutes rows within it and the cell set is unchanged) was
  refused as a straddle; it is now recognized as value-preserving and left unchanged.
- **Conditional formatting and data validation are COMPARED, not refused on presence.**
  The earlier fail-closed pass refused certification of any workbook carrying a CF rule or
  a data-validation dropdown — including xlq's own faithful transform — which made certify
  unusable on ordinary modern files. Their `sqref`+formula references (and x14 `<extLst>`
  references) are now compared against xlq's transform: a faithful edit matches, a mangle
  differs.
- **certify guards fabricated formula caches.** A `cached_value` difference is treated as
  benign (Excel recomputes on load) unless the foreign file EXPLICITLY disables
  recalc-on-load (`<calcPr fullCalcOnLoad="0">`), in which case a fabricated cache would be
  shown verbatim and the difference is disqualifying.
- **certify compares the date system and narrows the calc-settings compare.** A foreign
  `workbookPr@date1904` flip (which shifts every date value by 1462 days, invisible to a
  serial-vs-serial cell diff) is now caught; and the calc-settings compare was narrowed to
  the value-affecting attributes (`calcMode`, `iterate`, and — when iterate is on —
  `iterateCount`/`iterateDelta`, which set a circular reference's converged value) so a
  benign `calcId` build-stamp or `fullCalcOnLoad` no longer spuriously refuses a faithful
  edit.
- **certify compares autoFilter criteria.** A foreign edit that preserved an autoFilter's
  `ref` but altered a filter *criterion* (`<customFilter val>`, `<filter>`, `<top10>`,
  `<dynamicFilter>`, `<dateGroupItem>`, `<colorFilter>`, `<iconFilter>`) changed which rows
  the sheet shows without touching any cell — invisible to the cell diff; the criteria are
  now compared per sheet.
- **certify compares `<workbookPr fullPrecision>`.** Turning off precision-as-displayed
  (`fullPrecision="0"`) silently rounds every formula input to its displayed digits,
  changing recomputed values with no cell-level edit; the flag is now part of the compared
  workbook settings.
- **`fabricated-cache` guard also covers `calcMode="manual"`.** A foreign workbook set to
  manual calculation likewise shows stored caches verbatim (Excel does not recompute on
  load), so a differing cache under manual calc is now disqualifying, matching the
  `fullCalcOnLoad="0"` gate.
- **Web-publish source ranges shift-or-refuse.** A `<webPublishItem sourceRef>` (the cell
  range a sheet publishes to HTML) is a coordinate the engine copies verbatim; it is now
  refused when the edit would move it, closing another verbatim-copy gap in the edited-sheet
  body scan.
- **`certify`'s attribute reader honors XML `Eq` whitespace and whole-name boundaries.** The
  helper read an attribute by a literal `key=` substring, so XML-legal whitespace around the
  equals sign (`date1904 = "1"`, which Excel honors) read as the attribute's default —
  letting a foreign edit smuggle a value-affecting workbook setting (`date1904`,
  `fullPrecision`, `calcMode`) past the settings compare and CERTIFY. It now parses
  `Eq ::= S? '=' S?` and matches `key` only as a whole attribute name (so a suffix collision
  like `id` inside `guid=` cannot forge a value either). The same helper backs the
  recalc-on-load and structural-ref-attribute checks.
- **`certify` compares the implicit-intersection `@` operator.** `@A1:A10` coerces a range
  to the single intersecting cell (a scalar) while the bare `A1:A10` SPILLS the whole range
  — a different value *and* footprint. The engine normalizes `@` away on load, so the
  loaded-model cell diff cannot see a foreign edit that drops or adds it; the operator count
  is now compared per sheet, mirroring the `_xlfn.`-prefix guard.
- **`certify` verifies fabricated formula caches, not just explicitly-disabled recalc.** A
  `cached_value` difference was treated as benign unless the workbook *explicitly* set
  `<calcPr fullCalcOnLoad="0">` — but per ECMA-376 that flag defaults to `false`, so its
  mere ABSENCE (the common case) is equally unsafe: Excel then displays the stored cache
  verbatim. A foreign file could fill a shifted cell's blanked cache with a wrong value and
  drop the `fullCalcOnLoad="1"` xlq wrote, and certify would CERTIFY it. certify now compares
  the STORED formula caches directly: a formula cell in the foreign file that carries a
  PRESENT `<v>` xlq's transform did not vouch (absent in, or differing from, the transform)
  is disqualifying — UNLESS the foreign file forces a full recalc-on-load
  (`fullCalcOnLoad="1"`), in which case Excel recomputes it away. A cache-DROPPING edit
  (openpyxl leaves no `<v>`; xlq blanks every shifted cell) verifies cleanly, so the benign
  case is not over-refused. The result adds an `unverified_caches` count to the JSON.
- **`certify` compares MANUALLY hidden rows under `SUBTOTAL(101–111)`/`AGGREGATE`.** Those
  aggregates EXCLUDE manually hidden rows from their result, so a foreign edit that hides a
  data row inside the range changes the computed value with no formula or cached-value diff
  the cell diff could see. certify now compares the hidden-row set — but only on sheets that
  actually carry such a function (a `SUBTOTAL` code ≥ 101 or an `AGGREGATE` option in
  {1,3,5,7}); on any other sheet a hidden row is pure display state and is not compared, so
  ordinary hide/unhide is not over-refused.
- **`certify` catches a dropped `fullCalcOnLoad` that unmasks a stale cache.** When xlq's
  transform itself forces recalc-on-load (`fullCalcOnLoad="1"`), it displays RECOMPUTED
  values and its own stored caches are moot. A foreign edit that keeps the (now stale) cache
  but DROPS the recalc-forcing flag would show the stale value while the transform shows the
  recomputed one — and the stale caches, being identical on both sides, slipped the round-15
  cache compare. The cache check now treats EVERY present edited cache as unverified when the
  transform force-recomputes, closing that asymmetry (a symmetric flag compare was avoided —
  it would over-refuse a benign edit that drops both the flag and the caches).
- **`<ignoredError sqref>` is SHIFTED, not refused (over-refusal fix).** The green-triangle
  error suppression Excel writes on nearly every "number stored as text" / inconsistent-
  formula column was treated as an unshiftable body reference and refused the whole edit (and
  any faithful certify). Its `sqref` is an ordinary coordinate the shift engine tracks, so it
  now shifts like a conditional-format/data-validation `sqref` — a ubiquitous benign construct
  no longer blocks structural edits.
- **What-if data table references shift.** A `<f t="dataTable" ref="C2:C5" r1="A1" r2="B1"/>`
  carries LIVE coordinates in ATTRIBUTES — the output-array extent (`ref`) and the column/row
  input cells (`r1`/`r2`) — none in the formula body. The formula path (which shifts only
  `<f>` TEXT) and the edited-body scan (which skips formula tags) both passed it over, so it
  committed stale: the table recomputed against the wrong input cell and declared the wrong
  extent, a silent value corruption. Its attributes now shift like any other coordinate.
- **`certify` compares `_xlfn.`/`@` tokens PER CELL, not per-sheet count.** The guards for the
  two engine-normalized-away tokens (the `_xlfn.` prefix and the implicit-intersection `@`)
  keyed on a per-SHEET multiset, so RELOCATING one between two cells on the same sheet (`@`
  moved C1→C5, turning one spill into a scalar and vice-versa) left the count unchanged and
  certified. They are now compared per cell, catching relocation as well as drop/add.
- **Form-control / OLE data bindings are shifted-or-refused and compared.** A control's
  `linkedCell`/`fmlaLink`/`listFillRange` (the cell it reads/writes) or a web-publish
  `sourceRef` on a FOREIGN sheet, qualified to the edited sheet, was left stale by the edit
  (the foreign-sheet scan checked only `<f>` bodies and `ref`/`sqref`); it now fails closed.
  And `certify` now compares every control binding — worksheet attributes plus legacy VML
  `<x:FmlaLink>`/`<x:FmlaMacro>` — so a foreign edit that re-points a control (to read a
  different value or run a different macro) is caught.
- **A chartsheet / dialogsheet no longer blocks structural edits (over-refusal fix).** These
  are listed in `<sheets>` like a worksheet but live at `xl/chartsheets/`·`xl/dialogsheets/`
  and carry no cell grid (their chart data ranges are shifted by the chart path). The
  non-standard-sheet-path guard refused any workbook containing one; it now recognizes them as
  grid-free and only fails closed on a genuinely unrecognized sheet path (macrosheets, which
  can carry XLM formula cells, stay fail-closed).
- **A structured table reference on an UNRELATED sheet no longer blocks the edit (over-refusal
  fix).** The `Table[Column]` guard (which fails closed because the shift tokenizer can mangle
  a cell-shaped column specifier) ran presence-based across every part, refusing an edit on
  one sheet because a table reference existed on another. Only formulas the edit actually
  REWRITES — the edited sheet, chart data ranges, and workbook defined names — can be mangled,
  so the guard is now scoped to those; a structured reference on a foreign sheet (copied
  verbatim, never shifted) is left alone.
- **A foreign sheet's x14/sparkline `<xm:f>` referencing the edited sheet is SHIFTED, not
  refused (over-refusal fix).** The foreign-sheet residual scan blanket-refused any `<extLst>`
  formula that named the edited sheet, on the premise that the shift path does not rewrite an
  extLst formula. That premise was wrong: the shift matches formula elements by LOCAL name, and
  `<xm:f>` (which holds every x14 CF/DV and sparkline formula) has local name `f` — so it is
  rewritten exactly like a plain `<f>`. A common dashboard-with-sparkline workbook is now
  edited faithfully (the `<xm:f>` range shifts) instead of refused; a genuinely unshifted body
  (a legacy `<formula>`/`<formula1>`, or an array `<f>`) still fails closed.
- **`certify` no longer refuses a workbook that contains a cell comment/note (over-refusal
  fix).** The part allowlist had no entry for `xl/comments*.xml` (nor threaded comments /
  persons), so certify refused any commented workbook — including xlq's own byte-faithful
  transform, which restructure commits without complaint. A comment carries only a display
  anchor and text (no value-affecting reference; an anchor on the EDITED sheet is caught
  upstream as an unshiftable attachment), so these parts are now known-safe.
- **Internal hyperlink `location`s are SHIFTED, not refused (over-refusal fix + faithful
  transform).** A table-of-contents / index link (`<hyperlink location="Data!A15">`) whose
  in-workbook target the edit moves caused restructure to refuse the whole edit — and because
  restructure refused, `certify` refused *both* a faithfully-shifted copy and a stale one,
  unable to tell them apart. The engine already COMPUTED the shifted location just to detect
  the hazard; it now APPLIES it (via the σ oracle, with the link's own sheet as host, so a
  local link on the edited sheet, or one qualified to it, follows — `A15`→`A16` — while a link
  to another sheet is untouched, on any sheet the link lives on). A delete that consumes the
  target yields `#REF!`, mirroring Excel. certify then compares the shifted destination, so a
  faithful edit certifies and a stale one is refused.
- **`certify` compares the CSE-array / data-table `<f>` flag.** A foreign edit that turns a
  plain formula into a legacy array formula (`<f t="array" ref=…>`), or widens the array
  extent, changes the computed value on pre-dynamic-array Excel (`=SUM(A1:A3*A1:A3)`
  implicit-intersects to a scalar; `{=SUM(…)}` computes the full sum) — but the engine strips
  `t`/`ref` on load, so the cell diff sees nothing. The `array`/`dataTable` flag and extent are
  now compared per cell (the reverse — an original array formula — is already refused upstream).
- **`certify` COMPARES Excel Tables instead of refusing them (over-refusal fix).** The part
  allowlist had no `xl/tables/` entry, so certify refused ANY workbook containing a table
  (Ctrl+T) on any sheet — including xlq's own faithful transform, which restructure commits
  (restructure only refuses a table it would have to MOVE). The table's reference surface —
  `ref` extent, `name`/`displayName`, column names, `totalsRowFunction`, and
  calculated-column / totals-row formulas — is now compared semantically (tolerating a foreign
  tool's cosmetic re-serialization), so a faithful edit certifies and a mangled/re-scoped table
  is caught.
- **A form control's list/combo-box source range (`fmlaRange`) is shifted-or-refused.** The
  edited-sheet body scan flagged `linkedCell`/`fmlaLink`/`listFillRange`/`sourceRef` but omitted
  the sibling `fmlaRange` (a `<controlPr>` SOURCE range), so it committed stale; it now fails
  closed like the others, and certify compares it.
- **An inline-string cell containing an `X:Y!` substring no longer over-refuses.** The 3D-span
  and non-ASCII-qualifier guards scanned the whole worksheet part text, so ordinary prose in a
  cell (`Enter totals in A1:A5!`, an openpyxl inline string) was misread as a 3D interior-tab
  span and refused the edit. The scan is now scoped to FORMULA element bodies (`<f>`,
  `<formula*>`, `<definedName>`), where a live reference can actually appear.
- **A defined name containing a period (`A1.tax`) is no longer corrupted (silent-wrong).** The
  reference tokenizer's boundary predicate (`ident_tail`) treated a letter/`_`/`(` after a
  candidate cell ref as disqualifying but omitted `.`, while `shift_formula`'s own boundary
  treats `.` as identifier-continuation. So `A1.tax` (a legal Excel name) tokenized as the live
  cell `A1` and a row insert rewrote the NAME to `A2.tax` (→ `#NAME?`) with no residual —
  committed by a "verified" edit. `.` is now a boundary, aligning the two tokenizers.
- **`certify` reads value-affecting workbook settings namespace-prefix-agnostically.** The
  `date1904`/`fullPrecision`/`calcMode`/`iterate` compare and the recalc-on-load check found
  `<calcPr>`/`<workbookPr>` with a raw `find("<calcPr")`, blind to a prefixed `<x:calcPr>` — so
  a foreign edit that set `fullPrecision="0"` on a rebound-prefix element (which Excel honors,
  since namespace resolution is prefix-agnostic) read as the default and CERTIFIED. Both now
  match by local name.
- **An Excel Table on the edited sheet is refused only when the edit MOVES its extent
  (over-refusal fix).** restructure refused any edit on a sheet owning a table, on presence
  alone; a Table with a summary/total block below it (a very common layout) blocked an
  insert/delete strictly below or right of the table, which leaves the table's `ref` correct. It
  now refuses only when shifting the table's `<ref>` under the edit would change it (a
  formula-bearing table is still refused, as its formula may reference the moved region).
- **`certify` no longer refuses a workbook carrying an inert customXml data store
  (over-refusal fix).** Office/SharePoint custom-XML islands (`customXml/`) carry no worksheet
  coordinate — Excel formulas cannot read them — yet the part allowlist refused xlq's own
  transform of any workbook containing one; they are now known-safe.
- **An `<oleObject link>` linked-cell source is shifted-or-refused.** The linked-cell source of
  an embedded OLE object (`link="Sheet1!$A$11"`, present when the object has no `r:id`) was
  copied verbatim and left stale — the object would source the wrong cell after the edit. It is
  now flagged fail-closed like the other control/OLE bindings (and compared by certify).
- **An insert that would push last-row/last-column data off the grid is refused (silent-wrong
  fix).** The row/cell RELOCATION path (`shift_line`/`shift_cell_tag`) had no grid clamp, while
  the reference-shift path correctly `#REF!`s an overflow — so inserting above a datum at row
  1048576 emitted an out-of-grid `<row r="1048577">` and orphaned that datum out of a `SUM`
  (a `15 → 10` value change committed by a "verified" edit). Excel refuses this ("cannot shift
  nonblank cells off the worksheet"); xlq now detects the overflow up front and fails closed.
- **A structural edit on a CJK/Cyrillic-named sheet is no longer refused when the edit moves
  nothing referenced (over-refusal fix).** An unquoted non-ASCII sheet qualifier (`集計!A11` —
  the normal spelling Excel writes for Asian-language tabs) can't be parsed by the ASCII
  tokenizer, so it fails closed — but the guard was presence-based, refusing EVERY row/column
  edit on such a sheet regardless of where the edit landed. It is now affect-based: the ASCII
  cell part after the `!` is parsed and the σ oracle is asked whether THIS edit moves it, so an
  edit far from any reference (a row-50 insert vs a row-11 reference) commits, while an edit
  that actually moves the referenced cell still refuses.

The compare surface certify extracts per worksheet remains an enumerated *semantic*
surface (it must tolerate a foreign tool's cosmetic re-serialization), so its
completeness over non-cell references is asserted, not proven — the honesty caveat the
accompanying paper states in its scope section. The whole-part boundary, however, is now
fail-closed.

### Robustness
- A panic in any command becomes a machine-readable JSON error (exit 70) with a
  path-safe, basename-only source location, instead of a raw multi-line dump.
- `SIGPIPE` is restored to its default disposition, so `xlq … | head` dies cleanly
  instead of panicking on a closed pipe.
- A recursion-depth guard in the vendored formula parser turns a pathologically
  nested formula into a parse error instead of a stack-overflow abort.

### Packaging
- The crate is now self-contained: the Excel function catalog moved to
  `data/excel-functions.txt` and the test fixtures into `tests/fixtures/`, so it
  builds and tests without reaching outside its own directory.
- The five dev/bench binaries are gated behind a non-default `devtools` feature, so
  `cargo install` ships only the `xlq` binary. The full test suite runs under
  `cargo test --features devtools`; a bare `cargo test` runs unit + direct-fixture
  tests.
- Added crate metadata (README, keywords, categories, license/notice copies).

### Publishing
- The engine dependency is now wired for crates.io: the vendored IronCalc fork is
  consumed as renamed publishable packages (`xlq-ironcalc`, `xlq-ironcalc-base`,
  lib names unchanged) via the multiple-locations `path` + `version` pattern, so
  no `[patch.crates-io]` is needed and a published `xlq` links the correct engine.
  `cargo publish --dry-run` is green for the leaf `xlq-ironcalc-base`. Publishing
  the three crates (a permanent, account-scoped action of republishing a
  third-party engine fork) is left to the maintainer — see `PUBLISHING.md` for the
  exact bottom-up sequence. Local dev/install is unchanged (resolves from
  `../vendor`).

## [0.1.0]

- Initial release: read-only `inspect`, `diff`, `calc`, and the first
  transactional `apply` / `restructure` / `certify` write path with receipts.
