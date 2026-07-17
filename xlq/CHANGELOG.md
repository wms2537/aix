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
- **The multi-sheet 3D-span guard is now AFFECT- and ORDER-aware (silent-wrong + over-refusal
  fix).** A 3D span (`SUM(Sheet1:Sheet3!A5)`) shares one coordinate across several tabs, but a
  row/column edit moves cells on only the edited tab — so shifting the coordinate uniformly
  orphans the other tabs' data. The guard refused only when NEITHER endpoint was the edited
  sheet, so editing a NAMED ENDPOINT (`Sheet1`) silently mis-shifted `A5`→`A6` (`123`→`100`,
  no residual); and once that hole was closed by refusing every span, editing a sheet
  completely OUTSIDE the span was over-refused (one consolidation formula disabled the whole
  workbook). The guard now consults the workbook's tab order and the edit's affect: it refuses
  a span only when the edited sheet lies WITHIN the span's tab range AND the edit actually
  moves the referenced cell — so an endpoint/interior edit that moves the coordinate refuses,
  an outside-the-span edit or one that moves nothing commits, and a self-span (`Sheet1:Sheet1`)
  shifts normally. Partial/mixed and fully quoted span spellings (`Sheet1:'Sheet2'!`,
  `'A-Sheet:B-Sheet'!`) are now parsed correctly (a partial-quoted span previously evaded the
  guard and committed a stale reference).
- **A number-format change on a FORMULA cell is now visible to certify.** The diff classifier
  reported a format-only difference only for non-formula cells, so a formula cell whose number
  format changed (same stored formula and result) was classified as "no change" — invisible
  even under `fullPrecision="0"`, where it corrupts downstream computed values (`A15`
  reformatted `0.00`→`0` displays `18` not `18.33`, and `=A15*3` recomputes `54` not `54.99`).
  Format diffs on formula cells are now classified as `format` and, under precision-as-displayed,
  disqualified.
- **A defined name spelled like a cell reference is refused only when it is a real hazard
  (over-refusal fix).** A named range whose text matches an A1 reference (`Q1`, `FY21`, `H1`,
  `TAX1` — common in financial models) made the WHOLE workbook un-editable: every row/column
  edit on every sheet was refused. It is now refused only when both conditions hold — the name,
  read as a cell, would actually MOVE under this edit, AND it is used in a formula the edit
  SHIFTS (the edited sheet, a chart, or another defined name). A name used only on an unrelated
  sheet, or whose aliased coordinate the edit doesn't touch, no longer blocks the edit.
- **A Table's computed column / totals-row formula is refused only when it references the
  edited sheet (over-refusal fix).** A table with a `<calculatedColumnFormula>` or
  `<totalsRowFormula>` (`Total = [@Price]*[@Qty]`, `SUBTOTAL(109,Tbl[Amount])` — one of the most
  common table features) was refused on PRESENCE, disabling edits even on an unrelated sheet.
  These formulas are usually STRUCTURED references (table-local, by column name), which name no
  sheet coordinate. The guard now runs the σ oracle over the table formula and refuses only when
  it carries a reference to the edited sheet that the edit would MOVE (a table part is never
  rewritten); a table-local structured formula is left alone.
- **`certify` compares the autoFilter AND/OR combinator, not just the leaf predicates.** The
  criteria compare captured only the leaf filter elements (`customFilter`, `filter`, …), missing
  the `<customFilters and>` container attribute (the AND/OR combinator over two predicates) and
  `<filters blank>`. Flipping `and="1"`→`"0"` changes which rows the filter hides — a value input
  to `SUBTOTAL(101–111)`/hidden-ignoring `AGGREGATE` — and certified. The container attributes are
  now compared.
- **`certify` compares a formula cache's cell TYPE, not just its text.** The stored-cache guard
  recorded only the `<v>` text keyed by cell, so a foreign edit that retyped a formula result
  from number to text (`<v>55</v>` with `t="str"` — same digits) matched and certified; under
  manual/`fullCalcOnLoad="0"` Excel then aggregates `A11` as text (`SUM` treats it as 0). The
  cache signature now includes the cell `t` type (`n`/`str`/`b`/`e`), with the value still
  tolerating a benign numeric renumber.
- **`certify` no longer refuses a workbook with rich-value data (over-refusal fix).** In-cell
  images (`=IMAGE()`) and linked data types (Stocks/Geography) emit `xl/richData/*` parts that
  are index-linked from cells via the cell `vm` attribute and carry no shiftable coordinate — the
  same property as the already-allowlisted `xl/metadata.xml`. They were missing from the part
  allowlist, so certify refused xlq's own transform of any such (common, modern) workbook; now
  known-safe.
- **A delete that empties a `<mergeCells>`/`<dataValidations>` container omits it (invalid-output
  fix).** When a row/column delete consumed every child of one of these containers, the children
  were dropped but the parent survived empty with a stale `count` — schema-invalid (the child has
  `minOccurs=1`), which Excel opens with a repair prompt. An emptied container is now removed from
  the output, matching the `<cols>` path.
- **A defined name rebound to a VBA function/macro is caught by certify (false-certify fix).**
  certify compared a defined name's `(name, scope, refers-to)` but not its `function` /
  `vbProcedure` / `hidden` attributes, so a foreign edit reclassifying a data-range name into a
  VBA UDF/macro binding (and hiding it) — a computed-value and macro-execution change — certified.
  Those flags are now part of the compared signature.
- **`certify` no longer refuses a ribbon-customized workbook (over-refusal fix).** The ribbon
  extensibility part (`customUI/customUI14.xml`, ubiquitous in `.xlsm`/enterprise templates)
  carries no cell coordinate (its callbacks are VBA macro-name strings, and the macro binary is
  compared separately), but was missing from the allowlist, so certify refused its own transform;
  now known-safe.
- **Shared-formula materialization `#REF!`s an off-grid dependent (invalid-output fix).** Expanding
  a shared formula guarded only the underflow edge (a reference driven above row 1 / left of
  column A → `#REF!`), but not the overflow edge — a dependent offset past column XFD / row
  1048576 materialized an off-grid token (`XFE1`) that Excel reads as `#NAME?`. It now `#REF!`s on
  both edges, mirroring the reference-shift path's grid clamp.
- **`certify` compares the workbook write-reservation password (`<fileSharing>`) (security fix).**
  The protection compare covered `<workbookProtection>` and sheet protection but not
  `<fileSharing>` (the workbook-level write-reservation password — `reservationPassword` or its
  modern `algorithmName`+`hashValue`+`saltValue`+`spinCount` hash — plus `readOnlyRecommended`), so
  a foreign edit that stripped or weakened it certified as faithful. Now compared alongside the
  other protection elements.
- **An edited-sheet drawing/image/chart is refused only when the edit moves its anchor
  (over-refusal fix).** The attachment guard refused ANY edited-sheet drawing on presence, so a
  logo or chart pinned above/left of a data region blocked every insert/delete below/right of it,
  even though xlq copies the drawing verbatim and the anchor never moves. The guard now resolves
  the drawing part and affect-checks its `<xdr:from>`/`<xdr:to>` cell anchors: a drawing the edit
  displaces still fails closed, one outside the edited range commits. (Comments/VML and other
  attachment types remain presence-refused — their anchor form differs.)
- **`certify` no longer refuses a workbook with data connections / query tables / modern form
  controls (over-refusal fix).** `xl/connections.xml` (external data-source definitions) and
  `xl/queryTables/*` (whose extent lives in the associated table part) carry no cell coordinate,
  and `xl/ctrlProps/*` form-control bindings are now compared like their VML/inline
  counterparts — all three were missing from the allowlist, so certify refused its own transform
  of any workbook using them.
- **A modern form-control binding (`xl/ctrlProps/`) that references the edited sheet is now
  refused, not left stale (silent-wrong + certify-inversion fix).** A `<formControlPr fmlaLink>`
  in a `xl/ctrlProps/*` part lives outside the worksheet, so restructure's foreign-sheet scans
  skipped it and committed it STALE with no residual — and because certify DOES compare these
  bindings (added the previous round), xlq's own stale transform inverted certify: it refused the
  FAITHFUL edit and certified the value-wrong one. restructure now scans `xl/ctrlProps/*` for a
  binding qualified to the edited sheet that the edit would move and fails closed, exactly like
  the inline `<controlPr>` case — so restructure never commits a stale binding and certify never
  inverts.
- **Form-control `fmlaGroup` / `fmlaTxbx` bindings are now covered (false-certify + silent-wrong
  fix).** A `<formControlPr>`'s option-button-GROUP cell link (`fmlaGroup`) and edit-box cell link
  (`fmlaTxbx`) are genuine cell references — the modern mirror of the VML `<x:FmlaGroup>`/
  `<x:FmlaTxbx>` that were already compared — but both were omitted from the two attribute sets, so
  `certify` did not compare them (a foreign RE-POINT of an option-group's linked cell was CERTIFIED)
  and restructure did not refuse one qualified to the edited sheet (the control was committed
  silently re-bound to the wrong, now-moved cell). Both are now included, so a re-point is refused
  and an edited-sheet-crossing binding fails closed, matching the `fmlaLink` sibling.
- **`certify` catches a WITHIN-cell relocation of the implicit-intersection `@` operator.** The
  engine-normalized-token signature counted `@` per cell rather than recording positions, so
  moving `@` between operands in one cell (`@A1:A3-A1:A3` → `A1:A3-@A1:A3` — same count, a
  different spill) was certified. The signature now records each `@`'s position in the
  `@`-stripped body, so a same-count relocation differs.
- **`certify` tolerates a benign hyperlink-URL trailing slash (over-refusal fix).** A foreign
  tool (openpyxl/Excel) that renormalizes a bare-authority external target (`https://example.com`
  → `https://example.com/`) — the same resource — was a spurious refusal, since the target was
  byte-compared. A single trailing `/` is now stripped before comparison; a real retarget (a
  different host or path) still differs.
- **`certify` now compares external DATA-SOURCE targets and ribbon autorun callbacks
  (security fix).** `xl/connections.xml` (a `<webPr url>` web query, a `<dbPr command>` SQL
  string, an ODBC/OLEDB connection string, an OLAP source), its `xl/queryTables/*` connection
  bindings, and `customUI/*` ribbon callbacks were allowlisted as carrying no shiftable cell
  coordinate but never compared — so a foreign edit that REPOINTED the data source to an
  attacker host (SSRF / intranet-URL exfiltration, with attacker-controlled data injected into
  the connected cells on the next refresh — a value change no cell diff can see) or INJECTED an
  `onLoad` autorun callback was certified. xlq's transform copies these parts verbatim, so they
  are now compared by a normalized, order-independent element/attribute signature: a faithful
  edit (and a foreign tool's cosmetic reserialization) matches; a repoint or injection differs.
- **`restructure` no longer corrupts a cell-shaped suffix of a non-ASCII / backslash-prefixed
  defined name (silent-wrong fix).** A defined name whose spelling is a Unicode (e.g. CJK) or
  leading-`\` prefix immediately followed by a grid-valid A1 spelling — `売上A5`, `\A5`, used
  unqualified in a formula body — had its trailing `A5` shifted as if it were a fresh cell ref
  (`売上A5`→`売上A6`, an undefined name → `#NAME?`, a silent value corruption). The shift
  algebra's token-boundary predicate now treats a preceding non-ASCII scalar or backslash as
  name-continuation (Excel names admit Unicode letters/digits and `\`), so the whole name is
  left intact while a genuinely separate reference in the same formula still shifts. Root-cause
  fix in `shift_formula`, so every call site (worksheet `<f>`, shared-formula materialization,
  defined names) is covered.
- **`certify` treats the two OOXML encodings of an internal hyperlink as equivalent
  (over-refusal fix).** An in-workbook jump (`A4`→`Data!A1`) has two standard encodings: a
  relationship `Target="#Data!A1"` (openpyxl and other library writers) and a
  `location="Data!A1"` attribute (Excel/LibreOffice). certify keyed them as independent
  `(location, target)` fields, so a faithful edit that merely round-tripped the encoding was
  refused. Both now canonicalize to the same `(dest, ext="")`; a genuine external retarget (a
  phishing URL, a mispoint to another file) still lands in `ext` and differs.
- **`certify` vouches a faithful edit's PRESERVED formula caches by evaluation, not just by a
  stored-cache match (over-refusal + strengthening).** xlq's transform BLANKS every shifted
  formula's cache (it cannot recompute engine-free), so a stored-cache-vs-stored-cache
  comparison refused the common case — a normal Excel/LibreOffice save that preserves the
  correct cache but does not set `fullCalcOnLoad`. When the engine fully and deterministically
  covers xlq's proven transform (no unsupported / policy-limited / user-defined / VOLATILE
  function), certify now EVALUATES the transform and vouches each foreign cache against the true
  computed value. This both removes the over-refusal AND strengthens the guard: the prior
  comparison could not tell a correct cache (`55`) from a fabricated one (`999`) — both were
  refused; now the correct one certifies and the lie is refused. Gated on coverage, so an
  unsupported or volatile function never launders a wrong value.
- **`restructure` invalidates every formula cache on commit, never committing a STALE computed
  value (silent-wrong fix).** A structural edit changes computed values — a deleted data row
  changes a `SUM`, which changes a cell that reads that `SUM`, transitively across the whole
  workbook — but the byte-surgery path copied each formula's stored result cache (`<v>`)
  verbatim, so `SUM(A1:A10)` shifted to `SUM(A1:A9)` still displayed the OLD `55` (true `50`),
  and a `#REF!`-orphaned cell kept a fabricated numeric cache. Excel/LibreOffice (with no
  `fullCalcOnLoad`), and every cache-reading tool (openpyxl `data_only`, pandas), showed the
  stale value. xlq is engine-free and cannot recompute nor track the transitive affected set, so
  it now drops the `<v>` of EVERY formula cell on EVERY worksheet — exactly what openpyxl does on
  save — leaving `<f>` intact for the reader to recompute; literal (non-formula) values are
  untouched. (Earlier rounds missed this because the test fixtures shipped blank `<v/>` caches.)
- **`restructure` refuses an edit that would move a drawing shape's live cell reference
  (silent-wrong fix).** A linked shape/textbox mirrors a cell via `textlink="Sheet1!$A$8"` (what
  Excel writes for a shape whose formula bar reads `=A8`); a graphic frame does the same via
  `<xdr:f>`. The drawing guard checked only the shape's ANCHOR position, so a shape anchored away
  from the edit was copied verbatim with its textlink left pointing at the pre-edit cell —
  silently mirroring a different cell's value. The guard now also affect-checks the textlink /
  graphic-frame reference (σ oracle, edited sheet as host, so qualified and unqualified refs both
  count) and refuses when the edit moves it; `certify` now compares these references too, so a
  foreign re-point on an unaffected shape is caught rather than certified.
- **`certify` no longer refuses a PivotTable workbook — including xlq's own transform
  (over-refusal fix).** `xl/pivotCache/*` and `xl/pivotTables/*` were on neither the fail-closed
  allowlist nor a comparator, so certify refused every workbook with a pivot, even when the pivot
  sourced an unrelated sheet and restructure produced a provably faithful transform of it. They
  are now allowlisted and compared by `pivot_refs`: the source range (`<worksheetSource ref>`,
  which the transform shifts for the edited sheet), the render location, the consolidation range,
  and the connection binding. A faithful edit matches; a repointed source, a moved render extent,
  or a re-bound connection differs.
- **Shared-formula materialization no longer corrupts a non-ASCII / backslash-prefixed defined
  name (silent-wrong fix).** The round-31 fix that taught the token-boundary predicate to treat a
  Unicode/backslash prefix as name-continuation (`名A5`, `\A5`) was applied to `shift_formula` but
  NOT to `offset_formula`, which materializes shared-formula dependents — so a shared master
  `名A5*2` expanded its dependents to `名A6*2`/`名A7*2` (undefined names → `#NAME?`). The two
  boundary predicates are now a SINGLE shared function (`ref_start_boundary`), so they cannot
  drift again.
- **`certify`'s cache oracle respects "precision as displayed" (false-certify + over-refusal
  fix).** The round-31 evaluation oracle vouches a foreign cache against ironcalc's `evaluate()`,
  which always computes at FULL precision — but under `<calcPr fullPrecision="0">` Excel computes
  on the ROUNDED DISPLAYED values, so the oracle would CERTIFY a wrong full-precision cache and
  REFUSE the correct displayed-precision one (`=A1` with `A1`=1.4 shown as `1`). The oracle is now
  disabled under precision-as-displayed; a present cache in that mode stays unverified (the safe,
  conservative refusal).
- **`restructure` refuses a stale legacy-VML form-control binding on a foreign sheet
  (silent-wrong fix).** A form control's cell link lives in element TEXT
  (`<x:FmlaLink>Sheet1!$A$8</x:FmlaLink>`) inside `xl/drawings/*.vml`, which is neither a
  worksheet nor an attribute — so the worksheet cross-ref scan and the `ctrlProps` attribute scan
  both skipped it, and a control on another sheet bound to the edited sheet was committed stale
  (silently re-bound to the wrong cell, and inverting certify, which does compare VML FmlaLink).
  The residual scan now checks `.vml` bindings via the σ oracle and fails closed, symmetric with
  the modern `ctrlProps` case.
- **`restructure` and `certify` no longer refuse an annotated workbook on comment presence
  (over-refusal fix).** A cell comment / note (and legacy VML anchor) anywhere on the edited sheet
  blocked ALL row/column edits, even when the edit was nowhere near it — a comment is one of the
  most common constructs in real financial workbooks. Comments and VML anchors are now
  AFFECT-checked exactly like drawing anchors (`<comment ref>` / `<threadedComment ref>` /
  `<x:Row>`/`<x:Anchor>`): refused only when the edit MOVES the anchored cell. The edited sheet's
  own VML is additionally checked for a control binding to a moved cell (edited-sheet host, so a
  local unqualified `$A$8` counts) so the walkback opens no silent-wrong hole.
- **`certify` catches a value-changing tamper of a range-INTERSECTION formula (false-certify
  fix).** ironcalc collapses Excel's range-intersection operator — a space between two references,
  `=A1:A10 A4:A4` (the `=Revenue January` idiom) — to `@A1:A10`, DROPPING the second operand, so
  the loaded-model diff was blind to a foreign edit changing that operand's value (`A4:A4`→`A7:A7`,
  4→7). The engine-normalized-token signature now also records the whitespace-canonicalized raw
  body when a top-level intersection is present, so an operand change differs (and benign
  re-spacing does not). Operator-spacing (`A1 + A2`) and function calls are not mistaken for it.
- **`restructure` array/dynamic-array formulas are affect-based, not presence-refused
  (over-refusal fix).** ANY `<f t="array" ref=…>` on the edited sheet refused EVERY structural
  edit regardless of distance — and since Excel persists all modern dynamic-array spills
  (`FILTER`/`UNIQUE`/`SORT`/`SEQUENCE`/`XLOOKUP`) as `t="array"`, that categorically rejected a
  ubiquitous workbook class. An array is now refused only when the edit MOVES its `ref` extent or
  a cell its body references; an unaffected array is copied verbatim and the edit commits.
- **`certify` no longer counts a value-less style-only cell as an add/remove (over-refusal
  fix).** A merged title's covered cells, which Excel/LibreOffice materialize as `<c r="B1"
  s="1"/>` (no `<v>`, no `<f>`), were classified as `added`/`removed` and disqualified a faithful
  edit. A cell present on only one side with no formula and a null value is display-only and
  cannot change a computed result, so it is no longer a value divergence.
- **`certify` ignores the inert `formula2` of a list-type data validation (over-refusal fix).**
  For a `type="list"` dropdown, `formula2`/`operator` are meaningless (Excel uses `formula2` only
  with `between`/`notBetween`), but LibreOffice writes `<formula2>0</formula2>` for every list DV;
  comparing it spuriously refused a faithful round-trip. The construct comparator now drops
  `formula2` for a list DV while keeping it for the types where it is a real bound.
- **`certify` no longer disables its cache oracle workbook-wide on one volatile function
  (over-refusal fix).** `NOW`/`TODAY`/`RAND`/`OFFSET`/`INDIRECT` anywhere turned the whole cache
  oracle off, so a faithful edit that PRESERVED a verifiable NON-volatile cache (a `SUM`) without
  `fullCalcOnLoad` was refused as collateral. The oracle is now built regardless (all functions
  are engine-supported, so it evaluates correctly) and each individual VOLATILE cell is skipped
  (Excel recomputes those on load, so their cache never surfaces stale) — a fabricated non-volatile
  cache is still caught.
- **The volatile-cache skip now respects MANUAL calc mode (false-certify fix).** The skip above
  assumed Excel recomputes a volatile cell on load — true in AUTO mode, but a workbook in MANUAL
  calc mode (`<calcPr calcMode="manual">`, no `fullCalcOnLoad`) is NOT recalculated on open and
  displays every stored cache verbatim, so a fabricated cache in an `OFFSET`/`INDIRECT`/`NOW` cell
  was certified. The skip is now disabled under manual calc mode, where a volatile cell's cache is
  verified like any other.
- **`certify` catches a cross-sheet SUBTOTAL/AGGREGATE hidden-row change (false-certify fix).**
  The manual-hidden-row guard paired each sheet's hidden rows with THAT sheet's hidden-ignoring
  aggregate, but `Sheet2!B1 = SUBTOTAL(109, Sheet1!A1:A10)` takes its hidden-row input from the
  REFERENCED sheet — so hiding a data row on `Sheet1` changed the aggregate (55→50) with the guard
  never linking them, and certify blessed it. When any sheet carries such an aggregate, every
  sheet's hidden-row set is now compared (a sound over-approximation), since the aggregate can
  reference any sheet.
- **`certify` tolerates a foreign editor coalescing adjacent data-validation / conditional-
  formatting ranges (over-refusal fix).** A `sqref` of two adjacent ranges (`B1:B11 C1:C11`, what
  openpyxl writes) that a real editor saves as the equivalent single rectangle (`B1:C11`, what
  Excel/LibreOffice write) was compared as a raw string and refused despite covering the identical
  cells. The `sqref` is now canonicalized to its cell coverage (a full rectangle collapses to that
  rectangle, so single ranges are unchanged), so a lossless coalesce certifies while a genuinely
  different cell set still differs.
- **`certify` treats a number-format change as value-affecting under "precision as displayed".**
  A `format`-only difference is normally benign, but with `<calcPr fullPrecision="0">` Excel
  computes formulas on the ROUNDED DISPLAYED values, so a cell's number format is a value input
  (`A1` reformatted `0.00`→`0` rounds `1.44`→`1`, and `=A1*10` recomputes `10` instead of
  `14.4`). Under that mode, format diffs are now disqualifying; with full precision they remain
  benign (no over-refusal).
- **`certify` ignores the display-only `hiddenButton`/`showButton` on an AutoFilter
  `<filterColumn>` (over-refusal fix).** Those attributes govern only the filter dropdown BUTTON's
  visibility — pure display, no effect on which rows the filter hides — but openpyxl writes them
  explicitly at their defaults, so comparing them refused a value-identical edit. They are dropped
  from the criteria comparison; the value-affecting `colId` and predicate elements remain compared.
- **`certify` allowlists slicer / timeline parts (over-refusal fix).** `xl/slicerCaches/*`,
  `xl/slicers/*`, `xl/timelineCaches/*`, and `xl/timelines/*` bind to a pivot/table by name/ID and
  carry no shiftable A1 coordinate (like the already-allowlisted pivot parts), so restructure
  copies them verbatim — but they were outside certify's fail-closed allowlist, so certify refused
  its own faithful transform of any slicer/timeline dashboard. They are now allowlisted; their
  filter effect surfaces in the pivot's cached output cells, which the cell diff already compares.
- **`certify`'s cache oracle isolates trustworthy cells in a live-data workbook instead of
  disabling itself (over-refusal fix, soundly).** When a workbook uses an UNSUPPORTED,
  policy-limited (`RTD`/`WEBSERVICE`/`CUBEVALUE`), or user-defined function *anywhere*, the
  evaluation oracle was disabled workbook-wide, so a faithful edit preserving a verifiable cache
  (a pure `SUM`) was spuriously REFUSED. It now isolates the trustworthy cells by POISON-AND-DIFF:
  every cell whose formula calls such a function is overwritten with a constant and the model
  re-evaluated; a cell whose value CHANGES depends on an unreproducible result and is excluded,
  while a cell that is unchanged across the normal evaluation and two distinct poisonings is
  provably independent of it and its engine value equals Excel's. This is SOUND where the naive
  "vouch any clean matching value" is not: an error-masking wrapper (`IFERROR(RTD(),5)`, `ISERROR`,
  `COUNT`, …) yields a clean-but-WRONG engine value, but poisoning the `RTD` cell changes the
  wrapper's result, so it is correctly excluded — a fabricated cache matching the engine's `5`
  still REFUSES. Uses the engine's real dependency graph, so it covers defined names / structured
  references / `INDIRECT` that a hand-written reference walker would miss. The RTD cell's OWN cache
  (external data the engine cannot reproduce) remains, correctly, unverifiable → refused.
- **`certify` now compares an Excel Table's OWN AutoFilter criteria, not just worksheet AutoFilters
  (false-certify fix).** A `<table>` in `xl/tables/*.xml` carries its own `<autoFilter>`; scanning
  only worksheets let a foreign change to a table-filter predicate — a value input to a table
  `SUBTOTAL(1–11)` / hidden-ignoring `AGGREGATE` — certify silently. The table parts are now scanned
  too, keyed by class (`"table"`, so a benign part-renumber does not false-refuse while a real
  criterion change still differs within the sorted set).
- **`certify` normalizes an inert leading `=` in a CF/DV formula body (over-refusal fix).** A
  conditional-formatting / data-validation formula may carry a leading `=` (`=Lists!$A$1:$A$3`) that
  Excel and LibreOffice both accept and normalize away, so a foreign editor dropping (or adding) it
  is a value-identical edit. A single leading `=` is now stripped before keying the construct, so its
  presence/absence no longer flips the comparison.
- **`restructure` refuses an unquoted non-ASCII sheet qualifier only when the edit actually MOVES a
  reference, not on its mere presence (over-refusal fix, soundly).** On an ASCII-named edited sheet a
  non-ASCII qualifier (`集計!A5`) necessarily names a *different* sheet the edit cannot move, so a
  formula that references only such sheets is now written verbatim instead of refused. The check is
  affect-based: the non-ASCII-qualified references are neutralized and the σ algebra is run over the
  remainder — if any edited-sheet reference (bare or ASCII-qualified) still shifts, the edit is
  refused (never a stale bare reference); a non-ASCII 3D span, which may enclose the edited sheet as
  an interior tab, is also refused. The back-walk captures the whole qualifier including any ASCII
  cell-like prefix (`A1計!`), so that prefix can never leak out to be mis-shifted as an edited cell.
- **`certify`'s structural-reference scan is now namespace-aware — a prefixed `<x:hyperlink>`/
  `<x:mergeCell>`/`<x:autoFilter>` can no longer evade it (SECURITY, false-certify fix).** The scan
  matched elements by a raw `<hyperlink` substring, blind to a namespace-PREFIXED form. A foreign
  editor could bind a prefix to the spreadsheetML main namespace and inject `<x:hyperlink r:id=…>`
  pointing at an external phishing/malware URL (the target living in the sheet's `_rels`, which no
  other comparator scans); the prefixed element was invisible, so its reference set stayed empty and
  matched xlq's own (empty) transform — a **CERTIFIED phishing hyperlink**. The scan now walks the
  part with a namespace-aware parser keyed by local name (mirroring the earlier `definedName` fix); a
  benign prefix rebind of the same reference still keys identically, so no faithful edit is refused.
- **`certify` normalizes redundant sheet-name quoting in a defined name (over-refusal fix).** openpyxl
  writes the ubiquitous `_xlnm._FilterDatabase` autofilter name QUOTED (`'Data'!$A$1:$B$10`) while
  Excel/LibreOffice write it unquoted (`Data!$A$1:$B$10`) — semantically identical, so comparing the
  raw refers-to bodies spuriously refused a faithful edit of essentially any autofiltered workbook. A
  redundant quote around a plain-identifier sheet name (immediately followed by `!`/`:`) is now
  dropped before keying; names that genuinely need quotes (spaces, a leading digit, an embedded `''`)
  keep them, so no two distinct sheet names can collide. Applied to the CF/DV construct bodies too.
- **`restructure` counts only NEWLY introduced `#REF!`, not one already present (over-refusal fix).**
  The auxiliary shift helpers (defined names, chart/cross-sheet formula bodies) counted `#REF!` in the
  shifted output with no baseline subtraction, so a workbook already carrying a dangling `#REF!` (a
  common leftover from an earlier column/name deletion) inflated the reported error count — and for a
  **move-rows** edit, which refuses on any nonzero error count as a straddle, that spuriously blocked
  the operation on a whole class of real files. These sites now subtract the pre-shift `#REF!` count
  (matching the edited-sheet path), so only a reference *this* edit breaks is counted.
- **`certify` vouches a preserved formula cache at Excel's 15-significant-figure precision, not exact
  f64 (over-refusal fix).** The evaluation oracle rendered ironcalc's raw `f64` (`100*1.1` →
  `110.00000000000001`) and compared it against a preserved cache with EXACT float equality, so a real
  editor's correctly-rounded stored value (`110`) was not vouched — refusing a faithful edit of
  essentially any workbook doing fractional arithmetic and saved without `fullCalcOnLoad` (the normal
  state of an Excel/LibreOffice file). Numeric cache comparison now rounds both sides to 15 significant
  figures, which is exactly Excel's own equality: a value that genuinely differs beyond float noise
  still differs, so a fabricated cache is not vouched.
- **`certify` treats a number-format change as value-affecting when a `CELL()` info-function reads it
  (false-certify fix).** `=CELL("format"/"color"/"parentheses", A1)` returns a value derived from
  `A1`'s number format, so a foreign edit restyling `A1` (numFmtId `0`→`2`) changes the formula's Excel
  result — but certify classified that as a benign `format`-only diff and CERTIFIED. When any worksheet
  formula calls a number-format-sensitive `CELL()` (or `CELL()` with a non-literal, unresolvable info
  type), format diffs are now disqualifying (the same treatment as precision-as-displayed); a workbook
  with no such formula still tolerates a cosmetic format change.
- **`xl/volatileDependencies.xml` is handled like `xl/calcChain.xml` (over-refusal fix).** This
  rebuildable volatile/RTD dependency cache carries `<tr r>` cell coordinates that would go stale after
  a shift; `restructure` now DROPS it (as it already drops calcChain — Excel rebuilds both on open),
  and `certify` allowlists it (a foreign edit may keep it; it is value-inert with no verifiable
  coordinate). Previously certify refused its own faithful transform of any workbook carrying the part.
- **`certify` excludes date-function caches from the evaluation oracle in a 1904-date-system workbook
  (SILENT-VALUE false-certify + over-refusal fix).** The engine (ironcalc) hardcodes the 1900 epoch,
  so under `<workbookPr date1904="1">` its `YEAR`/`DATE`/`EOMONTH`/… results are off by the 1462-day
  shift. The oracle trusted those values, so it both CERTIFIED a forged cache holding the engine's
  wrong 1900-system value *and* REFUSED the correct 1904 value — a silent ~4-year value corruption
  blessed by certify. Date-system-dependent functions are now added to the oracle's unvouchable set
  under date1904 (the same poison-and-diff exclusion used for `RTD`/UDF), so such a cache stays
  unverified and certify refuses rather than vouch a wrong value.
- **`certify` signs a range-intersection formula whose second operand is PARENTHESIZED (false-certify
  fix).** ironcalc collapses a top-level intersection (`A2:A9 (A5:A5)`, the space operator) to its
  first operand, dropping the second, so the loaded-model diff is blind to a change of that operand.
  certify already signed the raw body when it detected an intersection, but the detector excluded `(`
  as an operand start, so a parenthesized second operand slipped through and a value-changing mangle
  (`(A5:A5)`→`(A6:A6)`) certified. `(` now starts an operand.
- **`certify` extracts a CDATA-wrapped form-control binding body (false-certify fix).** Excel emits a
  legacy VML control's `FmlaMacro`/`FmlaLink` binding as `<![CDATA[…]]>`; the text extractor had no
  CDATA branch, so the body read as empty and two distinct bindings collapsed to one key — a foreign
  RE-POINT of the macro/link (`SafeMacro`→`EvilMacro`) certified. The extractor now captures CDATA.
- **`certify` canonicalizes redundant sheet-name quoting on the chart data-reference surface
  (over-refusal fix).** A chart series ref written `'Data'!$D$3` (openpyxl/xlq) versus `Data!$D$3`
  (Excel/LibreOffice) is semantically identical, but the chart compare used a raw string match and
  refused the faithful re-serialization. The same `canonicalize_sheet_quotes` already applied to the
  defined-name and CF/DV surfaces is now applied to chart/drawing formula references.
- **`certify`'s numeric cache tolerance no longer leaks into text (`str:`) caches (false-certify
  fix).** The 15-sig-fig numeric tolerance was applied to a cache of ANY type, so two textually
  different string results that both parse to the same number (`"000123"` vs `"123"`, `"1.50"` vs
  `"1.5"`) were vouched as equal — certifying a corrupted zero-padded ID / invoice / account string.
  The tolerance is now gated to numeric (`n:`) caches; a `str:`/`e:`/`b:` cache must match exactly.
- **`certify` vouches a preserved cache at 14 significant figures, absorbing a transcendental
  last-place disagreement (over-refusal fix).** Two independent IEEE-754 implementations of `LOG`/
  `EXP`/trig/`POWER`/financial functions legitimately disagree by ~1 unit in the last place, which
  surfaces at the 15th significant figure — so the round-41 15-figure compare refused a faithful
  transcendental cache a real editor had recomputed. Comparison drops to 14 significant figures; a
  genuine value difference is far above that floor and still refuses.
- **`certify` tolerates the value-neutral `oneCellAnchor`↔`twoCellAnchor` re-encoding of a chart/
  shape placement (over-refusal fix).** The drawing-anchor compare used the full `<col>`/`<row>`
  multiset, so a `oneCellAnchor` (2 tokens) never matched the `twoCellAnchor` (4 tokens) every
  desktop editor re-encodes it to on save — refusing a faithful chart re-serialization. Only the
  `<from>` corner is now compared, so a genuine re-anchor still differs while the encoding change and
  the sub-cell `colOff`/`rowOff` offsets are ignored.
- **`certify`'s volatile-cache skip is now TRANSITIVE (over-refusal fix).** A cell Excel recomputes on
  load — one that transitively depends on a volatile function (`A2=A1` where `A1=NOW()`) — was
  verified against the engine's freshly re-rolled value, which never matches the stored timestamp, so
  a faithful timestamp-helper workbook was refused. The skip set is now computed through the engine's
  dependency graph (poison the volatile source cells, diff the re-evaluation), so a non-volatile
  dependent is skipped too; it stays empty under manual-calc mode (where the stored cache is shown
  verbatim and must be verified).

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
