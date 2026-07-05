"""Z3-verified algebraic laws of the reference-shift algebra σ — the independent
constraints that strengthen the Tier-2 probabilistic certification bound.
Proving these for ALL positions/ranges (not testing) makes them exact lemmas."""
from z3 import *

def prove(name, claim, extra=None):
    s = Solver()
    if extra is not None: s.add(extra)
    s.add(Not(claim))               # look for a counterexample
    r = s.check()
    if r == unsat:
        print(f"PROVED: {name}")
    else:
        print(f"FAILED: {name} -- counterexample: {s.model()}")

# single-line shift under insert of n rows at k (1-based); pos, k >= 1, n >= 1
def ins(p, k, n): return If(p >= k, p + n, p)
# delete of n lines starting at k: below unchanged, at/after band shift up,
# inside band => 'consumed' (we model with a sentinel via a flag)
def del_ok(p, k, n): return Or(p < k, p >= k + n)          # p survives the delete
def dele(p, k, n):  return If(p < k, p, p - n)             # value when it survives

p, k, n = Ints('p k n')
pos_ctx = And(p >= 1, k >= 1, n >= 1)

# LAW 1 — insert@k (n rows) then delete@[k,k+n) is the identity on every original
# position. (Inserted blank band is exactly what delete removes; nothing maps INTO
# the band, so every original position returns home.)
prove("insert(k,n) then delete(k,n) = identity",
      dele(ins(p, k, n), k, n) == p,
      extra=pos_ctx)
# and the inserted position always survives the matching delete
prove("inserted position survives the matching delete",
      del_ok(ins(p, k, n), k, n),
      extra=pos_ctx)

# LAW 2 — insert is strictly monotone (order-preserving) => a range's endpoint
# order is preserved (head <= tail stays head' <= tail'), a precondition for the
# 6-case clamp to be well-formed.
p2 = Int('p2')
prove("insert preserves order (monotone)",
      Implies(p <= p2, ins(p, k, n) <= ins(p2, k, n)),
      extra=And(pos_ctx, p2 >= 1))
prove("delete preserves order on survivors",
      Implies(And(p <= p2, del_ok(p,k,n), del_ok(p2,k,n)),
              dele(p, k, n) <= dele(p2, k, n)),
      extra=And(pos_ctx, p2 >= 1))

# LAW 3 — the 6-case delete clamp for a RANGE [lo,hi] equals the set-theoretic
# truth: the surviving rows of [lo,hi] after deleting [k,k+n), renumbered, form
# [clamp_lo, clamp_hi]. We verify our clamp formula matches the shift of the
# surviving endpoints. (head clamps up to k, tail shifts up by n, etc.)
lo, hi = Ints('lo hi')
rng_ctx = And(lo >= 1, hi >= lo, k >= 1, n >= 1)
# our implementation's clamp (mirrors structural.rs / refshift.rs shift_span):
def clamp_lo(lo, hi, k, n):
    return If(hi < k, lo,
           If(lo >= k+n, lo-n,
           If(And(lo < k, hi >= k+n), lo,          # straddle: head fixed
           If(And(lo >= k, lo < k+n, hi >= k+n), k, # head in band -> k
           If(And(lo < k, hi >= k, hi < k+n), lo,  # tail in band -> head fixed
              0)))))                                 # consumed (sentinel)
def clamp_hi(lo, hi, k, n):
    return If(hi < k, hi,
           If(lo >= k+n, hi-n,
           If(And(lo < k, hi >= k+n), hi-n,
           If(And(lo >= k, lo < k+n, hi >= k+n), hi-n,
           If(And(lo < k, hi >= k, hi < k+n), k-1,
              0)))))
def consumed(lo, hi, k, n): return And(lo >= k, hi < k+n)
# the truth: first surviving row of [lo,hi] shifted, and last surviving row shifted
# survivors = [lo,hi] \ [k,k+n); if any survivor exists, its min/max renumber under `dele`
# min survivor: lo if lo<k else (k+n if k+n<=hi else none)
# We assert: when NOT consumed, clamp_lo/hi equal dele(min_surv)/dele(max_surv).
min_surv = If(Or(lo < k, lo >= k+n), lo, k+n)   # first surviving original row
max_surv = If(Or(hi < k, hi >= k+n), hi, k-1)  # last surviving original row
prove("6-case clamp_lo matches shifted first-survivor",
      Implies(Not(consumed(lo,hi,k,n)), clamp_lo(lo,hi,k,n) == dele(min_surv,k,n)),
      extra=rng_ctx)
prove("6-case clamp_hi matches shifted last-survivor",
      Implies(Not(consumed(lo,hi,k,n)), clamp_hi(lo,hi,k,n) == dele(max_surv,k,n)),
      extra=rng_ctx)
