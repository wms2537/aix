#!/usr/bin/env python3
"""COINCIDENCE BOUND, Monte Carlo — simulate the error on real workbooks and
measure the empirical miss rate of a k-cell value check; compare to the
analytic bound (coincidence_q.py) and to engine ground truth (agent_ab.json).

Errors simulated
  M2: the EXACT openpyxl failure from the edit-path A/B — insert a blank row
      at row 2, shift NO references. Read map on the original grid:
      a reference to row 1 reads old row 1 (unaffected); to row 2 reads the
      inserted BLANK; to row j>=3 reads old row j-1. Whole-column ranges are
      unaffected (same multiset + one blank). One deterministic error per
      file; the randomness is which k cells the verifier checks.
  M1: a single reference in a single formula is re-targeted uniformly to
      another row of the same column's used span (localized error).

Two measurement levels
  LEVEL A (input-level, all analyzable formulas): a checked formula MISSES
      iff every value it reads under the error equals the value it should
      have read ("miss <=> the misread input equals the correct input").
      Exactly the event whose probability is q in the derivation. Relation to
      the true output-level check: (i) non-injective formulas (MAX, IF,
      COUNT, ROUND...) can also pass with a DIFFERING input, so input-level
      understates the miss rate per cell; (ii) it ignores corruption
      propagating INTO a checked cell from an unchecked upstream formula
      cell, which understates detection. Sheet-wide, input-collision of ALL
      formulas implies (by induction over the dependency order) that every
      recomputed value is unchanged, i.e. the edit passes ANY check.
  LEVEL B (output-level, evaluator subset): a small formula evaluator
      (arithmetic, comparisons, SUM/AVERAGE/MIN/MAX/COUNT/COUNTA/PRODUCT/
      ABS/SQRT/ROUND/INT/POWER/MOD/PI/IF/AND/OR/NOT) VALIDATED per formula by
      reproducing the Excel-cached <v> from the correct reads; only validated
      formulas are used. Measures the true per-cell output collision
      P(f(wrong reads) = f(correct reads)) including non-injectivity — the
      "injectivity gap" over Level A.

k-cell check: the verifier samples k of the N checkable formula cells
uniformly without replacement; the file-level miss probability is exact
(hypergeometric over the per-formula pass indicators) — no sampling noise.

Ground-truth anchor: agent_ab.json recorded, per file, whether LibreOffice
found openpyxl's insert@2 output corrupted. Level A's sheet-wide prediction
is compared per-file against that engine verdict (confusion matrix).

Excluded (counted, reported): formulas with defined names, row-ranges,
volatile/position-dependent functions (same list as forward_correctness.py),
unparseable text. Foreign (sheet-qualified) refs are unaffected reads for
M2 on the first sheet.
"""
import json
import math
import os
import random
import re
import statistics
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from coincidence_q import (CORPUS, EMPTY, collide, column_spans, parse_sheet,
                           col_num)

OUT = "/home/soh/aix/benchmarks/coincidence_mc.json"
AGENT_AB = "/home/soh/aix/benchmarks/agent_ab.json"
TESTS_ROOT = "/home/soh/aix/vendor/upstream/xlsx/tests"
SEED = 20260709
N_MC_FILES = 30
M1_TRIALS = 400
KS = [1, 2, 3, 5, 10]
MAX_RANGE_CELLS = 20000

VOLATILE = {"OFFSET", "INDIRECT", "NOW", "TODAY", "RAND", "RANDBETWEEN",
            "CELL", "INFO", "ROW", "COLUMN", "ROWS", "COLUMNS"}


# ------------------------------------------------------------- tokenizer ---

class Unanalyzable(Exception):
    pass


class ExcelError(Exception):
    def __init__(self, code):
        self.code = code


TOK = re.compile(r"""
  (?P<ws>\s+)
| (?P<str>"(?:[^"]|"")*")
| (?P<sheet>(?:'[^']*'|[A-Za-z_][A-Za-z0-9_.]*)!)
| (?P<ref>\$?[A-Za-z]{1,3}\$?[0-9]{1,7}(?![0-9A-Za-z_(]))
| (?P<colrange>\$?[A-Za-z]{1,3}:\$?[A-Za-z]{1,3}(?![0-9A-Za-z_(:]))
| (?P<num>(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[Ee][+-]?[0-9]+)?)
| (?P<func>[A-Za-z_][A-Za-z0-9_.]*(?=\())
| (?P<bool>(?:TRUE|FALSE)(?![0-9A-Za-z_]))
| (?P<op><>|<=|>=|[-+*/^&%<>=(),{;}])
""", re.X)


def tokenize(text):
    toks, i = [], 0
    while i < len(text):
        m = TOK.match(text, i)
        if not m:
            raise Unanalyzable(f"token at {i}: {text[i:i+12]!r}")
        i = m.end()
        kind = m.lastgroup
        if kind == "ws":
            continue
        toks.append((kind, m.group(0)))
    toks.append(("end", ""))
    return toks


def _parse_atom_ref(s):
    m = re.match(r"\$?([A-Za-z]{1,3})\$?([0-9]+)$", s)
    return int(m.group(2)), col_num(m.group(1))


# ---------------------------------------------------------------- parser ---
# AST nodes are lists (mutable, identity-distinct per occurrence):
# ['num',x] ['str',s] ['bool',b] ['ref',r,c] ['range',r1,c1,r2,c2]
# ['colrange'] ['foreign'] ['call',NAME,[args]] ['bin',op,a,b] ['un',op,a]
# ['pct',a] ['arr',[values]]

class Parser:
    def __init__(self, toks):
        self.toks, self.i = toks, 0

    def peek(self):
        return self.toks[self.i]

    def take(self, kind=None, val=None):
        k, v = self.toks[self.i]
        if (kind and k != kind) or (val is not None and v != val):
            raise Unanalyzable(f"expected {kind or val}, got {k}:{v!r}")
        self.i += 1
        return v

    def parse(self):
        node = self.expr_cmp()
        if self.peek()[0] != "end":
            raise Unanalyzable(f"trailing {self.peek()!r}")
        return node

    def _binloop(self, sub, ops):
        node = sub()
        while self.peek() == ("op", self.peek()[1]) and self.peek()[1] in ops:
            op = self.take("op")
            node = ["bin", op, node, sub()]
        return node

    def expr_cmp(self):
        return self._binloop(self.expr_concat, {"=", "<>", "<", ">", "<=", ">="})

    def expr_concat(self):
        return self._binloop(self.expr_add, {"&"})

    def expr_add(self):
        return self._binloop(self.expr_mul, {"+", "-"})

    def expr_mul(self):
        return self._binloop(self.expr_pow, {"*", "/"})

    def expr_pow(self):
        return self._binloop(self.expr_unary, {"^"})

    def expr_unary(self):
        k, v = self.peek()
        if k == "op" and v in ("-", "+"):
            self.take("op")
            return ["un", v, self.expr_unary()]
        return self.expr_post(self.primary())

    def expr_post(self, node):
        while self.peek() == ("op", "%"):
            self.take("op")
            node = ["pct", node]
        return node

    def primary(self):
        k, v = self.peek()
        if k == "num":
            self.take()
            return ["num", float(v)]
        if k == "str":
            self.take()
            return ["str", v[1:-1].replace('""', '"')]
        if k == "bool":
            self.take()
            return ["bool", v.upper() == "TRUE"]
        if k == "sheet":
            self.take()
            kk, _ = self.peek()
            if kk in ("ref", "colrange"):
                self.take()
                if self.peek() == ("op", ":") or (
                        self.peek()[0] == "ref" and False):
                    pass
                # possible foreign range: Sheet!A1:B2 tokenizes ref then :ref?
                # ':' is not in our op list, so A1:B2 came out as... handle
                # below via colrange/ref only; ranges A1:B2 tokenize as
                # ref(A1:?) no — ':' isn't an op token; A1:B2 matches
                # ref "A1" then ':' fails to tokenize... see RANGE note.
                return ["foreign"]
            raise Unanalyzable("sheet prefix without ref")
        if k == "ref":
            self.take()
            r, c = _parse_atom_ref(v)
            return ["ref", r, c]
        if k == "colrange":
            self.take()
            return ["colrange"]
        if k == "func":
            name = self.take().upper()
            self.take("op", "(")
            args = []
            if self.peek() != ("op", ")"):
                args.append(self.expr_cmp())
                while self.peek() == ("op", ","):
                    self.take("op")
                    args.append(self.expr_cmp())
            self.take("op", ")")
            return ["call", name, args]
        if k == "op" and v == "(":
            self.take("op")
            node = self.expr_cmp()
            self.take("op", ")")
            return node
        if k == "op" and v == "{":
            self.take("op")
            vals = []
            while self.peek() != ("op", "}"):
                kk, vv = self.peek()
                if kk == "num":
                    vals.append(float(self.take()))
                elif kk == "str":
                    vals.append(self.take()[1:-1].replace('""', '"'))
                elif kk == "op" and vv in (",", ";"):
                    self.take()
                elif kk == "op" and vv == "-":
                    self.take()
                    vals.append(-float(self.take("num")))
                else:
                    raise Unanalyzable("array literal")
            self.take("op", "}")
            return ["arr", vals]
        raise Unanalyzable(f"primary {k}:{v!r}")


# Ranges: ':' is not a tokenizer op, so "A1:B2" must be pre-combined.
# We do it textually before tokenizing: replace ATOM:ATOM with a synthetic
# single token via a placeholder pass.
RANGE_RE = re.compile(
    r"(?<![0-9A-Za-z_$!.:])(\$?[A-Za-z]{1,3}\$?[0-9]{1,7})"
    r":(\$?[A-Za-z]{1,3}\$?[0-9]{1,7})(?![0-9A-Za-z_(:])")
ROWRANGE_RE = re.compile(r"(?<![A-Za-z0-9_$:])[0-9]+:[0-9]+(?![0-9])")


def parse_formula(text):
    """-> (ast, meta). meta: dict with flags/collections."""
    if not text:
        raise Unanalyzable("no text")
    masked = re.sub(r'"(?:[^"]|"")*"', lambda m: '"' + "_" * (len(m.group(0)) - 2) + '"', text)
    if ROWRANGE_RE.search(masked):
        raise Unanalyzable("row range")
    # pre-extract ranges into placeholders the tokenizer can't split
    ranges = []

    def repl(m):
        ranges.append((m.group(1), m.group(2)))
        return f"RANGEPLACEHOLDER{len(ranges) - 1}Z(0)"

    # apply on real text but guided by masked spans: do both in lockstep
    out_parts, idx = [], 0
    for m in RANGE_RE.finditer(masked):
        out_parts.append(text[idx:m.start()])
        r1 = text[m.start(1):m.end(1)]
        r2 = text[m.start(2):m.end(2)]
        ranges.append((r1, r2))
        out_parts.append(f"RANGEPLACEHOLDER{len(ranges) - 1}Z(0)")
        idx = m.end()
    out_parts.append(text[idx:])
    text2 = "".join(out_parts)

    ast = Parser(tokenize(text2)).parse()
    meta = {"volatile": False, "foreign": False, "colrange": False,
            "unknown_funcs": set(), "refs": [], "ranges": []}

    def walk(n):
        if not isinstance(n, list):
            return
        tag = n[0]
        if tag == "call":
            name = n[1]
            pm = re.match(r"RANGEPLACEHOLDER(\d+)Z$", name)
            if pm:
                a, b = ranges[int(pm.group(1))]
                r1, c1 = _parse_atom_ref(a)
                r2, c2 = _parse_atom_ref(b)
                n[:] = ["range", min(r1, r2), min(c1, c2),
                        max(r1, r2), max(c1, c2)]
                if (n[3] - n[1] + 1) * (n[4] - n[2] + 1) > MAX_RANGE_CELLS:
                    raise Unanalyzable("huge range")
                meta["ranges"].append(n)
                return
            if name in VOLATILE:
                meta["volatile"] = True
            meta["unknown_funcs"].add(name)
            for a in n[2]:
                walk(a)
            return
        if tag == "ref":
            meta["refs"].append(n)
        elif tag == "foreign":
            meta["foreign"] = True
        elif tag == "colrange":
            meta["colrange"] = True
        for x in n[1:]:
            if isinstance(x, list):
                if x and isinstance(x[0], str):
                    walk(x)
                else:
                    for y in x:
                        walk(y)
    walk(ast)
    return ast, meta


# ------------------------------------------------------- level A: inputs ---

def m2_read(grid, r, c):
    """Value a reference to (r,c) reads after openpyxl insert@2 (data moved
    down, reference not shifted)."""
    if r == 1:
        return grid.get((1, c))
    if r == 2:
        return EMPTY
    return grid.get((r - 1, c))


def formula_reads(ast_meta):
    """All (r,c) coordinates a formula reads (singles + expanded ranges)."""
    _, meta = ast_meta
    coords = [(n[1], n[2]) for n in meta["refs"]]
    for n in meta["ranges"]:
        for r in range(n[1], n[3] + 1):
            for c in range(n[2], n[4] + 1):
                coords.append((r, c))
    return coords


def m2_input_pass(grid, coords):
    """True iff every read value under the M2 error equals the correct one."""
    for (r, c) in coords:
        if r >= 2 and not collide(m2_read(grid, r, c), grid.get((r, c))):
            return False
    return True


# ---------------------------------------------------- level B: evaluator ---

BLANK = object()
SUPPORTED = {"SUM", "AVERAGE", "MIN", "MAX", "COUNT", "COUNTA", "PRODUCT",
             "ABS", "SQRT", "ROUND", "INT", "POWER", "MOD", "PI", "IF",
             "AND", "OR", "NOT"}


def _num(x):
    if isinstance(x, bool):
        return 1.0 if x else 0.0
    if isinstance(x, float):
        return x
    if x is BLANK:
        return 0.0
    if isinstance(x, str):
        try:
            return float(x)
        except ValueError:
            raise ExcelError("#VALUE!")
    raise ExcelError("#VALUE!")


def _typerank(x):
    if isinstance(x, bool):
        return 2
    if isinstance(x, str):
        return 1
    return 0


def _cmp(a, b, op):
    if a is BLANK and b is BLANK:
        d = 0
    elif a is BLANK:
        return _cmp(0.0 if not isinstance(b, str) else "", b, op) \
            if not isinstance(b, bool) else _cmp(False, b, op)
    elif b is BLANK:
        return _cmp(a, 0.0 if not isinstance(a, str) else "", op) \
            if not isinstance(a, bool) else _cmp(a, False, op)
    else:
        ra, rb = _typerank(a), _typerank(b)
        if ra != rb:
            d = ra - rb
        elif isinstance(a, str):
            al, bl = a.lower(), b.lower()
            d = (al > bl) - (al < bl)
        else:
            av, bv = float(a), float(b)
            d = (av > bv) - (av < bv)
    return {"=": d == 0, "<>": d != 0, "<": d < 0, ">": d > 0,
            "<=": d <= 0, ">=": d >= 0}[op]


def _flatten_nums(vals):
    """Numeric elements per Excel RANGE semantics (skip text/bool/blank)."""
    out = []
    for v in vals:
        if isinstance(v, list):
            out.extend(x for x in v
                       if isinstance(x, float) and not isinstance(x, bool))
        elif v is BLANK:
            continue
        else:
            out.append(_num(v))
    return out


def eval_ast(node, read):
    tag = node[0]
    if tag == "num":
        return node[1]
    if tag == "str":
        return node[1]
    if tag == "bool":
        return node[1]
    if tag == "ref":
        return read(node[1], node[2], node)
    if tag == "range":
        vals = []
        for r in range(node[1], node[3] + 1):
            for c in range(node[2], node[4] + 1):
                vals.append(read(r, c, node))
        return vals
    if tag == "arr":
        return list(node[1])
    if tag == "pct":
        return _num(eval_ast(node[1], read)) / 100.0
    if tag == "un":
        v = _num(eval_ast(node[2], read))
        return -v if node[1] == "-" else v
    if tag == "bin":
        op = node[1]
        a = eval_ast(node[2], read)
        b = eval_ast(node[3], read)
        if op in ("=", "<>", "<", ">", "<=", ">="):
            return _cmp(a, b, op)
        if op == "&":
            def s(x):
                if x is BLANK:
                    return ""
                if isinstance(x, bool):
                    return "TRUE" if x else "FALSE"
                if isinstance(x, float):
                    return repr(int(x)) if x == int(x) else repr(x)
                return x
            return s(a) + s(b)
        x, y = _num(a), _num(b)
        if op == "+":
            return x + y
        if op == "-":
            return x - y
        if op == "*":
            return x * y
        if op == "/":
            if y == 0:
                raise ExcelError("#DIV/0!")
            return x / y
        if op == "^":
            try:
                v = x ** y
            except (OverflowError, ZeroDivisionError, ValueError):
                raise ExcelError("#NUM!")
            if isinstance(v, complex):
                raise ExcelError("#NUM!")
            return float(v)
    if tag == "call":
        return _call(node[1], node[2], read)
    raise Unanalyzable(f"eval {tag}")


def _call(name, argnodes, read):
    if name not in SUPPORTED:
        raise Unanalyzable(f"func {name}")
    if name == "IF":
        c = eval_ast(argnodes[0], read)
        if isinstance(c, str):
            raise ExcelError("#VALUE!")
        cond = bool(c) if isinstance(c, bool) else (c is not BLANK and _num(c) != 0)
        if cond:
            return eval_ast(argnodes[1], read)
        return eval_ast(argnodes[2], read) if len(argnodes) > 2 else False
    args = [eval_ast(a, read) for a in argnodes]
    if name == "PI":
        return math.pi
    if name == "SUM":
        return float(sum(_flatten_nums(args)))
    if name == "PRODUCT":
        ns = _flatten_nums(args)
        p = 1.0
        for v in ns:
            p *= v
        return p if ns else 0.0
    if name == "COUNT":
        n = 0
        for v in args:
            if isinstance(v, list):
                n += sum(1 for x in v
                         if isinstance(x, float) and not isinstance(x, bool))
            elif isinstance(v, (float, bool)):
                n += 1
            elif isinstance(v, str):
                try:
                    float(v)
                    n += 1
                except ValueError:
                    pass
        return float(n)
    if name == "COUNTA":
        n = 0
        for v in args:
            if isinstance(v, list):
                n += sum(1 for x in v if x is not BLANK)
            elif v is not BLANK:
                n += 1
        return float(n)
    if name == "AVERAGE":
        ns = _flatten_nums(args)
        if not ns:
            raise ExcelError("#DIV/0!")
        return sum(ns) / len(ns)
    if name in ("MIN", "MAX"):
        ns = _flatten_nums(args)
        if not ns:
            return 0.0
        return max(ns) if name == "MAX" else min(ns)
    if name == "ABS":
        return abs(_num(args[0]))
    if name == "SQRT":
        v = _num(args[0])
        if v < 0:
            raise ExcelError("#NUM!")
        return math.sqrt(v)
    if name == "INT":
        return float(math.floor(_num(args[0])))
    if name == "ROUND":
        x, d = _num(args[0]), int(_num(args[1]))
        m = 10.0 ** d
        return math.copysign(math.floor(abs(x) * m + 0.5), x) / m
    if name == "POWER":
        return eval_ast(["bin", "^", ["num", _num(args[0])],
                         ["num", _num(args[1])]], read)
    if name == "MOD":
        a, b = _num(args[0]), _num(args[1])
        if b == 0:
            raise ExcelError("#DIV/0!")
        return a - b * math.floor(a / b)
    if name == "NOT":
        return not (bool(args[0]) if isinstance(args[0], bool)
                    else _num(args[0]) != 0)
    if name in ("AND", "OR"):
        bs = []
        for v in args:
            items = v if isinstance(v, list) else [v]
            for x in items:
                if isinstance(x, bool):
                    bs.append(x)
                elif isinstance(x, float):
                    bs.append(x != 0)
        if not bs:
            raise ExcelError("#VALUE!")
        return any(bs) if name == "OR" else all(bs)
    raise Unanalyzable(f"func {name}")


def typed_to_py(v):
    if v is EMPTY:
        return BLANK
    t, x = v
    if t == "n":
        return x
    if t == "s":
        return x
    if t == "b":
        return x
    raise ExcelError(x)   # ('e', code)


def eval_result(ast, grid, mapper=None, override=None):
    """-> ('v', pyvalue) or ('e', code) or raises Unanalyzable."""
    def read(r, c, node):
        if override and id(node) in override:
            r, c = override[id(node)]
        if mapper:
            return typed_to_py(mapper(grid, r, c))
        return typed_to_py(grid.get((r, c)))
    try:
        return ("v", eval_ast(ast, read))
    except ExcelError as e:
        return ("e", e.code)
    except (RecursionError, MemoryError):
        raise Unanalyzable("resource")


def results_equal(a, b):
    if a[0] != b[0]:
        return False
    if a[0] == "e":
        return a[1] == b[1]
    x, y = a[1], b[1]
    if isinstance(x, bool) or isinstance(y, bool):
        return x is y if (isinstance(x, bool) and isinstance(y, bool)) else False
    if isinstance(x, float) and isinstance(y, float):
        return abs(x - y) <= 1e-9 * max(abs(x), abs(y), 1.0)
    return x == y


def matches_cache(res, cached):
    if cached is EMPTY:
        return False
    t, x = cached
    if res[0] == "e":
        return t == "e" and x == res[1]
    v = res[1]
    if t == "n":
        return (isinstance(v, float) and not isinstance(v, bool)
                and abs(v - x) <= 1e-9 * max(abs(v), abs(x), 1.0))
    if t == "b":
        return isinstance(v, bool) and v == x
    if t == "s":
        return isinstance(v, str) and v == x
    return False


# ------------------------------------------------------------- per file ---

def hypergeom_miss(indicators, k):
    """Exact P(all k sampled cells pass), sampling w/o replacement."""
    n = len(indicators)
    npass = sum(indicators)
    k = min(k, n)
    p = 1.0
    for i in range(k):
        p *= (npass - i) / (n - i)
        if p == 0.0:
            return 0.0
    return p


def analyze_file(path, rng):
    sheet = parse_sheet(path)
    if not sheet or not sheet["grid"]:
        return None
    grid = sheet["grid"]
    spans = column_spans(grid)
    parsed, excluded = [], {"no_text": 0, "unparseable": 0, "volatile": 0,
                            "no_cache": 0}
    for f in sheet["formulas"]:
        if f["value"] is EMPTY:
            excluded["no_cache"] += 1
            continue
        if not f["text"]:
            excluded["no_text"] += 1
            continue
        try:
            ast, meta = parse_formula(f["text"])
        except Unanalyzable:
            excluded["unparseable"] += 1
            continue
        if meta["volatile"]:
            excluded["volatile"] += 1
            continue
        parsed.append({"f": f, "ast": ast, "meta": meta,
                       "coords": None})
    for p in parsed:
        p["coords"] = formula_reads((p["ast"], p["meta"]))

    # ---- LEVEL A / M2: openpyxl insert@2, input-level pass indicators
    indicators = [m2_input_pass(grid, p["coords"]) for p in parsed]
    affected = [any(r >= 2 for (r, _c) in p["coords"]) for p in parsed]
    n_aff = sum(affected)
    pass_aff = sum(1 for i, a in zip(indicators, affected) if a and i)

    rec = {
        "n_formulas": len(sheet["formulas"]),
        "n_analyzable": len(parsed),
        "excluded": excluded,
        "mean_reads_per_formula": (round(statistics.mean(
            len(p["coords"]) for p in parsed), 2) if parsed else None),
        "m2_input": {
            "affected": n_aff,
            "pass_rate_affected": (pass_aff / n_aff) if n_aff else None,
            "pass_rate_all": (sum(indicators) / len(indicators))
            if indicators else None,
            # miss statistics are conditioned on AN ERROR BEING PRESENT:
            # a file with no affected read (no reference at/below the insert
            # row) is edited CORRECTLY by accident — nothing to detect.
            "full_check_miss": (all(indicators) if n_aff else None),
            "miss_k": ({k: hypergeom_miss(indicators, k) for k in KS}
                       if n_aff else None),
        },
    }

    # ---- LEVEL A / M1: single ref retargeted uniformly in its column span
    singles = [(p, n) for p in parsed for n in p["meta"]["refs"]]
    m1 = {"trials": 0, "input_collisions": 0}
    m1_detect = {k: [0, 0] for k in KS}   # detected, trials
    if singles and len(parsed) >= 2:
        for _ in range(M1_TRIALS):
            p, refnode = singles[rng.randrange(len(singles))]
            r, c = refnode[1], refnode[2]
            span = spans.get(c)
            if not span or span[1] - span[0] < 1:
                continue
            lo, hi = span
            wr = rng.randrange(lo, hi + 1)
            while wr == r:
                wr = rng.randrange(lo, hi + 1)
            coll = collide(grid.get((wr, c)), grid.get((r, c)))
            m1["trials"] += 1
            m1["input_collisions"] += coll
            bad_idx = parsed.index(p)
            for k in KS:
                kk = min(k, len(parsed))
                sample = rng.sample(range(len(parsed)), kk)
                m1_detect[k][1] += 1
                if bad_idx in sample and not coll:
                    m1_detect[k][0] += 1
    if m1["trials"]:
        rec["m1_input"] = {
            "trials": m1["trials"],
            "q_input": m1["input_collisions"] / m1["trials"],
            "detect_k_sampled": {k: d / t for k, (d, t) in m1_detect.items()
                                 if t},
            "coverage_bound_k_over_N": {k: min(k, len(parsed)) / len(parsed)
                                        for k in KS},
        }

    # ---- LEVEL B: evaluator, validation-gated, output vs input collision
    validated = []
    for p in parsed:
        m = p["meta"]
        if m["foreign"] or m["colrange"]:
            continue
        if any(fn not in SUPPORTED and not fn.startswith("RANGEPLACEHOLDER")
               for fn in m["unknown_funcs"]):
            continue
        try:
            res = eval_result(p["ast"], grid)
        except Unanalyzable:
            continue
        if matches_cache(res, p["f"]["value"]):
            validated.append((p, res))
    rec["evaluator_validated"] = len(validated)

    b_aff = b_in = b_out = 0
    for p, res0 in validated:
        if not any(r >= 2 for (r, _c) in p["coords"]):
            continue
        b_aff += 1
        inp = m2_input_pass(grid, p["coords"])
        try:
            res1 = eval_result(p["ast"], grid, mapper=m2_read)
        except Unanalyzable:
            b_aff -= 1
            continue
        out = results_equal(res0, res1)
        b_in += inp
        b_out += out
        # sanity: input collision must imply output collision
        assert not (inp and not out), "input-collide but output differs"
    rec["m2_output"] = {"affected_validated": b_aff,
                        "input_collision_rate": b_in / b_aff if b_aff else None,
                        "output_collision_rate": b_out / b_aff if b_aff else None}

    # LEVEL B / M1 output-level
    vs = [(p, n, res0) for (p, res0) in validated for n in p["meta"]["refs"]]
    t_b = in_b = out_b = 0
    if vs:
        for _ in range(M1_TRIALS):
            p, refnode, res0 = vs[rng.randrange(len(vs))]
            r, c = refnode[1], refnode[2]
            span = spans.get(c)
            if not span or span[1] - span[0] < 1:
                continue
            lo, hi = span
            wr = rng.randrange(lo, hi + 1)
            while wr == r:
                wr = rng.randrange(lo, hi + 1)
            try:
                res1 = eval_result(p["ast"], grid,
                                   override={id(refnode): (wr, c)})
            except Unanalyzable:
                continue
            t_b += 1
            in_b += collide(grid.get((wr, c)), grid.get((r, c)))
            out_b += results_equal(res0, res1)
    rec["m1_output"] = {"trials": t_b,
                        "input_collision_rate": in_b / t_b if t_b else None,
                        "output_collision_rate": out_b / t_b if t_b else None}
    return rec


# ------------------------------------------------- agent_ab ground truth ---

def agent_ab_confusion():
    """Level-A sheet-wide prediction vs LibreOffice verdict per A/B file.

    Also classifies each file by whether the openpyxl error is PRESENT at all
    (>=1 read whose target coordinate changes, i.e. a same-sheet reference to
    a row >= 2). Engine-faithful files WITH the error present are LATENT
    REFERENCE CORRUPTION that recomputes to identical values — invisible to a
    value check of any k (at least at the numeric cells the A/B oracle
    compared)."""
    with open(AGENT_AB) as fh:
        ab = json.load(fh)
    rows = []
    for e in ab["per_file"]:
        path = os.path.join(TESTS_ROOT, e["file"])
        if not os.path.exists(path):
            continue
        try:
            sheet = parse_sheet(path)
            grid = sheet["grid"]
            inds, inds_num, unanalyzable, n_aff = [], [], 0, 0
            for f in sheet["formulas"]:
                if not f["text"]:
                    unanalyzable += 1
                    continue
                try:
                    ast, meta = parse_formula(f["text"])
                except Unanalyzable:
                    unanalyzable += 1
                    continue
                if meta["volatile"]:
                    continue        # A/B oracle excluded these too
                coords = formula_reads((ast, meta))
                if any(r >= 2 for (r, _c) in coords):
                    n_aff += 1
                ok = m2_input_pass(grid, coords)
                inds.append(ok)
                if f["value"] is not EMPTY and f["value"][0] == "n":
                    inds_num.append(ok)   # what the A/B oracle could see
            pred = ("no_checkable" if not inds
                    else "pass" if all(inds) else "fail")
            pred_num = ("no_checkable" if not inds_num
                        else "pass" if all(inds_num) else "fail")
            rows.append({"file": e["file"], "predicted": pred,
                         "predicted_numeric_only": pred_num,
                         "affected_formulas": n_aff,
                         "unanalyzable": unanalyzable,
                         "engine": e["unguarded"]})
        except Exception as ex:
            rows.append({"file": e["file"], "predicted": "error",
                         "engine": e["unguarded"], "err": repr(ex)[:80]})
    cm = {"pred_fail_engine_corrupt": 0, "pred_pass_engine_faithful": 0,
          "pred_fail_engine_faithful": 0, "pred_pass_engine_corrupt": 0,
          "indeterminate": 0}
    mismatches = []
    for r in rows:
        eng = r["engine"]
        if r["predicted"] == "fail" and eng == "SILENT_CORRUPTION":
            cm["pred_fail_engine_corrupt"] += 1
        elif r["predicted"] == "pass" and eng == "faithful":
            cm["pred_pass_engine_faithful"] += 1
        elif r["predicted"] == "fail" and eng == "faithful":
            cm["pred_fail_engine_faithful"] += 1
            mismatches.append(r)
        elif r["predicted"] == "pass" and eng == "SILENT_CORRUPTION":
            cm["pred_pass_engine_corrupt"] += 1
            mismatches.append(r)
        else:
            cm["indeterminate"] += 1
    n_det = sum(v for k, v in cm.items() if k != "indeterminate")
    agree = cm["pred_fail_engine_corrupt"] + cm["pred_pass_engine_faithful"]

    # engine-ground-truth full-check miss rate, conditioned on error present
    err_present = [r for r in rows if r.get("affected_formulas", 0) > 0]
    latent = [r for r in err_present if r["engine"] == "faithful"]
    for r in latent:
        if r["predicted"] == "pass":
            r["latent_class"] = ("input_coincidence: every misread value "
                                 "equals the correct value — undetectable by "
                                 "ANY value comparison at any k")
        elif r["predicted_numeric_only"] == "fail":
            r["latent_class"] = ("output_noninjective: some numeric formula "
                                 "READS a different value, yet engine "
                                 "recompute matched every numeric cache")
        else:
            r["latent_class"] = ("oracle_blind: outputs are text/boolean; "
                                 "the numeric-only A/B oracle compared few "
                                 "or no cells — a text-aware check might "
                                 "still catch these")
    return {
        "files": len(rows), "confusion": cm,
        "agreement_rate": round(agree / n_det, 4) if n_det else None,
        "error_present_files(affected>=1)": len(err_present),
        "engine_verified_latent_corruption": {
            "count": len(latent),
            "full_value_check_miss_rate_by_engine_truth":
                round(len(latent) / len(err_present), 4) if err_present else None,
            "meaning": ("openpyxl left references pointing at WRONG cells, "
                        "yet LibreOffice's recompute matched every original "
                        "numeric cache — the wrong dependency graph is "
                        "value-invisible to a full (k=N) value check at the "
                        "cells the oracle compared. Caveat: the A/B oracle "
                        "compared numeric cells only; see latent_class."),
            "by_class": {
                "input_coincidence": sum(
                    1 for r in latent if r["latent_class"].startswith("input")),
                "output_noninjective": sum(
                    1 for r in latent if r["latent_class"].startswith("output")),
                "oracle_blind": sum(
                    1 for r in latent if r["latent_class"].startswith("oracle")),
            },
            "files": [{"file": r["file"], "class": r["latent_class"].split(":")[0],
                       "affected": r["affected_formulas"]} for r in latent],
        },
        "mismatches": mismatches[:20],
    }


# ----------------------------------------------------------------- main ---

def main():
    rng = random.Random(SEED)
    # eligible: >=10 analyzable formulas; even spread over sorted paths
    eligible = []
    for path in CORPUS:
        try:
            sheet = parse_sheet(path)
        except Exception:
            continue
        if not sheet or not sheet["grid"]:
            continue
        n = 0
        for f in sheet["formulas"]:
            if not f["text"] or f["value"] is EMPTY:
                continue
            try:
                _, meta = parse_formula(f["text"])
            except Unanalyzable:
                continue
            if not meta["volatile"]:
                n += 1
        if n >= 10:
            eligible.append(path)
    stride = max(1, len(eligible) // N_MC_FILES)
    chosen = eligible[::stride][:N_MC_FILES]

    per_file, results = [], {}
    for path in chosen:
        rel = os.path.relpath(path, TESTS_ROOT)
        rec = analyze_file(path, rng)
        if rec:
            rec["file"] = rel
            per_file.append(rec)

    # ---- aggregate M2 input-level k-check miss (mean over files, exact)
    # only over files where the error is PRESENT (>=1 affected read)
    m2_files = [f for f in per_file if f["m2_input"]["miss_k"]]
    agg_miss = {k: statistics.mean(f["m2_input"]["miss_k"][k]
                                   for f in m2_files) for k in KS}
    q_aff = [f["m2_input"]["pass_rate_affected"] for f in m2_files
             if f["m2_input"]["pass_rate_affected"] is not None]
    full_miss = [f["m2_input"]["full_check_miss"] for f in m2_files]

    # analytic comparison: mixture over the SAME files using per-file
    # M2v adjacent-pair rates from coincidence_q.json
    with open("/home/soh/aix/benchmarks/coincidence_q.json") as fh:
        qj = json.load(fh)
    pooled_q_m2v = qj["models"]["M2v_offbyone_row"]["excel_semantics"]["pooled_q"]

    results["m2_openpyxl_insert2"] = {
        "files_with_error_present": len(m2_files),
        "files_no_error(no affected read; excluded from miss stats)":
            len(per_file) - len(m2_files),
        "per_formula_pass_rate_affected": {
            "mean": round(statistics.mean(q_aff), 4),
            "median": round(statistics.median(q_aff), 4),
            "p90": round(sorted(q_aff)[int(0.9 * (len(q_aff) - 1))], 4),
        },
        "mc_miss_k_mean_over_files": {k: round(v, 5)
                                      for k, v in agg_miss.items()},
        "analytic_naive_qk_pooled_M2v": {k: round(pooled_q_m2v ** k, 5)
                                         for k in KS},
        "full_check_miss_rate(error-present files where ALL cells pass, "
        "input-level)": round(sum(full_miss) / len(full_miss), 4),
        "note": ("mc_miss uses EXACT hypergeometric per file over real "
                 "per-formula indicators (real shared-input and value-"
                 "repetition dependence), then averages over files — the "
                 "empirical counterpart of the mixture bound."),
    }

    # ---- M1 aggregate
    m1_files = [f for f in per_file if "m1_input" in f]
    if m1_files:
        pooled_trials = sum(f["m1_input"]["trials"] for f in m1_files)
        pooled_coll = sum(f["m1_input"]["q_input"] * f["m1_input"]["trials"]
                          for f in m1_files)
        results["m1_single_ref_retarget"] = {
            "files": len(m1_files),
            "trials": pooled_trials,
            "q_input_pooled": round(pooled_coll / pooled_trials, 4),
            "q_input_file_mean": round(statistics.mean(
                f["m1_input"]["q_input"] for f in m1_files), 4),
            "q_input_file_median": round(statistics.median(
                f["m1_input"]["q_input"] for f in m1_files), 4),
            "detect_k_sampled_mean": {
                k: round(statistics.mean(
                    f["m1_input"]["detect_k_sampled"][k] for f in m1_files
                    if k in f["m1_input"]["detect_k_sampled"]), 4)
                for k in KS},
            "coverage_bound_mean_k_over_N": {
                k: round(statistics.mean(
                    f["m1_input"]["coverage_bound_k_over_N"][k]
                    for f in m1_files), 4) for k in KS},
            "note": ("a single-reference error corrupts ONE cell (plus "
                     "dependents); a k-of-N sampled check detects it with "
                     "P <= k/N REGARDLESS of q — coverage, not collision, "
                     "is binding."),
        }

    # ---- Level B aggregate
    bo = [f for f in per_file if f["m2_output"]["affected_validated"] >= 5]
    if bo:
        results["m2_output_level"] = {
            "files": len(bo),
            "validated_formulas_total": sum(f["evaluator_validated"]
                                            for f in per_file),
            "affected_validated_total": sum(
                f["m2_output"]["affected_validated"] for f in bo),
            "input_collision_rate_mean": round(statistics.mean(
                f["m2_output"]["input_collision_rate"] for f in bo), 4),
            "output_collision_rate_mean": round(statistics.mean(
                f["m2_output"]["output_collision_rate"] for f in bo), 4),
        }
    b1 = [f for f in per_file if f["m1_output"]["trials"] >= 50]
    if b1:
        results["m1_output_level"] = {
            "files": len(b1),
            "trials": sum(f["m1_output"]["trials"] for f in b1),
            "input_collision_rate_mean": round(statistics.mean(
                f["m1_output"]["input_collision_rate"] for f in b1), 4),
            "output_collision_rate_mean": round(statistics.mean(
                f["m1_output"]["output_collision_rate"] for f in b1), 4),
            "note": ("output >= input is the injectivity gap: non-injective "
                     "formulas pass the value check even when they read a "
                     "DIFFERENT value."),
        }

    # ---- ground truth anchor
    results["agent_ab_ground_truth_anchor"] = agent_ab_confusion()

    out = {
        "experiment": ("COINCIDENCE MC: simulate reference errors on real "
                       "workbooks; empirical miss rate of a k-cell value "
                       "check vs the analytic bound and vs LibreOffice "
                       "engine ground truth (agent_ab.json)"),
        "seed": SEED,
        "eligible_files": len(eligible),
        "mc_files": len(per_file),
        "m1_trials_per_file": M1_TRIALS,
        "results": results,
        "what_level_A_measures": (
            "input-level miss (misread value == correct value), per the "
            "derivation's q; understates per-cell miss for non-injective "
            "formulas and understates detection via propagation from "
            "unchecked cells. Sheet-wide input-collision => the edit passes "
            "ANY check (induction over dependency order)."),
        "per_file": per_file,
    }
    with open(OUT, "w") as fh:
        json.dump(out, fh, indent=1)
    print(json.dumps({k: v for k, v in results.items()
                      if k != "agent_ab_ground_truth_anchor"}, indent=1))
    aa = results["agent_ab_ground_truth_anchor"]
    print("agent_ab anchor:", json.dumps(
        {k: aa[k] for k in ("files", "confusion", "agreement_rate")}, indent=1))


if __name__ == "__main__":
    main()
