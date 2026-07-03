## A Format-Aware Enforcement Boundary

The boundary is built on a single discipline: everything an agent may do
unattended is a read, reads are side-effect-free by construction, and the
runtime states plainly what it cannot evaluate rather than returning a
confident answer it cannot stand behind. The design owes its credibility to
that radical simplicity — there is nothing to trust about a read that provably
cannot write, and nothing to be misled by in a value the runtime has flagged
as unverifiable.

The claim staked here is narrow. Format-awareness is not itself new:
data-loss-prevention systems parse file formats to decide what may leave a
network, and schema-aware database sandboxes understand the tables they guard
well enough to reject a write that violates a constraint. What did not exist is
an enforcement boundary for an LLM agent that is aware of its target artifact's
*format fidelity*. To our knowledge, xlq is the first LLM-agent enforcement
boundary that is artifact-format-fidelity-aware. This is not a new enforcement
paradigm — the 2024–2026 runtime-enforcement wave already established that a
deterministic runtime beats prompt-level guidance [Progent; AgentSpec;
IsolateGPT] — but every system in that wave is format-blind: it decides whether
an action is *permitted* without understanding whether the action *corrupts*
the artifact, or whether the runtime can even *evaluate* the artifact it is
guarding. xlq folds the format into the boundary. We did not invent receipts,
transactions, or differential testing; the contribution of this section is the
format-aware layer, and it rests on the fidelity gap of §2 and is made
trustworthy by the coverage accounting of §4 and the oracle of §5.

### Read-only by construction

xlq v0.1 exposes exactly three commands — `inspect`, `diff`, and `calc` — and
none of them can write the target file. This is a structural property, not a
runtime check: the command dispatcher routes each subcommand to a function that
loads the workbook, computes a report, and returns a JSON value, and the tool's
only output sink is stdout. `calc` recomputes every formula and compares the
recomputed value against the value stored on disk, but it reports the
discrepancy rather than persisting it — its module contract reads "Never
writes." The mutating surface (`apply` and `calc --write`) is not disabled in
the read-only tool; it is physically absent — a separate, still-unbuilt
specification (below) — so an agent driving xlq today has no code path that
reaches a file write. The three read commands are exercised as pure
report-returning functions, and every source file carries ≥95% line coverage.

### A privacy-safe structural census

`xlq inspect` produces a *census*: a structural summary that answers "what does
this workbook need from an engine?" without answering "what is in it?" A census
reports sheet dimensions and cell/formula tallies, per-literal counts of the
errors already stored in the file, the call-site frequency of each Excel
function, which OOXML container parts are present, and the file's SHA-256 — and,
by specification, never a cell value, never a fragment of a formula body (no
string literals, constants, or cell references), and never a full filesystem
path (`file.name` is a basename). The invariant is regression-tested against a
sentinel workbook whose cells carry the markers `SECRET_VALUE_XYZ` and
`SECRET_FORMULA_LIT`: the test serializes the census in both normal and redacted
mode and asserts that neither marker — nor the file's parent directory — ever
appears in the output.

Taking the format seriously forces one distinction a format-blind summarizer
would miss: Excel vocabulary versus user data. A called name that is neither a
canonical Excel function nor known to the engine — a VBA/XLL user-defined
function, an add-in call, or a `LAMBDA` invoked through a defined name — is
treated as user data, because such names routinely encode deal terms and client
identities (a call site in our tests is `DealMargin_AcmeCorp`). These names are
reported under a separate `user_defined_calls` member, are redactable, and never
leak into the function tallies; a regression test confirms that
`DealMargin_AcmeCorp` is reported as a user-defined call, is absent from the
function map, and disappears entirely under `--redact`.

The census is also what makes the artifact legible to an agent at a fraction of
the token cost of dumping cells. On a workbook of roughly 100k formulas
(`perf-large.xlsx`), the census was 999 bytes against a 7,239,303-byte naive
full-cell JSON dump — a 7,246.5× reduction (results.json). This number is not
comparable to prompt-side content-compression schemes such as SpreadsheetLLM,
which compress cell *contents* to support understanding and question-answering:
the census discards contents entirely and keeps only the structure a
mutation-safety decision needs, so the two ratios measure different tasks and
are not placed head to head.

### Coverage honesty

The census carries a `coverage` object with a single `reliable` flag and, when
it is false, the exact reason. We name this primitive *coverage honesty*: a
runtime that reports its own per-artifact blind spots instead of presenting an
unqualified value. `reliable` is false whenever the workbook uses a function the
engine does not know, a function the engine recognizes but deliberately refuses
to compute (reported with the exact Excel error literal it yields), a
user-defined callable the engine cannot evaluate, or an engine-level feature
gap. The flag is *per file*: it answers "can this runtime be trusted about *this
workbook*?", not "does the engine support Excel in general" — the catalog-level
accounting is the three-number taxonomy of §4.

Coverage honesty is the first of two operational discriminators — concrete
things a format-aware boundary does that a format-blind one cannot. Consider a
workbook that is mostly evaluable but calls one function the runtime will not
compute — say `WEBSERVICE`, whose true value depends on a live HTTP fetch a
hermetic engine deliberately does not perform. A format-blind guard has two
options, both wrong: recompute and hand back a number (silently substituting
Excel's offline `#VALUE!` for the live value, presented as if authoritative), or
refuse the workbook whole. `xlq calc` does neither. It recomputes every cell it
can, and returns `coverage.reliable: false` together with
`policy_limited_functions: {"WEBSERVICE": "#VALUE!"}` — a typed statement naming
the one function whose stored value it cannot verify. The agent learns precisely
which cells not to trust, and why, and can still rely on the rest. The same
mechanism fires for a UDF-bearing workbook: `reliable` becomes false and the
offending callable is named (redactably). A boundary that does not model the
artifact's function catalog cannot make this distinction; it can only be
uniformly credulous or uniformly refusing.

### The semantic diff and the specified write path

The second discriminator is in `xlq diff`. Because every calculation substrate
rewrites nearly the entire OOXML container on save — openpyxl 3.1.5 re-inlines
shared strings and rewrites parts, and LibreOffice writes caches at lower
precision (§2) — a byte-level diff of two workbooks is useless as an integrity
oracle: it reports that almost everything changed. A task-success metric has the
opposite blind spot: it checks only the answer cells, which a careful save
leaves correct. `xlq diff` instead compares workbooks semantically, cell by
cell, on canonical formula text and raw stored value, and it separates a change
in a *formula* from a change in a formula's *cached result*. When two cells
carry the same formula but different stored values, the change is classified
`cached_value` and counted in its own bucket, apart from genuine edits. This
surfaces exactly the failure both other views miss: openpyxl writes an empty
`<v/>` for every formula cell it saves, blanking the cached result while leaving
the formula intact, so any downstream consumer that trusts caches reads nothing.
Across our corpus this blanking touched 101,961 cached values — 442 on
`branch-consolidation.xlsx` alone (results.json). The `cached_value` change-kind
exists because this failure first hid inside `diff` itself, which once reported
"1 change" while those results silently vanished — a finding we return to in the
discussion. Here the point is only that the classification is what lets a
semantic, recompute-aware diff, and nothing byte-level or task-level, name the
damage.

The write path completes the boundary but is deliberately its least-built part.
It is specified ahead of implementation and ships in v0.2. An agent proposes a
*typed patch* (`set_cell` / `set_formula`, with an explicit JSON-to-Excel type
mapping so the number-versus-text and date-serial traps are handled, never
inferred) carrying the base file hash. A `--dry-run` writes nothing and predicts
the affected cells, any *new* formula error, and before/after values for
caller-named watch cells, degrading its own coverage flag when a volatile or
unsupported function lies in the affected dependency graph. A real apply writes
an immutable revision file, atomically swaps the workbook, and appends a receipt
to an append-only, hash-chained sidecar journal; each receipt binds the
pre-image hash to the post-image hash, and an edit made outside the tool breaks
the chain detectably and is recorded as an `external_edit` receipt that *adopts*
the new hash rather than wedging the file.

None of these ingredients is new, and the paper claims none of them.
Append-only hash-chained receipts descend from forward-secure logging [Nitro,
CCS 2025], Certificate Transparency's Merkle-proof logs [SoK, IEEE S&P 2022],
and W3C PROV / PROV-AGENT provenance; agent-action receipts specifically were
introduced by Notarized Agents. Transactional rollback for agents is likewise
established — ACID-snapshot sandboxing with 100% rollback [Fault-Tolerant
Sandboxing], and semantic and compensating transactions [Cordon; SAFEFLOW]. Our
journal is a domain instantiation, not a cryptographic construction: a
single-writer local sidecar whose threat model is file custody and out-of-band
edit detection rather than receiver attestation, and whose differentiator over a
generic transactional sandbox is that the dry-run's prediction is *semantic* —
affected cells, new formula errors, watched magnitudes — not merely atomic. The
contribution of this section is not receipts or transactions; it is folding the
artifact's format into the boundary so that reads are provably non-destructive,
structure is shared without contents, and the runtime declares — per file — what
it cannot stand behind.
