# Full-Catalog Semantics Spec: the last 28 functions

Research artifact, 2026-07-02. Purpose: take xlq's engine from 494/522 to
522/522 catalog coverage with Excel-consistent, documented behavior for every
function — genuinely implemented where local computation suffices,
policy-blocked with the EXACT error literal desktop Excel produces where
external execution would be required (which xlq refuses by design; memo §16).

Confidence legend: [P] verbatim from Microsoft primary docs. [S] named
secondary source. [U] undocumented — implementation-defined, marked in code.

## Tier I — genuinely implementable, full semantics (9)

| Function | Semantics | Key edge behavior |
|---|---|---|
| FILTERXML(xml, xpath) | Pure local XPath-1.0-subset over a string (MSXML lineage). Supported constructs: `//`, `/`, `@attr`, `[n]`, `contains()`, `starts-with()`, `text()`, `last()`, `not()` [S]. Multiple matches spill vertically. | Invalid xml → #VALUE! [P]; invalid namespace prefix → #VALUE! [P]; no match / bad xpath → #VALUE! [S]. |
| EUROCONVERT(number, source, target, [full_precision], [triangulation_precision]) | Offline: rates irrevocably fixed by EU law. EXACTLY 14 codes (see table below; CYP/MTL/SKK/EEK/LVL/LTL/HRK are NOT in Excel's table). Legacy→legacy triangulates through EUR; triangulation_precision (int ≥3) rounds the intermediate EUR value to that many DECIMAL PLACES (doc says "significant digits"; its own worked example proves decimal places: FRF→DEM 1, prec 3: 1/6.55957=0.152449→0.152→×1.95583=0.29728616). | Invalid params → #VALUE! [P]. Same source=target → unchanged. Not usable in array formulas [P]. |
| DBCS(text) / JIS(text) | Same built-in, two names (ECMA-376 §18.17.7 stores JIS; accept both). Half-width→full-width: ASCII U+0021–U+007E → U+FF01–U+FF5E; half-width katakana U+FF61–U+FF9F → full-width, composing voiced marks (ｶ+ﾞ → ガ). | Nothing qualifying → text unchanged [P]. Never errors on text. |
| BAHTTEXT(number) | Thai text money algorithm (LibreOffice-verified, full rules in appendix): |value| rounded to 2dp; 6-digit blocks with ล้าน stacking; เอ็ด only when tens>0 in same block; satang 00 → ถ้วน; zero → ศูนย์บาทถ้วน. | BAHTTEXT(0.25) → ยี่สิบห้าสตางค์ (no บาท); negative prefix ลบ after zero-check. |
| PHONETIC(reference) | Concatenate rPh furigana runs from sharedStrings (`<rPh sb eb><t>` per ISO 29500 §18.4.6); uncovered base-text spans contribute their own chars. No runs (or engine dropped them on import) → return the cell's own text UNCHANGED — not empty, not error [S unanimous]. | Nonadjacent range → #N/A [P]; range → upper-left cell [P]. |
| GROUPBY(row_fields, values, function, [field_headers], [total_depth], [sort_order], [filter_array], [field_relationship]) | Pure data function, no pivot object [P]. Lambda-or-eta-reduced aggregation; field_headers 0-3 (auto infers text-then-number); total_depth 0/1/2/-1/-2; signed 1-based sort indices; field_relationship 0 hierarchy / 1 table. | No error literals documented [P]; filter_array length mismatch → #VALUE! [U]. |
| PIVOTBY(row_fields, col_fields, values, function, [field_headers], [row_total_depth], [row_sort_order], [col_total_depth], [col_sort_order], [filter_array], [relative_to]) | Pure; "not directly related to Excel's PivotTable feature" [P]. relative_to is 0–4 (0 col totals, 1 row totals, 2 grand, 3 parent col, 4 parent row). | Same as GROUPBY. |
| PERCENTOF(data_subset, data_all) | ≡ SUM(subset)/SUM(all) [P]; both args required. | #DIV/0! when SUM(all)=0 [U, arithmetic inference]. |
| TRIMRANGE(range, [trim_rows], [trim_cols]) | Pure: trims blank outer rows/cols; enums 0 none / 1 leading / 2 trailing / 3 both (default 3,3). Trim-ref operators A1.:.E10 ≡ (3,3), A1:.E10 ≡ (2,2), A1.:E10 ≡ (1,1). `""` formula results are values → retained. | All-blank input → #REF! [S x2]. |

### EUROCONVERT rate table (per 1 EUR; calc dp / display dp)
BEF 40.3399 (0/0), LUF 40.3399 (0/0), DEM 1.95583 (2/2), ESP 166.386 (0/0),
FRF 6.55957 (2/2), IEP 0.787564 (2/2), ITL 1936.27 (0/0), NLG 2.20371 (2/2),
ATS 13.7603 (2/2), PTE 200.482 (0/2), FIM 5.94573 (2/2), GRD 340.750 (0/2),
SIT 239.640 (2/2), EUR 1 (2/2).

## Tier II — recognized, policy/context-limited with exact Excel literals (17)

These functions are RECOGNIZED (never #NAME? for the name itself) and return
precisely what desktop Excel returns when the external work cannot happen.
xlq's engine performs full argument validation, then the documented refusal.

| Function | Literal returned (and why) |
|---|---|
| WEBSERVICE(url) | #VALUE! — Excel's literal for all failure-to-fetch, including offline [P]. Also #VALUE! for url>2048 chars, non-http(s) scheme — validate these FIRST. NOT #CONNECT!. |
| RTD(progID, server, topic…) | #N/A — "If you haven't installed a real-time data server… #N/A" [P]. |
| STOCKHISTORY(...) | #CONNECT! (offline/service literal [P generic]); arg validation first → #VALUE! [S/U]. |
| DETECTLANGUAGE(text) | #CONNECT! offline [P generic + S]; non-text arg → coercion rules first. |
| TRANSLATE(text, [src], [tgt]) | #CONNECT! offline; invalid language code → #VALUE! [S]. |
| COPILOT(prompt…) | #CONNECT! (timeout/no service) — matches Excel's own table [P]; #VALUE! bad prompt args. |
| IMAGE(source, [alt], [sizing], [h], [w]) | Local #VALUE! validation per doc (sizing 0-3 rules, arg types) FIRST [P]; then #CONNECT! (cannot retrieve) [P]. |
| CALL(...) | #BLOCKED! — the only MS-documented literal for blocked XLM evaluation [P]; worksheet CALL disabled since MS98-018. Never execute. |
| REGISTER.ID(...) | #BLOCKED! — same XLM policy basis [P generic; exact modern literal U]. |
| CUBEVALUE / CUBEMEMBER / CUBESET / CUBESETCOUNT / CUBERANKEDMEMBER / CUBEKPIMEMBER / CUBEMEMBERPROPERTY | #NAME? — NOT name-unknown: "if the connection name is not a valid workbook connection… #NAME?" [P, all 7 pages]; with no OLAP connectivity every connection string is invalid, so #NAME? is the Excel-exact result. Argument-shape validation still applies (#VALUE! for >255-char expressions where documented). CUBESETCOUNT takes a set (which will itself be an error here) — propagate. |
| GETPIVOTDATA(data_field, pivot_table, …) | #REF! — sole documented literal: not-a-pivot-range, invisible field/item, filtered-out data all → #REF! [P]. Engine has no pivot model → the range never contains a pivot → #REF!, Excel-exact. |

## Coverage accounting rule (for docs and probe)

Report three honest numbers:
1. catalog recognized: 522/522 (no #NAME? for any catalog name — except CUBE
   family where #NAME? is itself the documented connection-failure value; the
   probe must therefore distinguish "recognized" by parser acceptance, not by
   #NAME? absence — probe update required: a recognized function must parse
   and dispatch (any error but a parser unknown-name rejection), and the
   probe's #NAME?-based heuristic gets a carve-out list for the 7 CUBE
   functions with a code comment).
2. locally evaluable: Tier-supported count (497 on the master pin plus the
   residual phase — AGGREGATE/ENCODEURL/HYPERLINK — which already included
   PERCENTOF and TRIMRANGE, + the 8 remaining Tier I names = 505 expected).
3. policy/context-limited: the 17 Tier II functions, each with its
   documented literal and a one-line reason.

(Correction, 2026-07-03: the research-time draft said "(19)" over the Tier II
heading and predicted 506/16 here; the Tier II table has always listed 17
functions, and the shipped accounting — 522 recognized = 505 locally
evaluable + 17 policy-limited — is what `benchmarks/coverage.json` and
docs/COVERAGE.md report.)

## AGGREGATE hidden-row note (feeds the residual implementation)

Options 1/3/5/7 skip hidden ROWS (any cause — XLSX stores row/@hidden
uniformly, ECMA-376 §18.3.1.73); hidden COLUMNS are never skipped [P].
Options 0–3 also skip nested SUBTOTAL/AGGREGATE results. [U DISPUTED]:
whether options 0/2/4/6 skip manually-hidden rows (ExcelJet's tested claim
says option 4 still skips them). Implementation: follow the documented
literal reading (0/2/4/6 include hidden rows), mark with a code comment, and
add an oracle case so LibreOffice's interpretation is recorded.

## Source of record

Full research tables with per-claim source URLs preserved in the research
transcript; primary sources are the support.microsoft.com function pages and
the #BLOCKED!/#CONNECT!/#BUSY! error pages, fetched 2026-07-02.
