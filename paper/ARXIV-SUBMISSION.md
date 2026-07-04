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
Given the same one-cell edit on the same workbook, a status-quo LLM agent using the standard Python library produced a corrupt, non-reloadable file -- two charts and a pivot table stripped of their relationships, 13 of 50 package parts dropped, only 1 of 50 left byte-identical -- and reported success, having read the raw XML and confirmed its cell had landed; an agent confined to the boundary presented here made the identical edit with charts and pivot byte-identical, 48 of 50 parts untouched, the file reloadable, and a signed receipt. This is the corruption of Anthropic's own shipped spreadsheet skill (issue #22044), reproduced live under a real agent, and prevented. The boundary is xlq, and its core is a surgical write primitive with a fidelity property that holds by construction and is enforced on every write: after an edit, every OOXML part that does not contain a changed cell is byte-identical to the input, because the writer copies those parts verbatim and re-serializes only the sheet parts an operation touches -- and because every apply byte-diffs the un-edited parts before it commits and aborts on any change -- a property that openpyxl, LibreOffice, and even the engine's own whole-file writer all fail, each rewriting the entire container. The write is wrapped in a transactional envelope: a basehash precondition under an advisory lock, a --dry-run that predicts affected cells and new formula errors, a proof-carrying commit that re-loads its own output, verifies every predicted cell landed, and confirms every non-edited part survived byte-identical before it commits, a write-reliability gate that refuses to persist any engine-computed cache whose formula uses a function our own differential oracle found divergent from Excel, an atomic revision swap, and an append-only hash-chained receipt journal. We define the enforcement claim precisely -- an agent whose harness confines its workbook writes to xlq cannot reach the #22044 failure mode, while an agent handed a raw shell can, which is the harness's responsibility and not xlq's -- matching how the 2025–26 agent-enforcement literature scopes its own boundaries. Underneath, the boundary reads a workbook through a privacy-safe structural census (999 B versus a 7.24 MB full-cell dump on a ~100k-formula file), reports its own per-artifact evaluability as three honest numbers (522 of 522 catalog functions recognized, 505 locally evaluable, 17 policy-limited), and rests on a formula engine validated by a documentation-arbitered differential oracle over 1,659 cases; an independent financial cross-check confirms two of the oracle's Treasury-bill verdicts and downgrades a third, which we report as a strength of the method rather than hide. All artifacts are reproducible.
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
