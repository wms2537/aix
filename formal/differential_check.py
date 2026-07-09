#!/usr/bin/env python3
"""Differential test: the Lean-proven decision procedure (Checker.check) vs the
running implementation (experiments/generality/router.certify_edit).

The claim in the paper is that router.certify_edit IMPLEMENTS the checked premise
`check` (plus purely-syntactic containment conditions). This battery generates
random syntactic computations, a relabeling σ, and randomized botches (skeleton
change / dep reorder / dep retarget / dropped dep), runs BOTH deciders on each
case, and requires verdict agreement. The Lean side runs the actual proven
`check` via #eval in one `lean` invocation.

Scope note (honest): router.certify_edit additionally checks self-oracle values
on leaves and containment (injected nodes) — cases here are constructed so those
extra checks are neutral (O transported faithfully, no injections), isolating
the shared graph premise. Verdict agreement on this battery is evidence of
implementation fidelity, not a proof of it."""
import os, random, subprocess, sys

sys.path.insert(0, "/home/soh/aix/experiments/generality")
from core import Artifact
from router import certify_edit

LEAN_HEADER = open("/home/soh/aix/formal/Checker.lean").read().split("/-! ## Executable demos")[0]
WORK = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/diffcheck"
N_CASES = 30
SEED = 20260709

SKELS = ["ADD(#0,#1)", "MUL(#0,#1)", "NEG(#0)", "SUM3(#0,#1,#2)", "DATA"]


def gen_case(rng):
    """Random layered DAG: some DATA leaves + computed nodes over earlier nodes."""
    n_leaves = rng.randint(2, 4)
    n_comp = rng.randint(1, 4)
    nodes = list(range(1, n_leaves + n_comp + 1))
    skel, deps = {}, {}
    for i in range(1, n_leaves + 1):
        skel[i], deps[i] = "DATA", []
    for i in range(n_leaves + 1, n_leaves + n_comp + 1):
        s = rng.choice([x for x in SKELS if x != "DATA"])
        arity = s.count("#")
        pool = list(range(1, i))
        deps[i] = [rng.choice(pool) for _ in range(arity)]
        skel[i] = s
    offset = rng.randint(5, 50)
    sigma = {n: n + offset for n in nodes}
    # faithful edited copy
    skel1 = {sigma[n]: skel[n] for n in nodes}
    deps1 = {sigma[n]: [sigma[m] for m in deps[n]] for n in nodes}
    # maybe botch
    kind = rng.choice(["faithful", "wrong_skel", "reorder", "retarget", "drop_dep"])
    comp_nodes = [n for n in nodes if skel[n] != "DATA"]
    multi = [n for n in comp_nodes if len(deps[n]) >= 2]
    if kind == "wrong_skel" and comp_nodes:
        t = sigma[rng.choice(comp_nodes)]
        skel1[t] = "SUB(#0,#1)" if skel1[t] != "SUB(#0,#1)" else "ADD(#0,#1)"
    elif kind == "reorder" and multi:
        t = sigma[rng.choice(multi)]
        if len(set(deps1[t])) >= 2:                     # reversal must actually change it
            deps1[t] = list(reversed(deps1[t]))
        else:
            kind = "faithful"
    elif kind == "retarget" and comp_nodes:
        t = sigma[rng.choice(comp_nodes)]
        others = [sigma[m] for m in nodes if sigma[m] not in deps1[t]]
        if others and deps1[t]:
            i = rng.randrange(len(deps1[t]))
            old = deps1[t][i]
            new = rng.choice(others)
            deps1[t] = deps1[t][:i] + [new] + deps1[t][i + 1:]
            if new == old:
                kind = "faithful"
        else:
            kind = "faithful"
    elif kind == "drop_dep" and comp_nodes:
        t = sigma[rng.choice(comp_nodes)]
        if deps1[t]:
            deps1[t] = deps1[t][:-1]
        else:
            kind = "faithful"
    else:
        kind = "faithful"
    return nodes, skel, deps, sigma, skel1, deps1, kind


def lean_fun(mapping, default, ty):
    """Emit a Lean fun via nested ifs over a dict."""
    out = f"fun n => {default}"
    body = default
    for k, v in sorted(mapping.items()):
        body = f"if n = {k} then {v} else ({body})"
    return f"fun n => {body}"


def to_lean_case(idx, nodes, skel, deps, sigma, skel1, deps1):
    def skel_fun(sk):
        return lean_fun({k: f"\"{v}\"" for k, v in sk.items()}, "\"DATA\"", "String")
    def deps_fun(dp):
        return lean_fun({k: "[" + ", ".join(map(str, v)) + "]" for k, v in dp.items()}, "[]", "List Nat")
    sig = lean_fun({k: str(v) for k, v in sigma.items()}, "n", "Nat")
    return f"""
private def s0_{idx} : SynComp Nat String :=
  {{ nodes := [{", ".join(map(str, nodes))}]
    skel := {skel_fun(skel)}
    deps := {deps_fun(deps)} }}
private def s1_{idx} : SynComp Nat String :=
  {{ nodes := [{", ".join(str(sigma[n]) for n in nodes)}]
    skel := {skel_fun(skel1)}
    deps := {deps_fun(deps1)} }}
#eval check s0_{idx} s1_{idx} ({sig})
"""


def router_verdict(nodes, skel, deps, sigma, skel1, deps1):
    O = {n: float(i * 7 + 1) for i, n in enumerate(nodes)}          # faithful oracle
    A = Artifact(fn=dict(skel), deps={k: list(v) for k, v in deps.items()}, O=O)
    O1 = {sigma[n]: O[n] for n in nodes}
    B = Artifact(fn=dict(skel1), deps={k: list(v) for k, v in deps1.items()}, O=O1)
    cert = certify_edit(A, B, lambda n: sigma.get(n, n), set())
    return cert.status == "CERTIFIED"


if __name__ == "__main__":
    os.makedirs(WORK, exist_ok=True)
    rng = random.Random(SEED)
    cases = [gen_case(rng) for _ in range(N_CASES)]
    # Lean side: one file, N #eval lines
    lean_src = LEAN_HEADER
    for i, (nodes, skel, deps, sigma, skel1, deps1, kind) in enumerate(cases):
        lean_src += to_lean_case(i, nodes, skel, deps, sigma, skel1, deps1)
    lean_src += "\nend Checker\n"
    lp = os.path.join(WORK, "battery.lean")
    open(lp, "w").write(lean_src)
    env = dict(os.environ)
    env["PATH"] = os.path.expanduser("~/.elan/bin") + ":" + env["PATH"]
    r = subprocess.run(["lean", lp], capture_output=True, text=True, env=env, timeout=600)
    if r.returncode != 0:
        print("lean battery failed:", (r.stderr or r.stdout)[:400]); sys.exit(2)
    lean_verdicts = [ln.strip() == "true" for ln in r.stdout.strip().splitlines()]
    assert len(lean_verdicts) == N_CASES, f"expected {N_CASES} verdicts, got {len(lean_verdicts)}"

    agree = disagree = 0
    for i, (nodes, skel, deps, sigma, skel1, deps1, kind) in enumerate(cases):
        rv = router_verdict(nodes, skel, deps, sigma, skel1, deps1)
        lv = lean_verdicts[i]
        ok = rv == lv
        agree += ok; disagree += (not ok)
        exp = kind == "faithful"
        mark = "OK " if ok else "DISAGREE"
        print(f"  case {i:2d} [{kind:10}] lean={lv} router={rv} expected_certifiable={exp}  {mark}")
        if lv != exp:
            print(f"    !! lean verdict differs from construction expectation")
    print(f"\nagreement: {agree}/{N_CASES}, disagreements: {disagree}")
    sys.exit(1 if disagree else 0)
