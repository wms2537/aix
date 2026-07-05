#!/usr/bin/env python3
"""Format-parametric certifier core — the generality claim, made concrete.

An artifact is reduced to the exact triple the Lean `Computation` carries and
Theorem 1 (formal/SelfOracle.lean) reasons about:

    fn   : Node -> op          # the NAME-FREE operation (deps replaced by slots)
    deps : Node -> [Node]      # ordered dependency nodes (the edges)
    O    : Node -> value       # the embedded self-oracle (engine's stored output)

`certify(orig, edited, sigma)` checks the two hypotheses of `eval_iso_invariant`
— function-preservation (`hfn`) and edge-preservation-under-sigma (`hdeps`). If
both hold, the machine-checked theorem guarantees every computed value is
preserved under ANY deterministic semantics: the edit is CERTIFIED value-faithful
WITHOUT invoking the defining engine. Otherwise REFUSED.

This SAME core certifies spreadsheets, SQLite, and notebooks. That shared core IS
the generality claim: the exact tier fires on formats with static references
(spreadsheet, SQLite) and honestly abstains where dependencies are data-computed
(Jupyter). No engine is ever invoked here."""
from dataclasses import dataclass, field
from typing import Callable, Hashable

Node = Hashable


@dataclass
class Artifact:
    """The format-independent triple. `dynamic` marks nodes whose dependency set
    is data-computed (INDIRECT/OFFSET/dynamic-SQL/implicit-namespace) — these
    cannot be graph-iso-checked engine-free and are excluded from the exact tier."""
    fn: dict          # Node -> str (name-free op) ; leaves map to a sentinel
    deps: dict        # Node -> list[Node]
    O: dict           # Node -> value (self-oracle); may be partial
    dynamic: set = field(default_factory=set)   # nodes with data-computed deps


@dataclass
class Verdict:
    status: str                 # CERTIFIED | REFUSED | PROBABILISTIC
    reason: str = ""
    checked: int = 0
    fn_ok: int = 0
    edge_ok: int = 0
    oracle_consistent: int = 0  # nodes where stored O matched across the iso (theorem sanity)
    failures: list = field(default_factory=list)


def certify(orig: Artifact, edited: Artifact, sigma: Callable[[Node], Node]) -> Verdict:
    """Engine-free exact-tier certification of a relabeling edit.
    Certifies iff, for every node n, the edited artifact at sigma(n) has the SAME
    operation and its dependencies are exactly sigma applied to n's dependencies.
    """
    # any dynamic-dependency node in the affected set forces the probabilistic tier
    dyn = [n for n in orig.fn if n in orig.dynamic]
    if dyn:
        return Verdict("PROBABILISTIC",
                       reason=f"{len(dyn)} node(s) have data-computed dependencies "
                              f"(e.g. {dyn[:2]}); exact tier unavailable, route to probabilistic")

    v = Verdict("CERTIFIED")
    for n in orig.fn:
        sn = sigma(n)
        v.checked += 1
        # (hfn) same operation
        if edited.fn.get(sn) != orig.fn[n]:
            v.failures.append(("fn", n, orig.fn[n], edited.fn.get(sn)))
            continue
        v.fn_ok += 1
        # (hdeps) dependencies are exactly the sigma-image of n's dependencies
        want = [sigma(m) for m in orig.deps.get(n, [])]
        if edited.deps.get(sn) != want:
            v.failures.append(("deps", n, want, edited.deps.get(sn)))
            continue
        v.edge_ok += 1
        # theorem sanity: the stored self-oracle value must then be preserved
        if n in orig.O and sn in edited.O and orig.O[n] == edited.O[sn]:
            v.oracle_consistent += 1
    if v.failures:
        v.status = "REFUSED"
        v.reason = f"{len(v.failures)} node(s) break the isomorphism (fn or edge) — " \
                   f"not a faithful relabeling; e.g. {v.failures[0][:2]}"
    else:
        v.reason = f"graph-iso holds on all {v.checked} nodes; by Theorem 1 every " \
                   f"computed value is preserved under any semantics (engine-free)"
    return v


# ---- name-free operation extraction (shared by adapters) ----
import re
_IDENT = re.compile(r"[A-Za-z_][A-Za-z_0-9]*")


def normalize_expr(expr: str, is_ref):
    """Turn an expression string into (name_free_op, ordered_unique_deps).
    `is_ref(token)` decides whether an identifier is a reference (a dependency)
    vs a function name / keyword. References are replaced by positional slots
    (#0, #1, ...) so the op is name-independent — a rename that updates all
    references leaves the op identical, which is exactly what fn-preservation
    must see."""
    deps = []
    def repl(m):
        tok = m.group(0)
        if not is_ref(tok):
            return tok                      # function/keyword: keep literally
        if tok not in deps:
            deps.append(tok)
        return f"#{deps.index(tok)}"
    op = _IDENT.sub(repl, expr)
    op = re.sub(r"\s+", "", op)             # canonical whitespace
    return op, deps
