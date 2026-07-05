#!/usr/bin/env python3
"""Jupyter notebook adapter — the deliberate CONTRAST that makes the tier
boundary a LAW, not a spreadsheet coincidence.

A notebook has abundant SELF-ORACLE (code cells embed their outputs). But a
code cell's dependencies are Python names resolved in a mutable, Turing-complete
namespace — there is NO static reference graph extractable from syntax without
running the kernel. So EVERY code cell is `dynamic`: the exact tier is honestly
ZERO, and certification must fall to the PROBABILISTIC tier (re-execute under an
independent kernel, diff against the embedded outputs). The boundary lands
exactly where Theorem 1's precondition (static references) predicts."""
import json


def load(path):
    with open(path) as f:
        return json.load(f)


def classify_cells(nb):
    """Structural tier classification of a notebook's cells, engine-free.
    Returns counts: markdown (leaf), code_with_output (self-oracle present ->
    probabilistic-eligible), code_no_output (no self-oracle)."""
    md = code_out = code_noout = 0
    for c in nb.get("cells", []):
        t = c.get("cell_type")
        if t == "markdown" or t == "raw":
            md += 1
        elif t == "code":
            src = "".join(c.get("source", [])).strip()
            if not src:
                md += 1                      # empty code cell ~ no computation
                continue
            has_output = bool(c.get("outputs"))
            if has_output:
                code_out += 1
            else:
                code_noout += 1
    return {"markdown_or_empty": md, "code_with_output": code_out,
            "code_no_output": code_noout}


def exact_tier_available(nb):
    """Is ANY cell exact-tier certifiable engine-free? A code cell's deps are
    data/namespace-computed -> never. (Markdown carries no computation.) So the
    answer is structurally False for every notebook — the honest zero."""
    for c in nb.get("cells", []):
        if c.get("cell_type") == "code" and "".join(c.get("source", [])).strip():
            return False   # a real code cell exists -> implicit deps -> no exact tier
    return False
