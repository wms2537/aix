# Live-agent slice = differential testing (honest reframe after adversarial review)

Adversarial review was right: as first written this experiment was **circular**, and
its "0 false certifications" headline was near-tautological. This is the honest version.

## Why the original framing was circular
`live3way_truth.py` builds the agent's file by taking xlq's own structurally-correct
output and overwriting only the target `<f>` bodies with the agent's formulas. So:
- "agent correct" is defined as `norm(agent) == norm(xlq's shifted formula)`, and
- the guard, `xlq certify`, CERTIFIES iff the file equals xlq's transform.

These are the **same predicate**. "0 false certifications" therefore holds *by
construction*, not as an empirical test of guard soundness — and the harness is blind
to the one failure that matters (xlq's transform itself being wrong: when xlq was wrong
and the agent right, it scored the agent as erring AND the guard as safely refusing).
Guard soundness is **not** carried by this experiment; it is carried by `foreign_certify`
(147/147 corrupted foreign edits refused, 0 false certs, engine-free), the edit-path A/B
(86.6% openpyxl silent corruption vs 0% guarded, independent LibreOffice oracle), and
the machine-checked Theorem 1.

## What this slice genuinely establishes (differential testing)
Run as **differential testing of a real agent (Haiku) against xlq** on 20 real
workbooks, insert-row@2, its honest yield is:

1. **It found two real silent-corruption bugs in xlq's tokenizer** — the certifier's
   own trusted base — by surfacing agent-vs-xlq disagreements a human then adjudicated:
   `BIN2DEC`→`BIN3DEC` (function name with digits) and `Sales2020` (defined name). A
   third (`LOG10`) and a whole out-of-grid class (`XFE9`, `A2000000`) fell out of the
   follow-up review. All fixed and now **differential-fuzz-validated** (175 formula×op
   pairs, 0 disagreements — `tokenizer_fuzz.py`).
2. **A measured agent↔xlq agreement rate:** after the fixes, Haiku matched xlq's proven
   shift on **19/20** workbooks; the one divergence (`COMPLEXs`, a dropped `=""`) was a
   genuine agent omission that `xlq certify` refused.

The valuable result is **the method (differential testing hardened the TCB)**, not a
guard-soundness number — that would be circular here.

## Honest scope / what's still open (per the reviewer)
- The interventional "guard catches a live agent's *varied* errors" claim is **not**
  closed by this slice: the agent made ~0 genuine reference-shift errors (Haiku is a
  strong shifter), so the guard's catch is demonstrated on `foreign_certify`
  (openpyxl), not on a live agent's mistakes.
- Remaining to close it: build the agent's file **independently of xlq's bytes** and
  adjudicate `xlq certify`'s CERTIFY/REFUSE verdicts against an **xlq-independent**
  oracle (LibreOffice/Excel), reporting a real TP/FP/FN/TN confusion matrix — plus at
  least one structural op beyond insert-row@2 (the fuzzer now covers delete-rows for
  the tokenizer; the certify confusion matrix does not yet).
