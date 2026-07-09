#!/usr/bin/env python3
"""Scenario (e) — regression for the quoted-identifier false certification.

Adversarial review found _normalize_text protected only single-quoted literals, so a
case-changed DOUBLE-quoted identifier ("Region" -> "region") normalized identically
and the edit was CERTIFIED — unsound under Postgres-class case-sensitive-identifier
semantics (the certificate must hold under ANY deterministic SQL semantics). After
the fix, the same edit must be REFUSED, end-to-end through the router."""
import sys
sys.path.insert(0, "/home/soh/aix/experiments/dbt")
sys.path.insert(0, "/home/soh/aix/experiments/generality")
from adapter_dbt import normalize_model
from core import Artifact
from router import certify_edit


def artifact(sqls):
    fn, deps, O = {}, {}, {}
    for name, sql in sqls.items():
        skel, d, dyn = normalize_model(sql)
        assert not dyn, f"unexpected dynamic node {name}"
        fn[name], deps[name] = skel, d
    # leaf source
    fn["source:raw.orders"], deps["source:raw.orders"] = "DATA", []
    O["source:raw.orders"] = (("id",), ((1,), (2,)))
    return Artifact(fn=fn, deps=deps, O=O)


ORIG = {
    "stg_orders": 'select "Region", id from {{ source(\'raw\', \'orders\') }}',
    "fct_by_region": 'select "Region", count(*) from {{ ref(\'stg_orders\') }} group by "Region"',
}

if __name__ == "__main__":
    A = artifact(ORIG)
    # FAITHFUL identity edit -> CERTIFIED (control)
    B = artifact(dict(ORIG))
    c1 = certify_edit(A, B, lambda n: n, set())
    print("(control) identical project        ->", c1.status, "(must be CERTIFIED)")
    # THE COUNTEREXAMPLE: same SQL but the quoted identifier's case changed
    EDIT = dict(ORIG)
    EDIT["fct_by_region"] = EDIT["fct_by_region"].replace('"Region"', '"region"')
    C = artifact(EDIT)
    c2 = certify_edit(A, C, lambda n: n, set())
    print("(e) quoted-identifier case change  ->", c2.status, "(must be REFUSED)")
    ok = c1.status == "CERTIFIED" and c2.status == "REFUSED"
    print("RESULT:", "quoted-identifier false certification CLOSED" if ok else "STILL OPEN")
    sys.exit(0 if ok else 1)
