#!/usr/bin/env python3
"""dbt-domain demo: certify model refactors ENGINE-FREE with the unchanged
format-parametric router (experiments/generality/router.py).

A realistic mini project (3 sources + 7 models: staging -> intermediate ->
marts -> report) and four agent-edit scenarios:

  a. FAITHFUL refactor  rename stg_orders -> staging_orders, every ref updated
                        -> CERTIFIED (no SQL executed for the check; O comes
                           from the ORIGINAL materialization, the self-oracle)
  b. BOTCHED refactor 1 rename, but one downstream ref left dangling -> REFUSED
  c. BOTCHED refactor 2 rename + silent logic change (SUM -> AVG) -> REFUSED,
                        then CONFIRMED against ground truth by independently
                        re-materializing (the values really differ)
  d. COMPOSITION        rename + INTENTIONAL logic change declared as a fill
                        -> scaffold CERTIFIED, audit surface = that model +
                           its downstream cone; collapse ratio reported, and
                           loop-checked (outside-cone values really preserved)

The certifier NEVER executes the edited projects in (a),(b),(c),(d); duckdb
runs only to build the original self-oracle and for the falsification loops.
"""
import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "..", "generality"))

from adapter_dbt import build_artifact, rename_sigma, source_node
from router import certify_edit

W = os.environ.get("DBT_WORK") or tempfile.mkdtemp(prefix="dbt_demo_")

# ---------------------------------------------------------------- the project
SOURCES = {
    source_node("jaffle", "raw_orders"): (
        ["order_id", "customer_id", "order_date", "status"],
        [(1, 101, "2026-01-05", "completed"),
         (2, 102, "2026-01-07", "completed"),
         (3, 101, "2026-01-19", "completed"),
         (4, 103, "2026-02-02", "pending"),
         (5, 104, "2026-02-10", "completed"),
         (6, 105, "2026-02-14", "returned"),
         (7, 102, "2026-03-01", "completed"),
         (8, 103, "2026-03-03", "completed"),
         (9, 105, "2026-03-15", "pending"),
         (10, 104, "2026-03-20", "completed")]),
    source_node("jaffle", "raw_customers"): (
        ["customer_id", "name", "region"],
        [(101, "Alice", "north"), (102, "Bob", "south"), (103, "Cara", "north"),
         (104, "Dan", "east"), (105, "Eve", "south")]),
    source_node("jaffle", "raw_payments"): (
        ["payment_id", "order_id", "method", "amount"],
        [(1, 1, "card", 120.0), (2, 2, "card", 80.0), (3, 3, "cash", 45.5),
         (4, 4, "card", 60.0), (5, 5, "card", 200.0), (6, 6, "cash", 35.0),
         (7, 7, "card", 150.0), (8, 8, "cash", 95.0), (9, 9, "card", 70.0),
         (10, 10, "card", 110.0), (11, 1, "cash", 30.0),
         (12, 5, "voucher", None)]),
}

MODELS = {
    "stg_orders": """\
SELECT
    order_id,
    customer_id,
    CAST(order_date AS DATE) AS order_date,
    status
FROM {{ source('jaffle', 'raw_orders') }}
""",
    "stg_customers": """\
SELECT customer_id, name, region
FROM {{ source('jaffle', 'raw_customers') }}
""",
    "stg_payments": """\
SELECT payment_id, order_id, method, amount
FROM {{ source('jaffle', 'raw_payments') }}
WHERE amount IS NOT NULL
""",
    "int_orders_enriched": """\
SELECT
    o.order_id,
    o.customer_id,
    o.order_date,
    o.status,
    c.region,
    COALESCE(p.paid, 0) AS paid
FROM {{ ref('stg_orders') }} AS o
JOIN {{ ref('stg_customers') }} AS c ON o.customer_id = c.customer_id
LEFT JOIN (
    SELECT order_id, SUM(amount) AS paid
    FROM {{ ref('stg_payments') }}
    GROUP BY order_id
) AS p ON o.order_id = p.order_id
""",
    "fct_revenue": """\
SELECT
    region,
    SUM(paid) AS revenue,
    COUNT(*) AS orders
FROM {{ ref('int_orders_enriched') }}
WHERE status = 'completed'
GROUP BY region
""",
    "dim_customers": """\
SELECT
    c.customer_id,
    c.name,
    c.region,
    COUNT(o.order_id) AS lifetime_orders,
    COALESCE(SUM(o.paid), 0) AS lifetime_value
FROM {{ ref('stg_customers') }} AS c
LEFT JOIN {{ ref('int_orders_enriched') }} AS o ON c.customer_id = o.customer_id
GROUP BY c.customer_id, c.name, c.region
""",
    "rpt_kpis": """\
SELECT
    (SELECT SUM(revenue) FROM {{ ref('fct_revenue') }}) AS total_revenue,
    (SELECT COUNT(*) FROM {{ ref('dim_customers') }} WHERE lifetime_orders > 0)
        AS active_customers
""",
}


# ---------------------------------------------------------------- helpers
def write_project(dirname, models):
    d = os.path.join(W, dirname)
    mdir = os.path.join(d, "models")
    os.makedirs(mdir, exist_ok=True)
    for name, sql in models.items():
        with open(os.path.join(mdir, name + ".sql"), "w") as f:
            f.write(sql)
    return d


def rename_everywhere(models, old, new):
    """The FAITHFUL rename: file renamed AND every ref('old') updated."""
    out = {}
    for name, sql in models.items():
        out[new if name == old else name] = sql.replace(
            f"ref('{old}')", f"ref('{new}')")
    return out


def assert_engine_free(art, label):
    """Prove the certify-path artifact carries no materialized model values."""
    leaked = [n for n in art.fn if art.fn[n] != "DATA" and n in art.O]
    assert not leaked, f"{label}: engine-free build leaked model O for {leaked}"


# ---------------------------------------------------------------- run
if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    results, ok = {}, True

    orig_dir = write_project("orig", MODELS)
    A_orig = build_artifact(orig_dir, SOURCES, materialize_models=True)  # self-oracle, ONCE
    assert not A_orig.dynamic, "unexpected unmodeled jinja in fixture"
    n_models = sum(1 for n in A_orig.fn if A_orig.fn[n] != "DATA")
    print(f"project: {n_models} models + {len(SOURCES)} sources = "
          f"{len(A_orig.fn)} nodes; self-oracle materialized once from the ORIGINAL\n")

    SIGMA = rename_sigma({"stg_orders": "staging_orders"})

    # --- (a) FAITHFUL rename ------------------------------------------------
    m_a = rename_everywhere(MODELS, "stg_orders", "staging_orders")
    A_a = build_artifact(write_project("a_faithful", m_a), SOURCES,
                         materialize_models=False)          # ENGINE-FREE
    assert_engine_free(A_a, "a")
    cert_a = certify_edit(A_orig, A_a, SIGMA, declared_fills=set())
    good_a = cert_a.status == "CERTIFIED" and cert_a.audit_surface == []
    ok &= good_a
    print(f"(a) faithful rename        -> {cert_a.status}: {cert_a.reason}")

    # --- (b) BOTCHED: dangling ref ------------------------------------------
    m_b = dict(MODELS)
    m_b["staging_orders"] = m_b.pop("stg_orders")            # file renamed...
    #    ...but no ref() updated: int_orders_enriched still points at stg_orders
    A_b = build_artifact(write_project("b_dangling", m_b), SOURCES,
                         materialize_models=False)          # ENGINE-FREE
    assert_engine_free(A_b, "b")
    cert_b = certify_edit(A_orig, A_b, SIGMA, declared_fills=set())
    good_b = (cert_b.status == "REFUSED"
              and "int_orders_enriched" in cert_b.undeclared_changes)
    ok &= good_b
    print(f"(b) rename, dangling ref   -> {cert_b.status}: {cert_b.reason}")

    # --- (c) BOTCHED: rename + silent SUM -> AVG ----------------------------
    m_c = rename_everywhere(MODELS, "stg_orders", "staging_orders")
    m_c["fct_revenue"] = m_c["fct_revenue"].replace("SUM(paid)", "AVG(paid)")
    c_dir = write_project("c_silent_logic", m_c)
    A_c = build_artifact(c_dir, SOURCES, materialize_models=False)  # ENGINE-FREE
    assert_engine_free(A_c, "c")
    cert_c = certify_edit(A_orig, A_c, SIGMA, declared_fills=set())
    # ground truth: independently re-materialize the edited project (this is
    # the falsification loop, NOT part of the certificate)
    A_c_truth = build_artifact(c_dir, SOURCES, materialize_models=True)
    c_truth_differs = A_c_truth.O["fct_revenue"] != A_orig.O["fct_revenue"]
    good_c = (cert_c.status == "REFUSED"
              and "fct_revenue" in cert_c.undeclared_changes
              and c_truth_differs)
    ok &= good_c
    print(f"(c) rename + silent AVG    -> {cert_c.status}: {cert_c.reason}")
    print(f"    ground truth (independent duckdb run): fct_revenue values "
          f"differ = {c_truth_differs}  "
          f"[refusal {'CORRECT' if c_truth_differs else 'WRONG'}]")

    # --- (d) COMPOSITION: rename + DECLARED logic change --------------------
    m_d = rename_everywhere(MODELS, "stg_orders", "staging_orders")
    m_d["fct_revenue"] = m_d["fct_revenue"].replace(
        "WHERE status = 'completed'",
        "WHERE status IN ('completed', 'pending')")          # intentional redefinition
    d_dir = write_project("d_declared", m_d)
    A_d = build_artifact(d_dir, SOURCES, materialize_models=False)  # ENGINE-FREE
    assert_engine_free(A_d, "d")
    cert_d = certify_edit(A_orig, A_d, SIGMA, declared_fills={"fct_revenue"})
    expected_cone = ["fct_revenue", "rpt_kpis"]
    # falsification loop for the audit-surface claim: re-materialize the edited
    # project and check values OUTSIDE the cone are preserved, INSIDE differ
    A_d_truth = build_artifact(d_dir, SOURCES, materialize_models=True)
    outside_preserved = all(
        A_d_truth.O[SIGMA(n)] == A_orig.O[n]
        for n in A_orig.fn if SIGMA(n) not in set(cert_d.audit_surface))
    inside_differs = A_d_truth.O["fct_revenue"] != A_orig.O["fct_revenue"]
    good_d = (cert_d.status == "CERTIFIED"
              and cert_d.audit_surface == expected_cone
              and outside_preserved and inside_differs)
    ok &= good_d
    print(f"(d) rename + declared fill -> {cert_d.status}: {cert_d.reason}")
    print(f"    audit surface = {cert_d.audit_surface} (declared model + downstream cone)")
    print(f"    collapse ratio = {cert_d.collapse_ratio} "
          f"({cert_d.total_nodes - len(cert_d.audit_surface)}/{cert_d.total_nodes} "
          f"nodes certified untouched)")
    print(f"    ground truth: outside-cone values preserved = {outside_preserved}, "
          f"fill value actually changed = {inside_differs}")

    # ---------------------------------------------------------------- report
    def as_dict(cert):
        return {"status": cert.status, "reason": cert.reason,
                "total_nodes": cert.total_nodes,
                "scaffold_certified": cert.scaffold_certified,
                "declared_fills": cert.declared_fills,
                "audit_surface": [str(x) for x in cert.audit_surface],
                "undeclared_changes": [str(x) for x in cert.undeclared_changes],
                "collapse_ratio": cert.collapse_ratio}

    results = {
        "project": {"models": n_models, "sources": len(SOURCES),
                    "total_nodes": len(A_orig.fn),
                    "engine_free_certify_path": True,
                    "oracle": "single materialization of the ORIGINAL project (duckdb)"},
        "a_faithful_rename": {**as_dict(cert_a), "expected": "CERTIFIED",
                              "pass": bool(good_a)},
        "b_rename_dangling_ref": {**as_dict(cert_b), "expected": "REFUSED",
                                  "pass": bool(good_b)},
        "c_rename_silent_logic_change": {**as_dict(cert_c), "expected": "REFUSED",
                                         "ground_truth_values_differ": bool(c_truth_differs),
                                         "pass": bool(good_c)},
        "d_rename_plus_declared_fill": {**as_dict(cert_d), "expected": "CERTIFIED",
                                        "expected_audit_surface": expected_cone,
                                        "ground_truth_outside_cone_preserved": bool(outside_preserved),
                                        "ground_truth_fill_value_changed": bool(inside_differs),
                                        "pass": bool(good_d)},
        "all_pass": bool(ok),
    }
    out = os.path.join(HERE, "dbt_results.json")
    with open(out, "w") as f:
        json.dump(results, f, indent=2)
    print(f"\nresults -> {out}")
    print("RESULT:", "all four scenarios match expected verdicts; certificates "
          "agree with engine ground truth" if ok else "MISMATCH — investigate")
    sys.exit(0 if ok else 1)
