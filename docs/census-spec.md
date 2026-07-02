# Workbook Census Format — Open Spec v0.2

Status: **v0.2, stable.** Produced by `xlq inspect` (see `xlq/src/inspect.rs`;
the implementation and this document must stay in sync).

Changes from v0.1 (additive, plus one privacy correction):

- New `user_defined_calls` member: user-defined callables (VBA/XLL UDFs,
  add-in functions, called LAMBDA defined names) are user data and are now
  reported separately from Excel functions — and are redactable. In v0.1
  they leaked into `functions`/`unsupported_functions`, which redaction did
  not touch; that was a privacy bug, not a feature of the format.
- New optional `coverage.unsupported_features` member (engine-level feature
  gaps such as legacy CSE array formulas).
- `sheets[].rows`/`cols` are defined as `0` for an empty sheet.
- `ooxml_parts.has_comments` covers modern threaded comments
  (`xl/threadedComments/`) as well as legacy `xl/comments*` notes.

This document specifies a JSON format for **privacy-safe workbook
compatibility reporting**: a structural census of an `.xlsx` workbook that
its owner can share with a tool author, support channel, or compatibility
corpus without disclosing any of the workbook's contents. The format is
intended as a small open standard — any tool may produce or consume it, not
only xlq. Requirement words (MUST, MUST NOT, SHOULD, MAY) are used in their
RFC 2119 sense.

## Purpose

A census answers "what does this workbook need from an engine?" — which
functions it calls, which OOXML features it carries, where errors already
exist — without answering "what is in this workbook?". The motivating use:
users whose data-governance policy forbids sharing files can still send a
census, and the recipient learns exactly which Excel functions and container
parts a compatible tool must handle.

## Top-level document

A census is a single JSON object with the following members, all required
unless marked optional.

```json
{
  "xlq": { "version": "0.1.0", "command": "inspect" },
  "file": { "name": "payroll.xlsx", "bytes": 48213, "sha256": "9f2c…" },
  "sheets": [ … ],
  "defined_names": { "count": 2, "names": ["TaxRate", "PayPeriod"] },
  "functions": { "IF": 80, "SUM": 3, "VLOOKUP": 40 },
  "unsupported_functions": [],
  "volatile_functions": [],
  "user_defined_calls": { "count": 0, "call_sites": 0, "names": [] },
  "ooxml_parts": { … },
  "coverage": { "engine": "ironcalc 0.7.1", "reliable": true }
}
```

### `xlq` (object) — producer identification

| Field | Type | Meaning |
|---|---|---|
| `version` | string | Version of the producing tool. |
| `command` | string | Always `"inspect"` for a census. |

Other implementations SHOULD keep the key name `xlq` for compatibility and
put their own tool version in `version`. The census *format* itself is
versioned by this spec (see Versioning policy), not by this field.

### `file` (object)

| Field | Type | Meaning |
|---|---|---|
| `name` | string | **Basename only.** MUST NOT contain any directory component or full path. |
| `bytes` | integer (u64) | Size of the file in bytes. |
| `sha256` | string | SHA-256 of the file's bytes, lowercase hex, 64 characters. |

The hash identifies the exact file revision the census describes; it cannot
be reversed into contents. See Privacy guarantees for the one linkage caveat.

### `sheets` (array of objects)

One entry per sheet, in workbook order.

| Field | Type | Meaning |
|---|---|---|
| `name` | string | Sheet name; `"sheet_N"` under redaction (see Redaction mode). |
| `state` | string | Sheet visibility, using the OOXML `ST_SheetState` values: `"visible"`, `"hidden"`, `"veryHidden"`. |
| `rows` | integer | Number of rows in the populated extent (highest populated row, 1-based); `0` when the sheet has no populated cells. |
| `cols` | integer | Number of columns in the populated extent (highest populated column, 1-based); `0` when the sheet has no populated cells. |
| `cells` | integer | Count of populated cells. |
| `formulas` | integer | Count of populated cells whose content is a formula. |
| `errors` | object | Map from error literal to occurrence count, e.g. `{"#N/A": 4}`. Only literals that occur appear as keys; `{}` when the sheet has no error cells. Recognized literals: `#REF!`, `#VALUE!`, `#DIV/0!`, `#NAME?`, `#N/A`, `#NUM!`, `#NULL!`, `#SPILL!`, `#CALC!`. |

Error counts reflect the **stored** cell values as saved in the file; a
census does not recalculate.

### `defined_names` (object)

| Field | Type | Meaning |
|---|---|---|
| `count` | integer | Number of defined names in the workbook. Always present. |
| `names` | array of strings | The defined names themselves. Present only when redaction is off; MUST be omitted under redaction. |

Only the *names* may appear — never the ranges or formulas they refer to
(`refersTo` targets are workbook content and are excluded unconditionally).

### `functions` (object)

Map from canonical uppercase function name to the number of call sites in
the workbook, e.g. `{"SUM": 120, "VLOOKUP": 40}`. Producers MUST derive this
by tokenizing formulas (a function call is an identifier immediately followed
by `(`), not by regex over formula text — string literals and quoted sheet
names would otherwise false-positive. Call sites are counted in BOTH cell
formulas and defined-name formulas: a function used only inside a defined
name still determines what the workbook needs from an engine.

Only **Excel-vocabulary** function names may appear here. A called name that
matches a workbook defined name, or that is unknown to both the canonical
Excel function catalog and the producing engine, is a *user-defined
callable* (VBA/XLL UDF, add-in function, called LAMBDA name): it is user
data and MUST be reported under `user_defined_calls` instead. Arguments,
literals, and references MUST NOT appear.

### `unsupported_functions` (array of strings)

Excel functions present in the workbook that the producing engine cannot
evaluate. Empty array when the engine covers everything the workbook uses.
This field is the census's core compatibility payload: it tells the
recipient exactly which functions a tool must add for this workbook to be
fully supported. User-defined callables never appear here (see
`user_defined_calls`); their presence is signaled by `coverage.reliable`.

### `volatile_functions` (array of strings)

Volatile functions present in the workbook (Excel semantics): `NOW`, `TODAY`,
`RAND`, `RANDBETWEEN`, `OFFSET`, `INDIRECT`, `CELL`, `INFO` — whether called
from a cell formula or from inside a defined name. Their presence is a
determinism hazard for reproducible recalculation, so consumers planning
verified writes need to know about them up front.

### `user_defined_calls` (object)

Calls to user-defined callables: VBA/XLL UDFs, add-in functions, and
workbook defined names invoked as functions (LAMBDA-style). These names are
**user data** — they routinely encode deal terms, client names, and business
logic — so they get the same treatment as defined names, not the same
treatment as Excel vocabulary.

| Field | Type | Meaning |
|---|---|---|
| `count` | integer | Number of distinct user-defined callables. Always present. |
| `call_sites` | integer | Total number of call sites across the workbook. Always present. |
| `names` | array of strings | The callable names. Present only when redaction is off; MUST be omitted under redaction. |

An engine cannot evaluate user-defined callables, so a non-zero `count`
forces `coverage.reliable: false`.

### `ooxml_parts` (object)

Facts about the `.xlsx` **zip container**, not the engine's model of it.
Producers MUST derive these by scanning the archive's part names directly,
because calculation engines commonly drop parts they do not model — the zip
is the ground truth.

| Field | Type | Derived from |
|---|---|---|
| `has_vba` | boolean | `xl/vbaProject.bin` present. |
| `has_pivot_cache` | boolean | Any part under `xl/pivotCache/`. |
| `has_external_links` | boolean | Any part under `xl/externalLinks/`. |
| `has_charts` | boolean | Any part under `xl/charts/`. |
| `has_comments` | boolean | Any part matching `xl/comments*` (legacy notes) or under `xl/threadedComments/` (modern Excel comments). |
| `part_count` | integer | Total number of parts in the archive. |

Presence booleans only. Part contents, VBA code, chart data, and comment
text MUST NOT appear in a census.

### `coverage` (object)

| Field | Type | Meaning |
|---|---|---|
| `engine` | string | Engine name and version, e.g. `"ironcalc 0.7.1"`. |
| `reliable` | boolean | MUST be `false` when `unsupported_functions` is non-empty, when `user_defined_calls.count` is non-zero, or when `unsupported_features` is non-empty; `true` otherwise. |
| `unsupported_features` | array of strings (optional) | Engine-level feature gaps that prevent faithful evaluation of this workbook, e.g. `"legacy array formulas (CSE)"`. MAY be omitted when empty. |

`reliable: false` means value-level claims by this engine about this workbook
are not trustworthy. Consumers MUST NOT treat a census with
`reliable: false` as evidence that recalculation of the workbook would be
correct.

Workbooks the engine cannot even load in full still deserve a census: a
producer SHOULD degrade gracefully (e.g. xlq normalizes legacy CSE array
formulas to plain formulas for census purposes — the census never
evaluates), record the gap in `unsupported_features`, and set
`reliable: false`, rather than failing to produce a census at all.

## Privacy guarantees

A conforming census MUST NEVER include:

- **Cell values** — no numbers, text, dates, or booleans from any cell.
- **Formula bodies** — no formula text, and no fragments of it: no string
  literals, no cell or range references, no constants. Only canonical
  function *names* extracted by tokenization.
- **Full filesystem paths** — `file.name` is the basename only. Paths leak
  usernames, project names, and directory structure.
- **Defined-name targets** — a name's `refersTo` range or formula.
- **Comment text, chart data, VBA source, pivot data** — only presence
  booleans for the container parts.
- **Any identity metadata** — no OS username, hostname, or document author
  fields.

What the census does reveal, deliberately: sheet names, defined names, and
user-defined callable names (structural metadata, redactable — see below),
dimensions and tallies, function usage frequencies, container-part presence,
and the file's SHA-256.

One honest caveat on the hash: SHA-256 reveals nothing about the file's
contents, but anyone who already possesses a candidate file can hash it and
confirm it is the same file. If even that linkage is unacceptable, the
producer MAY omit or truncate `file.sha256`; consumers MUST tolerate its
absence.

These guarantees are why "send me the census" is a reasonable ask where
"send me the workbook" is policy-forbidden.

## Redaction mode

Structural names can themselves be sensitive — sheet tabs named after
clients, defined names encoding deal terms. Redaction mode (`xlq inspect
--redact`) is for policies where that matters:

- Each sheet's `name` is replaced with `"sheet_N"`, numbered by workbook
  order starting at 1 (`sheet_1`, `sheet_2`, …).
- `defined_names.names` is omitted; `defined_names.count` remains.
- `user_defined_calls.names` is omitted; `count` and `call_sites` remain.
  UDF and called-LAMBDA names are exactly the kind of identifier this mode
  exists to hide.
- Everything else is unchanged, including `file.name` (a basename), the
  hash, tallies, and function names. `functions` contains only Excel
  vocabulary (see `functions` above), not user data, and is never
  redacted — it is the census's purpose.

## Versioning policy

- This is spec **v0.2**. The version applies to the format, independent of
  any producing tool's version string.
- **Minor versions** (0.1 → 0.2) are strictly additive: new optional fields
  may appear; existing fields never change type or meaning. Consumers MUST
  ignore unknown fields.
- **Major versions** may remove fields or change semantics, and will be
  signaled by an explicit format marker field introduced at that time.
- The privacy guarantees above are permanent: no future version of this
  format will add cell values, formula bodies, or full paths. A format that
  includes those is not a census and must not present itself as one.
