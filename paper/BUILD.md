# Paper build pipeline

**`paper.src.md` is the ONLY hand-edited file.** Everything else under `paper/` is
generated output:

```
artifacts (benchmarks/*.json, formal/*.json, …)
        │  paper/facts.py   — derives every artifact-backed number (47 facts),
        │                     asserting against the artifacts as it computes
        ▼
paper/paper.src.md          — prose with {{fact_name}} placeholders
        │  paper/build.py   — fail-closed:
        │      · unresolved placeholder        → build fails
        │      · adversarially-gated phrase    → build fails (ws-normalized sweep)
        │      · pandoc error / missing glyph  → build fails
        ▼
paper-v3.md (banner-marked) → paper-v3-build.md → paper-v3.tex + paper-v3.pdf
        │
        ▼
repro/verify_claims.py      — the INDEPENDENT checker (separately-written
                              extraction of 121 claims + its own gated-phrase
                              sweep); build fails unless it exits 0
```

Why this exists: three consecutive prose-fix passes each left stale gated text
behind (silent `str.replace` no-ops on hand-maintained mirrors). Generated numbers
cannot go stale; unresolved placeholders cannot ship; gated phrases cannot render;
and the generator (facts.py) and checker (verify_claims.py) are independent
derivations from the same artifacts that must agree.

Build: `python3 paper/build.py`   Check only: `python3 paper/build.py --check`
