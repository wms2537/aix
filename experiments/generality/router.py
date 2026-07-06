#!/usr/bin/env python3
"""The certify-or-refuse ROUTER — `xlq certify` in miniature.

A real agent edit = a structural transform (the scaffold) + value fills. The
router verifies the edit MATCHES its declaration and factors it:

  1. SCAFFOLD (exact tier, engine-free): every node EXCEPT the declared value
     fills must be the σ-relabeling of the original (same op, deps = σ(deps)).
     Certified by Theorem 1 (formal/SelfOracle.lean).
  2. UNDECLARED CHANGES: any node outside the declared fills that does NOT match
     the σ-relabeling is an unaccounted change — a botch or an undeclared edit.
     The router REFUSES (never silently wrong).
  3. AUDIT SURFACE (probabilistic tier): the declared fills PLUS their downstream
     cone (nodes that transitively depend on a fill). By eval_local /
     audit_surface_bound, EVERYTHING ELSE keeps its certified value. So a
     consumer re-checks only these N cells, not the whole artifact.

Output: CERTIFIED-with-audit-surface, or REFUSED. This collapses the human audit
surface from 'did this corrupt anything anywhere?' to 'are these N values right?'.
"""
from dataclasses import dataclass, field
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
from core import Artifact


@dataclass
class Certificate:
    status: str                      # CERTIFIED | REFUSED
    reason: str = ""
    total_nodes: int = 0
    scaffold_certified: int = 0
    declared_fills: int = 0
    audit_surface: list = field(default_factory=list)   # cells a consumer must check
    undeclared_changes: list = field(default_factory=list)
    collapse_ratio: float = 0.0      # fraction of the artifact certified untouched


def _downstream(edited: Artifact, seeds):
    """All nodes that transitively DEPEND on a seed node (a value fill), in the
    edited dependency graph. These are the cells whose value may have changed."""
    dependents = {}
    for n, ds in edited.deps.items():
        for d in ds:
            dependents.setdefault(d, []).append(n)
    seen, stack = set(seeds), list(seeds)
    while stack:
        x = stack.pop()
        for u in dependents.get(x, []):
            if u not in seen:
                seen.add(u); stack.append(u)
    return seen


def certify_edit(orig: Artifact, edited: Artifact, sigma, declared_fills: set) -> Certificate:
    """declared_fills: the set of EDITED nodes the agent declares it intentionally
    changed (value edits). Everything else must be a faithful σ-relabeling."""
    cert = Certificate("CERTIFIED", total_nodes=len(edited.fn))
    image = {sigma(n): n for n in orig.fn}
    # the declared fills' downstream cone: nodes whose VALUE legitimately changes
    # because they depend on a fill. Structure is preserved everywhere; values
    # may change only inside this cone.
    cone = _downstream(edited, declared_fills)
    # 1+2. verify the scaffold; collect undeclared changes
    for n in orig.fn:
        sn = sigma(n)
        if sn in declared_fills:
            continue                                  # intentional value fill
        want_deps = [sigma(m) for m in orig.deps.get(n, [])]
        # STRUCTURE (fn/deps — Theorem 1) must match the σ-relabeling at EVERY
        # non-declared node: a value fill changes values, never the graph.
        struct_ok = (edited.fn.get(sn) == orig.fn[n] and edited.deps.get(sn) == want_deps)
        # VALUE (self-oracle) must be preserved OUTSIDE the fill cone. This is
        # load-bearing for LEAF cells (a graph-iso says nothing about a leaf's
        # value) and is the check that catches an undeclared edit to a base cell.
        value_ok = (sn in cone or n not in orig.O or sn not in edited.O
                    or edited.O[sn] == orig.O[n])
        if struct_ok and value_ok:
            cert.scaffold_certified += 1
        else:
            cert.undeclared_changes.append(sn)
    # injected nodes (not in σ-image and not declared) are undeclared too
    for e in edited.fn:
        if e not in image and e not in declared_fills:
            cert.undeclared_changes.append(e)
    cert.declared_fills = len(declared_fills)

    if cert.undeclared_changes:
        cert.status = "REFUSED"
        cert.reason = (f"{len(cert.undeclared_changes)} node(s) changed outside the "
                       f"declared edit — unaccounted (botch or undeclared edit); "
                       f"e.g. {cert.undeclared_changes[:3]}")
        return cert
    # 3. audit surface = declared fills + their downstream cone
    cone = _downstream(edited, declared_fills)
    cert.audit_surface = sorted(cone, key=lambda x: str(x))
    certified_untouched = cert.total_nodes - len(cone)
    cert.collapse_ratio = round(certified_untouched / cert.total_nodes, 3) if cert.total_nodes else 0.0
    cert.reason = (f"structural scaffold certified bit-exact on "
                   f"{cert.scaffold_certified} nodes; the only values to re-check "
                   f"are the {len(cone)} in the fill cone — "
                   f"{int(cert.collapse_ratio*100)}% of the artifact is certified untouched")
    return cert
