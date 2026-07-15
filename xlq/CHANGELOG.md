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
