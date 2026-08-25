# Live-agent study v4

Date: 2026-08-25. Protocol: [V4-PROTOCOL.md](V4-PROTOCOL.md), including
Amendment A.

## Result

Both arms used the same frontier model and the frozen 100-task set. Three tasks
failed all five model attempts with malformed JSON in each arm, leaving 97 tasks
scored per arm. The same 97 tasks form the paired comparison.

| Metric | Careful | Hasty |
|---|---:|---:|
| Tasks scored | 97 | 97 |
| Agent errors | 4 | 3 |
| Unguarded corrupt shipments | 4 | 3 |
| Guarded false certifications | **0** | **0** |
| Guarded incorrect saves | 4 | 3 |
| Guarded refusals of correct work | 85 | 86 |

The careful prompt did not reduce errors relative to hasty on this run. All seven
incorrect artifacts were refused before shipment and no certified artifact contained
a truth-visible corruption.

## Per-operation results

### Careful

| Operation | Tasks | Errors | Incorrect saves | Correct refusals |
|---|---:|---:|---:|---:|
| Insert rows | 39 | 2 | 2 | 33 |
| Delete rows | 20 | 1 | 1 | 18 |
| Insert cols | 19 | 0 | 0 | 17 |
| Delete cols | 19 | 1 | 1 | 17 |

### Hasty

| Operation | Tasks | Errors | Incorrect saves | Correct refusals |
|---|---:|---:|---:|---:|
| Insert rows | 39 | 1 | 1 | 34 |
| Delete rows | 20 | 0 | 0 | 19 |
| Insert cols | 19 | 1 | 1 | 16 |
| Delete cols | 19 | 1 | 1 | 17 |

The discordant counts were four careful-only errors and three hasty-only errors.
With only seven discordances, this is directionally uninformative rather than
evidence of parity.

## API accounting

The validation call plus scored arms consumed 238 successful calls and 236 retry
attempts: 97 scored answers plus 22 failed attempts for careful, 97 scored answers
plus 20 failed attempts for hasty, and one successful validation call. Careful
returned 119,722 total tokens; hasty returned 95,203. Reported provider cost was
zero in every recorded response. Four primary tasks remained unanswered: the same
three malformed-response failures in both arms plus one additional hasty
formula-error task.

## Honest limits

1. **One model and one temperature.** This is a within-model prompt-condition
   probe, not a claim about frontier models generally.
2. **Small discordance.** Seven paired discordances cannot estimate a reliable
   effect size or support a conventional superiority claim.
3. **Strict text truth.** Semantically equivalent rewrites count as wrong, as in
   v3.
4. **Guard-compatible selection.** Refusal rates are not population-wide rates;
   constructs outside the guard grammar were excluded by protocol.
5. **Truth-grammar coverage bounds the false-certification claim.** Zero means
   zero on cells the reference grammar can rule on.
6. **Malformed JSON is an endpoint behavior.** Failed tasks are excluded from the
   denominator under Amendment A; they are not counted as reference-shift errors.

## Reproduction

```bash
python3 benchmarks/agent_study/score_v3.py \
  benchmarks/agent_study/results_v4_live_careful_raw.json \
  benchmarks/agent_study/results_v4_live_careful.json \
  benchmarks/agent_study/tasks_v3.json

python3 benchmarks/agent_study/score_v3.py \
  benchmarks/agent_study/results_v4_live_hasty_raw.json \
  benchmarks/agent_study/results_v4_live_hasty.json \
  benchmarks/agent_study/tasks_v3.json
```
