# arXiv Submission Guide

Everything needed to submit *An Enforcement Boundary for LLM Agents Operating
on Spreadsheet Artifacts* to arXiv. What only you can do is marked **[YOU]**.

## What only you can do
- **[YOU] arXiv account** — https://arxiv.org (log in / register with your
  institutional or personal email).
- **[YOU] Endorsement** — first-time submitters to **cs.SE** may need an
  endorsement from an existing arXiv author in that category. If prompted,
  the endorsement page explains the process (ask a colleague who has posted
  to cs.SE, or submit and wait for the endorsement flow). Established
  submitters skip this.
- **[YOU] Upload + license + submit** — upload the source (see below), pick
  the license (recommend **CC BY 4.0** for maximum reach, or arXiv's
  non-exclusive default), confirm the metadata, and click Submit. There is a
  moderation hold (usually <1 business day) before it goes live.

## Recommended metadata
- **Primary category:** `cs.SE` (Software Engineering) — it is a systems/SE
  tools-and-measurement paper.
- **Cross-list:** `cs.AI` (LLM agents) and `cs.HC` (the Pista comparison /
  agent-oversight framing). Two cross-lists is normal.
- **Title:** An Enforcement Boundary for LLM Agents Operating on Spreadsheet
  Artifacts
- **Authors:** [YOU — fill in name(s) + affiliation; the repo currently uses
  "the xlq authors" as a placeholder]
- **Comments field (optional but recommended):** "Artifact:
  https://github.com/wms2537/aix — reproducible (xlq CLI, AXLE-bench,
  differential oracle, fixtures)."

## Abstract (paste into the arXiv abstract box — plain text, no markdown)
When an LLM agent edits a spreadsheet, its decisions do not sit in a separate
log: they become the cells the file's owner is accountable for. Yet the
substrate agents use today silently corrupts the file: a single re-save
through the standard Python library strips 101,961 cached formula values from
a five-workbook corpus, and no existing agent benchmark measures the damage.
We present xlq, a format-aware enforcement boundary that treats the workbook
as an artifact to be preserved rather than a string to be rewritten. xlq is
read-only by construction: it exposes a privacy-safe structural census so the
agent reads structure, not content (up to 7,246.5x smaller than a full-cell
dump); it reports coverage as three honest numbers rather than one -- 522
functions recognized, 505 locally evaluable, 17 returning the exact error
literal Excel returns. Enforcement today is structural: xlq v0.1 exposes no
write path at all, so an agent driving it cannot corrupt the file through it.
The mediated write layer that would let an agent edit safely -- typed patches,
dry-run semantic prediction, hash-chained receipts -- is specified and ships
in v0.2, not measured here. To earn trust in the engine rather than assume it,
we built a differential oracle over 1,659 cases across 492 functions, using
LibreOffice as a reference and Microsoft's documentation as arbiter; it
validated the engine and surfaced real bugs in both it and LibreOffice. We
package the evaluation as AXLE-bench, whose fidelity axis measures the
dimension current benchmarks omit. All artifacts are reproducible.

## Source to upload — two options
arXiv accepts either. LaTeX source is preferred (arXiv rebuilds the PDF and
it renders natively); PDF-only is accepted for papers not written in TeX.

**Option A — LaTeX (preferred):** `paper/paper-v2.tex` (generated; see
paper/README if present). Upload the .tex plus any `.bbl`/figures. If we could
not generate clean LaTeX in this environment, use Option B and convert later.

**Option B — PDF-only (fastest):** upload `paper/paper-v2.pdf`. To make the PDF:
- Open `paper/paper-v2.md` in any browser → Print → Save as PDF (30 seconds,
  works anywhere; the HTML carries the same publication CSS), OR
- run `make-pdf` where a Chromium/browse daemon is available:
  `pdf generate --cover --toc paper/paper.md paper/paper-v2.pdf`.

## Pre-submission checklist
- [ ] Author name(s) + affiliation filled in (replace "the xlq authors").
- [ ] Abstract pasted (plain text version above; no markdown/backticks).
- [ ] PDF or .tex uploaded and previews correctly on arXiv.
- [ ] Categories: cs.SE primary + cs.AI, cs.HC cross-list.
- [ ] License chosen (CC BY 4.0 recommended).
- [ ] Artifact URL in the Comments field.
- [ ] (Optional) GitHub repo made public first so the link resolves.
