#!/usr/bin/env python3
"""THE VERIFIABILITY THESIS, tested on the spreadsheet path: does the certify-or-
refuse router, ENGINE-FREE, correctly rule on edits it did NOT author?

The edit-path A/B (agent_ab.py) had xlq author AND self-certify — so it never
tested the certifier as a checker of UNTRUSTED FOREIGN work, which is the entire
moat claim. Here we build the reference-dependency graph of the ORIGINAL and of a
FOREIGN-edited file (openpyxl's output — xlq did not produce it), and run the same
router (experiments/generality/router.certify_edit) with σ = insert-row@2:

  CERTIFY  the foreign edit's graph IS the σ-relabeling of the original.
  REFUSE   any unaccounted difference (openpyxl leaves references un-shifted, so a
           formula moved below the insert now reads σ(host) with UN-shifted deps
           != σ(deps) -> mismatch -> REFUSE).

The soundness metric that matters: FALSE CERTIFICATIONS — a foreign edit the router
CERTIFIES that the INDEPENDENT engine (LibreOffice, from agent_ab.json) says is
corrupted. That number must be 0 for the thesis to hold. We cross-check every
router verdict against the independent-oracle label."""
import json, os, re, sys, zipfile
from collections import Counter

sys.path.insert(0, os.path.dirname(__file__))
sys.path.insert(0, "/home/soh/aix/experiments/generality")
from core import Artifact
from router import certify_edit
from forward_correctness import first_sheet_part, openpyxl_insert, VOLATILE, K

CORPUS = "/home/soh/aix/vendor/upstream/xlsx/tests"

# one A1 reference, optionally sheet-qualified, optional $abs, no range colon here.
REF = re.compile(r"(?:(?:'[^']+'|[A-Za-z_][A-Za-z0-9_.]*)!)?(\$?)([A-Z]{1,3})(\$?)(\d+)")
# a range is two refs joined by ':'. Match ranges first so we don't split them.
RANGE = re.compile(r"(?:(?:'[^']+'|[A-Za-z_][A-Za-z0-9_.]*)!)?"
                   r"\$?[A-Z]{1,3}\$?\d+:\$?[A-Z]{1,3}\$?\d+")
CELLTAG = re.compile(rb'<c r="([A-Z]+)(\d+)"((?:(?!</c>).)*?)</c>', re.S)
FTAG = re.compile(rb'<f[^>]*>([^<]*)</f>')
VTAG = re.compile(rb'<v>([^<]*)</v>')


def col_num(s):
    n = 0
    for ch in s:
        n = n * 26 + (ord(ch) - 64)
    return n


def parse_refs(formula):
    """Return (fn_skeleton, deps) where deps is an ordered list of tokens:
    ('C', col, row) for a cell, ('R', c1, r1, c2, r2) for a range. fn_skeleton is
    the formula with every ref replaced by #i in order (structure, name-free)."""
    deps, out, i, slot = [], [], 0, 0
    # tokenize by scanning ranges then cells left to right
    tokens = []
    for m in RANGE.finditer(formula):
        tokens.append((m.start(), m.end(), "range", m.group(0)))
    # cells NOT inside a range span
    spans = [(s, e) for s, e, _, _ in tokens]
    for m in REF.finditer(formula):
        if any(s <= m.start() < e for s, e in spans):
            continue
        tokens.append((m.start(), m.end(), "cell", m.group(0)))
    tokens.sort()
    pos = 0
    for s, e, kind, txt in tokens:
        out.append(formula[pos:s]); out.append(f"#{slot}"); slot += 1; pos = e
        if kind == "cell":
            mm = REF.search(txt)
            deps.append(("C", col_num(mm.group(2)), int(mm.group(4))))
        else:
            a, b = txt.split(":")
            ma, mb = REF.search(a), REF.search(b)
            deps.append(("R", col_num(ma.group(2)), int(ma.group(4)),
                         col_num(mb.group(2)), int(mb.group(4))))
    out.append(formula[pos:])
    return "".join(out), deps


def extract(path):
    """Build an Artifact (fn/deps/O) over the first sheet's formula cells.
    Returns None if any formula fails to parse (conservative — we never certify a
    graph we could not fully read)."""
    part = first_sheet_part(path)
    if not part:
        return None
    data = zipfile.ZipFile(path).read(part)
    fn, deps, O = {}, {}, {}
    for m in CELLTAG.finditer(data):
        col, row, body = col_num(m.group(1).decode()), int(m.group(2)), m.group(3)
        fm = FTAG.search(body)
        if not fm:
            continue
        ftext = fm.group(1).decode("utf-8", "replace")
        if VOLATILE.search(ftext.encode()):
            continue                       # position-dependent: skip (like the oracle)
        node = ("C", col, row)
        skel, d = parse_refs(ftext)
        fn[node] = skel
        deps[node] = d
        vm = VTAG.search(body)
        if vm:
            try:
                O[node] = float(vm.group(1))
            except ValueError:
                pass
    return Artifact(fn=fn, deps=deps, O=O) if fn else None


def sigma_insert_row(k):
    """σ for inserting a blank row at k: any coordinate whose row >= k moves +1.
    Applies to host cell-nodes AND to dep tokens (cells and range endpoints)."""
    def sh(r):
        return r + 1 if r >= k else r
    def s(x):
        if x[0] == "C":
            return ("C", x[1], sh(x[2]))
        if x[0] == "R":
            return ("R", x[1], sh(x[2]), x[3], sh(x[4]))
        return x
    return s


# Reference forms this A1 extractor CANNOT model, so a mis-shift of them would go
# undetected -> a silent false certification. FAIL CLOSED: if a formula contains any
# of these, the file is uncertifiable by this extractor and must be REFUSED (the same
# residual-gate discipline xlq applies to tables). Adversarial review found that
# whole-row (6:6), whole-column (A:A), cross-sheet (Sheet2!A5), defined names (NC_1),
# and table refs ([...]) all slipped past the earlier '[' -only gate.
_WHOLE_ROW = re.compile(r"\$?\d+:\$?\d+")                       # 6:6  $6:$7
_WHOLE_COL = re.compile(r"(?<![A-Z0-9])\$?[A-Z]{1,3}:\$?[A-Z]{1,3}(?![A-Z0-9])")  # A:A
_FUNC = re.compile(r"[A-Za-z_][A-Za-z0-9_.]*\s*\(")            # a function call name
_STR = re.compile(r'"[^"]*"')


def uncertifiable_formula(f):
    """True if this formula contains any reference form the extractor cannot fully
    model (so a mis-shift could go undetected). Conservative — errs toward REFUSE."""
    if "[" in f:                       # structured table reference
        return True
    if "!" in f:                       # cross-sheet ref (σ here does not model sheet scoping)
        return True
    if _WHOLE_ROW.search(f) or _WHOLE_COL.search(f):
        return True
    # defined names / any unmodeled identifier: strip strings, ranges, cells, and
    # function-call NAMES, then if any alphabetic residue remains it is an
    # unmodeled name (e.g. NC_1) -> cannot certify.
    s = _STR.sub("", f)
    s = RANGE.sub("", s)
    s = REF.sub("", s)
    s = _FUNC.sub("(", s)
    s = re.sub(r"\b(TRUE|FALSE|AND|OR|NOT|XOR)\b", "", s, flags=re.I)
    return bool(re.search(r"[A-Za-z]", s))


def has_uncertifiable(path):
    """True if any first-sheet formula is uncertifiable by this extractor."""
    part = first_sheet_part(path)
    if not part:
        return False
    data = zipfile.ZipFile(path).read(part)
    for m in FTAG.finditer(data):
        if uncertifiable_formula(m.group(1).decode("utf-8", "replace")):
            return True
    return False


def has_any_cell_ref(A):
    return any(A.deps.get(n) for n in A.fn)


def certify_foreign(orig_path, foreign_path, k):
    # FAIL CLOSED: any formula the extractor cannot fully model -> REFUSE (matches
    # xlq's residual gate; without this, whole-row/whole-col/cross-sheet/defined-name
    # refs would be silently certified — the hole adversarial review found).
    if has_uncertifiable(orig_path) or has_uncertifiable(foreign_path):
        return "REFUSED"
    A = extract(orig_path)
    B = extract(foreign_path)
    if A is None or B is None:
        return None                        # unparseable -> not testable here
    sig = sigma_insert_row(k)
    # declared fills: the inserted blank row k has no formulas; the agent declares
    # no value fills for a pure structural insert.
    cert = certify_edit(A, B, sig, declared_fills=set())
    return cert.status, has_any_cell_ref(A)


if __name__ == "__main__":
    # reuse the independent-oracle labels from the edit-path A/B
    ab = json.load(open("/home/soh/aix/benchmarks/agent_ab.json"))
    oracle = {r["file"]: r["unguarded"] for r in ab["per_file"]}  # openpyxl label
    work = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/foreign"
    os.makedirs(work, exist_ok=True)
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 60

    rows, conf = [], Counter()
    false_certs = []
    for rel, olabel in list(oracle.items())[:limit]:
        src = os.path.join(CORPUS, rel)
        try:
            fpath = openpyxl_insert(src, work)     # the FOREIGN edit (xlq didn't make it)
        except Exception:
            continue
        res = certify_foreign(src, fpath, K)
        if res is None:
            conf["unparseable_skip"] += 1
            continue
        if res == "REFUSED":               # table-ref refusal (no cell-ref info)
            verdict, has_refs = "REFUSED", True
        else:
            verdict, has_refs = res
        # oracle: 'SILENT_CORRUPTION' = the foreign edit is engine-wrong; 'faithful' = ok
        corrupted = (olabel == "SILENT_CORRUPTION")
        if verdict == "REFUSED" and corrupted:
            conf["refused_corrupted_CORRECT"] += 1
        elif verdict == "REFUSED" and not corrupted:
            conf["refused_faithful_conservative"] += 1
        elif verdict == "CERTIFIED" and not corrupted:
            conf["certified_faithful_CORRECT"] += 1
        elif verdict == "CERTIFIED" and corrupted:
            # A formula with NO cell references is shift-INVARIANT — the router's
            # CERTIFY is provably correct, so an oracle 'corrupted' label here is an
            # ENGINE DISAGREEMENT (LibreOffice != Excel on that function, e.g.
            # ACCRINT), NOT a router false certification. Separate the two.
            if not has_refs:
                conf["certified_norefs_ORACLE_DISAGREEMENT"] += 1
            else:
                conf["certified_corrupted_FALSE_CERTIFICATION"] += 1
                false_certs.append(rel)
        rows.append({"file": rel, "router": verdict, "oracle": olabel, "has_refs": has_refs})

    n = len(rows)
    fc = conf["certified_corrupted_FALSE_CERTIFICATION"]
    caught = conf["refused_corrupted_CORRECT"]
    oracle_dis = conf["certified_norefs_ORACLE_DISAGREEMENT"]
    n_corrupt = caught + fc      # genuine ref-shift corruptions (excl. oracle noise)
    summary = {
        "thesis": "certify-or-refuse router ruling on UNTRUSTED FOREIGN edits (openpyxl's, "
                  "xlq did not author), ENGINE-FREE, cross-checked vs the independent "
                  "LibreOffice oracle from the edit-path A/B",
        "foreign_edits_tested": n,
        "FALSE_CERTIFICATIONS_ref_shift_corruption_certified": fc,
        "false_certification_files": false_certs[:10],
        "corrupted_foreign_edits_caught_REFUSED": caught,
        "corrupted_foreign_edits_total_excl_oracle_noise": n_corrupt,
        "recall_on_corrupted_foreign_edits": round(caught / n_corrupt, 3) if n_corrupt else None,
        "faithful_foreign_certified": conf["certified_faithful_CORRECT"],
        "faithful_foreign_refused_conservative": conf["refused_faithful_conservative"],
        "no_ref_oracle_disagreements_router_correct": oracle_dis,
        "note_extraction_completeness": "FAIL-CLOSED gate: REFUSED includes every file "
            "whose formulas contain a reference form this A1 extractor cannot fully model "
            "— whole-row (6:6), whole-column (A:A), cross-sheet (Sheet2!A5), table ([...]), "
            "or a bare defined-name identifier. Adversarial review found these were "
            "silently CERTIFIED by the earlier '['-only gate (and laundered into the no-ref "
            "'provably correct' bucket). Soundness rests on extraction COMPLETENESS: a "
            "reference the extractor cannot see is a mis-shift it cannot catch. Cost: it now "
            "certifies only 1/23 faithful foreign edits — USEFUL soundness requires xlq's "
            "complete formula parser (the TCB) to model+shift these forms instead of refusing.",
        "breakdown": dict(conf),
        "headline": (f"router ruled on {n} foreign edits ENGINE-FREE: "
                     f"{fc} false certifications (ref-shift corruption wrongly certified); "
                     f"caught {caught}/{n_corrupt} genuine corrupted foreign edits by REFUSING; "
                     f"{oracle_dis} no-ref cases were oracle disagreements (router provably "
                     f"correct). The certifier checking UNTRUSTED work."),
    }
    with open("/home/soh/aix/benchmarks/foreign_certify.json", "w") as f:
        json.dump(summary, f, indent=2)
    print(json.dumps({k2: v for k2, v in summary.items() if k2 != "false_certification_files"}, indent=2))
