#!/usr/bin/env python3
"""Corpus-scale differential: the VERIFIED Lean reference tokenizer/shift
(Tokenizer.lean, run via `lean --run`) vs the PRODUCTION Rust tokenizer
(`xlq __shift-formula-batch`). This discharges the remaining trusted link:
"the Rust implements the verified reference on the model surface."

Comparison classes per (formula, edit):
  IN-SURFACE  Lean produces a shifted formula -> Rust must match EXACTLY.
              (delete edits on formulas containing ':' are excluded: the model
              #REF!s range endpoints per-cell while production applies the
              Z3-proved clamp — a documented, deliberate semantic difference.)
  GUARD       Lean refuses (unquoted '!'/"'" outside literals). If the formula
              trips the non-ASCII-qualifier detector, Rust must refuse too
              (__REFUSE__). ASCII-qualified formulas are out of the model
              surface (production legitimately shifts them): counted, skipped.
Anything else that disagrees is a FINDING.

Inputs: formal/corpus_formulas.txt (deduped corpus formulas) + a generated
battery (defect shapes, edge cases). Outputs: formal/tokenizer_differential.json
"""
import json, os, random, re, subprocess, sys
sys.path.insert(0, "/home/soh/aix/benchmarks")
from shift_correctness_real import WHOLECOL, WHOLEROW   # the committed gates

LEAN = os.path.expanduser("~/.elan/bin/lean")
TOKENIZER = "/home/soh/aix/formal/Tokenizer.lean"
XLQ = "/home/soh/aix/xlq/target/release/xlq"
CORPUS = "/home/soh/aix/formal/corpus_formulas.txt"
OUT = "/home/soh/aix/formal/tokenizer_differential.json"
EDITS = [("row", "insert", 2, 1), ("row", "delete", 4, 1),
         ("col", "insert", 2, 1), ("col", "delete", 3, 1)]
CAP = int(sys.argv[1]) if len(sys.argv) > 1 else 20000   # formulas (sorted prefix + battery)

NONASCII_QUAL = None  # computed per formula with the same backwalk as the Rust guard


def has_unquoted_nonascii_qualifier(f: str) -> bool:
    b = f.encode("utf-8")
    i, n = 0, len(b)
    delims = set(b"()+,-*/^&=<>; {}%\"'")
    while i < n:
        c = b[i]
        if c == 0x22:                                   # " literal
            i += 1
            while i < n:
                if b[i] == 0x22:
                    if i + 1 < n and b[i + 1] == 0x22:
                        i += 2; continue
                    break
                i += 1
            i += 1
        elif c == 0x27:                                 # ' quoted qualifier
            i += 1
            while i < n:
                if b[i] == 0x27:
                    if i + 1 < n and b[i + 1] == 0x27:
                        i += 2; continue
                    break
                i += 1
            i += 1
        elif c == 0x21:                                 # !
            j = i
            while j > 0 and b[j - 1] not in delims:
                j -= 1
            if any(x >= 0x80 for x in b[j:i]):
                return True
            i += 1
        else:
            i += 1
    return False


def battery():
    """Generated edge cases incl. both defect shapes."""
    out = [
        'IF(C8="","",IF(C8=$IA$4,"大当たり！","はずれ！もう一度考えよう！"))',
        'IF(A5=1,"café ○ 𝄞","×")&B5',
        "SUM(A2:B5)+LOG10(A2)", "BIN2DEC(A3)", "LOG10(B2)+LOG2(C3)",
        "A1+B2*C3-$D$4/E5", "$A2+A$2+$A$2", "XFD1048576+A1", "XFE9+A2000000",
        "Sales2020+A4", "MAX(A2,A3,A4)", "SUM(CHOOSE(2,A9,A10))",
        'TEXT(A2,"0.00")&"円"', 'CONCATENATE("A2",B3)', '"A2"&A2',
    ]
    rng = random.Random(20260710)
    cols = ["A", "B", "Z", "AA", "IV", "XFD", "XFE"]
    for _ in range(300):
        parts = []
        for _ in range(rng.randint(1, 4)):
            c = rng.choice(cols); r = rng.randint(1, 1100000)
            dollar1 = "$" if rng.random() < .3 else ""
            dollar2 = "$" if rng.random() < .3 else ""
            parts.append(f"{dollar1}{c}{dollar2}{r}")
        f = "+".join(parts)
        if rng.random() < .3:
            f = f'IF({f}>0,"はい","no")'
        out.append(f)
    return out


def run_lean(lines):
    payload = "".join(l + "\n" for l in lines)
    env = dict(os.environ); env["PATH"] = os.path.dirname(LEAN) + ":" + env["PATH"]
    r = subprocess.run([LEAN, "--run", TOKENIZER], input=payload,
                       capture_output=True, text=True, timeout=7200, env=env)
    lines = r.stdout.splitlines()
    start = max(i for i, l in enumerate(lines) if l.strip() == "__BEGIN__")
    return [l for l in lines[start + 1:] if l.strip() != ""]


def run_rust(lines):
    payload = "".join(l + "\n" for l in lines)
    r = subprocess.run([XLQ, "__shift-formula-batch"], input=payload,
                       capture_output=True, text=True, timeout=3600)
    return r.stdout.splitlines()


if __name__ == "__main__":
    formulas = battery()
    if os.path.exists(CORPUS):
        with open(CORPUS) as fh:
            corpus = [l.rstrip("\n") for l in fh if l.strip()]
        rng = random.Random(20260710)
        formulas += corpus if len(corpus) <= CAP else rng.sample(corpus, CAP)
    else:
        print("WARNING: corpus_formulas.txt missing — battery only", flush=True)
    # build request lines
    reqs = []
    for f in formulas:
        if "\t" in f or "\n" in f:
            continue
        for axis, op, at, cnt in EDITS:
            reqs.append(f"{f}\t{axis}\t{op}\t{at}\t{cnt}")
    print(f"formulas: {len(formulas)}  requests: {len(reqs)}", flush=True)

    lean_out = run_lean(reqs)
    rust_out = run_rust(reqs)
    assert len(lean_out) == len(reqs), f"lean lines {len(lean_out)} != {len(reqs)}"
    assert len(rust_out) == len(reqs), f"rust lines {len(rust_out)} != {len(reqs)}"

    stats = {"in_surface_agree": 0, "in_surface_DISAGREE": 0,
             "guard_agree_refuse": 0, "guard_DISAGREE": 0,
             "oos_ascii_qualified": 0, "excluded_delete_range": 0,
             "oos_wholecolrow": 0, "lean_refused_other": 0}
    findings = []
    for req, lo, ro in zip(reqs, lean_out, rust_out):
        f, axis, op, at, cnt = req.split("\t")
        lo = lo.strip()
        if lo == "__REFUSE__":
            if has_unquoted_nonascii_qualifier(f):
                if ro.strip() == "__REFUSE__":
                    stats["guard_agree_refuse"] += 1
                else:
                    stats["guard_DISAGREE"] += 1
                    if len(findings) < 15:
                        findings.append({"class": "guard", "formula": f, "edit": req.split("\t")[1:],
                                         "lean": lo, "rust": ro})
            elif "!" in f or "'" in f:
                stats["oos_ascii_qualified"] += 1
            else:
                stats["lean_refused_other"] += 1   # e.g. unterminated literal
        else:
            lo2 = lo[6:-1].encode().decode("unicode_escape").encode("latin-1").decode("utf-8") \
                if lo.startswith('some "') else lo
            # Lean driver prints the raw shifted string (main), not `some "..."` — handle both
            expect = lo if not lo.startswith('some "') else lo2
            if op == "delete" and ":" in f:
                stats["excluded_delete_range"] += 1
                continue
            if WHOLECOL.search(f) or WHOLEROW.search(f):
                # whole-column/row refs are production surface (engine-validated
                # dev-tier); the model's verified surface is single-cell refs
                stats["oos_wholecolrow"] += 1
                continue
            if ro == expect:
                stats["in_surface_agree"] += 1
            else:
                stats["in_surface_DISAGREE"] += 1
                if len(findings) < 15:
                    findings.append({"class": "surface", "formula": f,
                                     "edit": req.split("\t")[1:], "lean": expect, "rust": ro})
    result = {"benchmark": "Lean verified reference tokenizer vs Rust production tokenizer",
              "requests": len(reqs), "stats": stats, "findings": findings}
    json.dump(result, open(OUT, "w"), indent=2, ensure_ascii=False)
    print(json.dumps(result, indent=2, ensure_ascii=False)[:1800])
    sys.exit(1 if (stats["in_surface_DISAGREE"] or stats["guard_DISAGREE"]) else 0)
