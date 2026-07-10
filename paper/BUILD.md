# Paper build pipeline

**`paper.src.md` is the ONLY hand-edited file.** Everything else under `paper/` is
generated output, and the system makes the alternative mechanically impossible:

```
artifacts (benchmarks/*.json, formal/*.json, …)
        │  paper/facts.py — derives every artifact-backed number (55 facts),
        │                   asserting against the artifacts as it computes;
        │                   denominators read from artifacts, never hardcoded
        ▼
paper/paper.src.md — prose with {{fact_name}} placeholders
        │  paper/build.py — fail-closed AND atomic:
        │    · strict placeholder grammar: every word-like {{…}} must be an
        │      exact {{known_fact}} — mistyped, spaced, or unknown → ABORT
        │      (quoted dbt Jinja like {{ ref(…) }} is non-word → prose)
        │    · gated phrase (shared list, ws-normalized)        → ABORT
        │    · an UNUSED fact's value appearing in the text
        │      (a hand-written copy of a derived number)        → ABORT
        │    · pandoc error / missing glyph                     → ABORT
        ▼
paper/.staging/{md, build.md, tex, pdf}    ← everything renders HERE first
        │    (os.makedirs without exist_ok atomically claims the staging dir —
        │     a concurrent build aborts instead of racing)
        │  repro/verify_claims.py runs against STAGING (PAPER_DIR env):
        │    · the full independent claim table (separately-written extraction; run
        │      verify_claims.py for the current count — it is not hand-maintained here)
        │    · gated-phrase sweep (same shared list: repro/gated_phrases.json)
        │    · CONSISTENCY GUARD: re-renders paper.src.md and byte-compares
        │      against the generated md/build.md — hand-edits to generated
        │      files cannot pass
        │  any failure → staging discarded, canonical files UNTOUCHED
        ▼
paper-v3.md / -build.md / .tex / .pdf — atomically installed only after green
```

Standalone `python3 repro/verify_claims.py` (any cwd) re-checks the canonical
files, including the consistency guard — so a hand-edit to a generated file fails
the checker even outside a build.

Known limitation: the PDF embeds timestamps, so its bytes differ per rebuild;
PDF provenance rests on the deterministic md/build.md/tex plus the pandoc
invocation recorded in build.py.

Why this exists: three consecutive prose-fix passes each left stale gated text
behind (silent `str.replace` no-ops on hand-maintained mirrors), and round 7
showed the first pipeline draft still allowed hand-edits to generated files,
non-atomic failed builds, spaced/mistyped placeholders, and hand-written copies
of derived numbers. Each of those is now a tested, aborting gate.

Build: `python3 paper/build.py`   Render gates only: `python3 paper/build.py --check`
