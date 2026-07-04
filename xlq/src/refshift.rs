//! The reference-shift algebra σ for surgical structural edits (Phase 3 PoC).
//!
//! Given a structural edit (insert/delete n rows/cols at index k on a named
//! sheet), σ maps any A1 reference to its shifted form, or `#REF!` when the
//! reference is entirely consumed by a delete. This is the corrected algebra
//! from research-log/013 (theory-review gate FAIL → fixed): independent
//! per-endpoint insert, the 6-case delete clamp, sheet scoping, absolute-flag
//! transparency, and axis selectivity. It is the intellectual core of the
//! structural-edit contribution; correctness is validated by the unit tests
//! below, which encode Excel's documented insert/delete semantics.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Insert,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Col,
}

/// A structural edit: `op` `count` lines on `axis`, starting at 1-based `at`,
/// on sheet `sheet`.
#[derive(Clone, Debug)]
pub struct StructuralEdit {
    pub axis: Axis,
    pub at: u32,    // k, 1-based
    pub count: u32, // n
    pub op: Op,
    pub sheet: String, // the edited sheet
}

/// Outcome of shifting one reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shift {
    /// Reference is out of scope or unaffected — leave byte-identical.
    Unchanged,
    /// Reference shifts to a new textual form.
    Shifted(String),
    /// Reference is entirely consumed by a delete → `#REF!`.
    Ref,
}

/// Shift a single 1-based line index (row number or column number) on the
/// edit's axis. Returns `Some(new_index)` or `None` if that single line is
/// consumed by a delete.
fn shift_index(pos: u32, edit: &StructuralEdit) -> Option<u32> {
    let (k, n) = (edit.at, edit.count);
    match edit.op {
        Op::Insert => Some(if pos >= k { pos + n } else { pos }),
        Op::Delete => {
            if pos < k {
                Some(pos)
            } else if pos >= k + n {
                Some(pos - n)
            } else {
                None // inside deleted band
            }
        }
    }
}

/// The 6-case clamp for a [head, tail] endpoint pair on the edit's axis.
/// Returns `Some((h', t'))` or `None` if the whole range is consumed (#REF!).
fn shift_span(head: u32, tail: u32, edit: &StructuralEdit) -> Option<(u32, u32)> {
    let (k, n) = (edit.at, edit.count);
    match edit.op {
        Op::Insert => {
            // Independent per-endpoint (C2): reproduces grow/shift/asymmetry.
            let h = if head >= k { head + n } else { head };
            let t = if tail >= k { tail + n } else { tail };
            Some((h, t))
        }
        Op::Delete => {
            let band_end = k + n; // exclusive
            let head_below = head < k;
            let tail_below = tail < k;
            let head_after = head >= band_end;
            let tail_after = tail >= band_end;
            if tail_below {
                Some((head, tail)) // both < k: unchanged
            } else if head_after {
                Some((head - n, tail - n)) // both >= k+n: shift up
            } else if head_below && tail_after {
                Some((head, tail - n)) // straddle: shrink, head fixed
            } else if !head_below && !head_after && tail_after {
                Some((k, tail - n)) // head in band → clamp head to k
            } else if head_below && !tail_below && !tail_after {
                Some((head, k - 1)) // tail in band → clamp tail to k-1
            } else {
                None // k <= head <= tail < k+n: entirely consumed → #REF!
            }
        }
    }
}

/// Column letters → 1-based number. "A"→1, "Z"→26, "AA"→27.
pub fn col_to_num(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for c in s.bytes() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        n = n.checked_mul(26)?.checked_add((c.to_ascii_uppercase() - b'A' + 1) as u32)?;
    }
    Some(n)
}

/// 1-based column number → letters.
pub fn num_to_col(mut n: u32) -> String {
    let mut s = Vec::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        s.push(b'A' + rem);
        n = (n - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// Parse one A1 endpoint like `$A$5`, `A5`, `$A5`, `A`, `5`, `$A`, `$5`.
/// Returns (col_abs, col_num_opt, row_abs, row_num_opt). A None col means a
/// whole-row ref (e.g. `5`); a None row means a whole-column ref (e.g. `A`).
fn parse_endpoint(s: &str) -> Option<(bool, Option<u32>, bool, Option<u32>)> {
    let b = s.as_bytes();
    let mut i = 0;
    let col_abs = i < b.len() && b[i] == b'$';
    if col_abs {
        i += 1;
    }
    let col_start = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    let col = if i > col_start {
        Some(col_to_num(&s[col_start..i])?)
    } else {
        None
    };
    let row_abs = i < b.len() && b[i] == b'$';
    if row_abs {
        i += 1;
    }
    let row_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let row = if i > row_start {
        Some(s[row_start..i].parse::<u32>().ok()?)
    } else {
        None
    };
    if i != b.len() {
        return None; // trailing garbage → not a clean A1 endpoint
    }
    if col.is_none() && row.is_none() {
        return None;
    }
    // reject col_abs with no col, or row_abs with no row
    if (col_abs && col.is_none()) || (row_abs && row.is_none()) {
        return None;
    }
    Some((col_abs, col, row_abs, row))
}

fn fmt_endpoint(col_abs: bool, col: Option<u32>, row_abs: bool, row: Option<u32>) -> String {
    let mut s = String::new();
    if let Some(c) = col {
        if col_abs {
            s.push('$');
        }
        s.push_str(&num_to_col(c));
    }
    if let Some(r) = row {
        if row_abs {
            s.push('$');
        }
        s.push_str(&r.to_string());
    }
    s
}

/// Shift a bare (sheet-stripped) A1 reference body: a single endpoint or a
/// `head:tail` range. Returns the new body, or `#REF!`, or None if unchanged.
fn shift_body(body: &str, edit: &StructuralEdit) -> Shift {
    if let Some((h, t)) = body.split_once(':') {
        let hp = parse_endpoint(h);
        let tp = parse_endpoint(t);
        let (hp, tp) = match (hp, tp) {
            (Some(a), Some(b)) => (a, b),
            _ => return Shift::Unchanged, // not a clean A1 range (opaque)
        };
        // Extract the axis line for head/tail; None means the ref is whole on
        // the OTHER axis and this op does not touch it.
        let (h_line, t_line) = match edit.axis {
            Axis::Row => (hp.3, tp.3),
            Axis::Col => (hp.1, tp.1),
        };
        match (h_line, t_line) {
            (Some(hl), Some(tl)) => {
                let (lo, hi) = if hl <= tl { (hl, tl) } else { (tl, hl) };
                match shift_span(lo, hi, edit) {
                    None => Shift::Ref,
                    Some((nl, nh)) => {
                        if nl == lo && nh == hi {
                            return Shift::Unchanged;
                        }
                        // rebuild endpoints preserving col/abs, replacing axis line
                        let (nh_ep, nt_ep) = rebuild_range(hp, tp, edit.axis, nl, nh, hl <= tl);
                        Shift::Shifted(format!("{}:{}", nh_ep, nt_ep))
                    }
                }
            }
            _ => Shift::Unchanged, // axis-orthogonal whole ref (e.g. A:A under a row op)
        }
    } else {
        let ep = match parse_endpoint(body) {
            Some(e) => e,
            None => return Shift::Unchanged,
        };
        let line = match edit.axis {
            Axis::Row => ep.3,
            Axis::Col => ep.1,
        };
        match line {
            None => Shift::Unchanged, // axis-orthogonal
            Some(l) => match shift_index(l, edit) {
                None => Shift::Ref,
                Some(nl) => {
                    if nl == l {
                        Shift::Unchanged
                    } else {
                        let (col_abs, col, row_abs, row) = ep;
                        let ne = match edit.axis {
                            Axis::Row => fmt_endpoint(col_abs, col, row_abs, Some(nl)),
                            Axis::Col => fmt_endpoint(col_abs, Some(nl), row_abs, row),
                        };
                        Shift::Shifted(ne)
                    }
                }
            },
        }
    }
}

fn rebuild_range(
    hp: (bool, Option<u32>, bool, Option<u32>),
    tp: (bool, Option<u32>, bool, Option<u32>),
    axis: Axis,
    new_lo: u32,
    new_hi: u32,
    head_was_lo: bool,
) -> (String, String) {
    // assign new axis lines back to head/tail preserving original order
    let (h_new, t_new) = if head_was_lo { (new_lo, new_hi) } else { (new_hi, new_lo) };
    let head = set_axis(hp, axis, h_new);
    let tail = set_axis(tp, axis, t_new);
    (head, tail)
}

fn set_axis(ep: (bool, Option<u32>, bool, Option<u32>), axis: Axis, line: u32) -> String {
    let (col_abs, col, row_abs, row) = ep;
    match axis {
        Axis::Row => fmt_endpoint(col_abs, col, row_abs, Some(line)),
        Axis::Col => fmt_endpoint(col_abs, Some(line), row_abs, row),
    }
}

/// The scoped shift: given a reference that may carry a sheet qualifier, and
/// the sheet the reference LIVES on (`current_sheet`), apply σ only if the
/// reference targets the edited sheet. External `[n]…` refs never shift.
pub fn shift_ref(reference: &str, current_sheet: &str, edit: &StructuralEdit) -> Shift {
    // external workbook ref like [1]Sheet!A1 → never shift
    if reference.starts_with('[') {
        return Shift::Unchanged;
    }
    // split optional sheet qualifier at the LAST '!' (sheet names may be quoted)
    if let Some(bang) = reference.rfind('!') {
        let (sheet_part, body) = reference.split_at(bang);
        let body = &body[1..];
        let target = unquote_sheet(sheet_part);
        // 3D span Sheet1:Sheet3 — PoC: shift if edited sheet is named endpoint
        let targets_edit = if let Some((s1, s2)) = target.split_once(':') {
            eq_sheet(s1, &edit.sheet) || eq_sheet(s2, &edit.sheet)
        } else {
            eq_sheet(&target, &edit.sheet)
        };
        if !targets_edit {
            return Shift::Unchanged;
        }
        match shift_body(body, edit) {
            Shift::Unchanged => Shift::Unchanged,
            Shift::Ref => Shift::Ref,
            Shift::Shifted(nb) => Shift::Shifted(format!("{}!{}", sheet_part, nb)),
        }
    } else {
        // unqualified → lives on current_sheet; shift only if that is edited
        if !eq_sheet(current_sheet, &edit.sheet) {
            return Shift::Unchanged;
        }
        shift_body(reference, edit)
    }
}

fn unquote_sheet(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        s[1..s.len() - 1].replace("''", "'")
    } else {
        s.to_string()
    }
}

fn eq_sheet(a: &str, b: &str) -> bool {
    unquote_sheet(a).eq_ignore_ascii_case(&unquote_sheet(b))
}

/// Apply σ to every A1 reference inside a formula body, leaving string
/// literals, function names, defined names, and structured table refs
/// untouched. `current_sheet` is the sheet the formula lives on (for scoping
/// unqualified refs). Returns (new_formula, number_of_references_shifted).
///
/// The scanner walks the formula, skips `"..."` string literals (with `""`
/// escapes), and at each reference-candidate start (optional `'sheet'!` /
/// `sheet!` / `[n]sheet!` qualifier, then a `$?COL$?ROW` / range / whole-row /
/// whole-column body) tries to parse a reference. A candidate is only treated
/// as a reference if it carries a row number, or is an explicit column range
/// (`A:A`) — a bare identifier (function name / defined name) is left alone.
pub fn shift_formula(formula: &str, current_sheet: &str, edit: &StructuralEdit) -> (String, u32) {
    let b = formula.as_bytes();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    let mut shifted = 0u32;
    while i < b.len() {
        let c = b[i];
        // string literal — copy verbatim
        if c == b'"' {
            out.push('"');
            i += 1;
            while i < b.len() {
                out.push(b[i] as char);
                if b[i] == b'"' {
                    i += 1;
                    if i < b.len() && b[i] == b'"' {
                        out.push('"');
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            continue;
        }
        // a reference candidate begins at a sheet qualifier or a column letter
        // or a '$' or a digit (whole-row like 5:5) — but only if the previous
        // emitted char isn't part of an identifier/number (so we don't grab the
        // digits of a numeric literal or the tail of a name).
        let prev = out.chars().last();
        let boundary = match prev {
            None => true,
            Some(p) => !(p.is_ascii_alphanumeric() || p == '_' || p == '.' || p == '$' || p == '!' || p == '\''),
        };
        if boundary && (c == b'\'' || c == b'[' || c.is_ascii_alphabetic() || c == b'$' || c.is_ascii_digit()) {
            if let Some((tok_len, replacement, did_shift)) =
                try_reference(&formula[i..], current_sheet, edit)
            {
                out.push_str(&replacement);
                if did_shift {
                    shifted += 1;
                }
                i += tok_len;
                continue;
            }
        }
        out.push(c as char);
        i += 1;
    }
    (out, shifted)
}

/// Try to parse a (possibly sheet-qualified) reference at the start of `s`.
/// Returns (consumed_bytes, replacement_text, did_shift) or None if not a ref.
fn try_reference(s: &str, current_sheet: &str, edit: &StructuralEdit) -> Option<(usize, String, bool)> {
    let b = s.as_bytes();
    let mut i = 0;
    // optional external prefix [n]
    if i < b.len() && b[i] == b'[' {
        let close = s[i..].find(']')? + i;
        i = close + 1;
    }
    // optional sheet qualifier: 'quoted'! or bare! (possibly 3D a:b)
    let qual_end;
    if i < b.len() && b[i] == b'\'' {
        // quoted sheet name, may contain '' escapes, ends at ' then !
        let mut j = i + 1;
        loop {
            if j >= b.len() {
                return None;
            }
            if b[j] == b'\'' {
                if j + 1 < b.len() && b[j + 1] == b'\'' {
                    j += 2;
                    continue;
                }
                break;
            }
            j += 1;
        }
        // j at closing quote; need '!' after
        if j + 1 < b.len() && b[j + 1] == b'!' {
            qual_end = Some(j + 2);
        } else {
            return None; // quoted thing not a sheet qualifier → not our ref
        }
    } else {
        // bare identifier(s) then '!'
        let mut j = i;
        while j < b.len()
            && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'.' || b[j] == b':')
        {
            j += 1;
        }
        if j < b.len() && b[j] == b'!' && j > i {
            qual_end = Some(j + 1);
        } else {
            qual_end = None;
        }
    }
    let body_start = qual_end.unwrap_or(i);
    // parse the body: endpoint or endpoint:endpoint
    let (body_len, is_ref) = scan_ref_body(&s[body_start..]);
    if !is_ref || body_len == 0 {
        return None;
    }
    let total = body_start + body_len;
    let full = &s[..total];
    match shift_ref(full, current_sheet, edit) {
        Shift::Unchanged => Some((total, full.to_string(), false)),
        Shift::Shifted(ns) => Some((total, ns, true)),
        Shift::Ref => {
            // rebuild with the qualifier preserved, body → #REF!
            let qual = &s[..body_start];
            Some((total, format!("{}#REF!", qual), true))
        }
    }
}

/// Scan a reference body (no sheet qualifier). Returns (consumed_len, is_ref).
/// Accepts A1, A1:B2, $A$1, A:A, 5:5, $A:$C. Rejects bare identifiers with no
/// row number and no ':' (function/name).
fn scan_ref_body(s: &str) -> (usize, bool) {
    fn scan_endpoint(s: &str) -> (usize, bool, bool) {
        // returns (len, has_col, has_row)
        let b = s.as_bytes();
        let mut i = 0;
        if i < b.len() && b[i] == b'$' {
            i += 1;
        }
        let col_start = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        let has_col = i > col_start;
        let mut had_row_dollar = false;
        if i < b.len() && b[i] == b'$' {
            had_row_dollar = true;
            i += 1;
        }
        let row_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let has_row = i > row_start;
        if had_row_dollar && !has_row {
            // trailing $ without row → back off the $
            i -= 1;
        }
        (i, has_col, has_row)
    }
    let (l1, c1, r1) = scan_endpoint(s);
    if l1 == 0 {
        return (0, false);
    }
    // range?
    if s.as_bytes().get(l1) == Some(&b':') {
        let (l2, c2, r2) = scan_endpoint(&s[l1 + 1..]);
        if l2 > 0 {
            // valid range if both endpoints parse; whole-row (rows only) or
            // whole-col (cols only) or full cells
            let total = l1 + 1 + l2;
            let ok = (r1 || !c1) && (r2 || !c2); // each endpoint is col-only or has row
            // require columns match kind: A:A (col-only) or 1:1 (row-only) or A1:B2
            let coherent = (c1 == c2) || (r1 && r2);
            return (total, ok && coherent);
        }
    }
    // single endpoint: a real cell ref needs a row number (else it's a name)
    (l1, c1 && r1)
}

/// Residual detection: does this formula body use a construct the minimal-patch
/// invariant cannot preserve by token surgery? Returns the reason, or None.
pub fn residual_reason(formula_attrs: &str) -> Option<&'static str> {
    // shared/array formula stubs and INDIRECT/OFFSET text refs
    if formula_attrs.contains("t=\"array\"") {
        return Some("array_formula_spanning_edit");
    }
    if formula_attrs.contains("t=\"shared\"") {
        return Some("shared_formula_interior_crossed");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_edit(op: Op, at: u32, count: u32) -> StructuralEdit {
        StructuralEdit { axis: Axis::Row, at, count, op, sheet: "Sheet1".into() }
    }
    fn col_edit(op: Op, at: u32, count: u32) -> StructuralEdit {
        StructuralEdit { axis: Axis::Col, at, count, op, sheet: "Sheet1".into() }
    }
    fn s(x: &str) -> Shift {
        Shift::Shifted(x.into())
    }

    // ---- col letter round-trip ----
    #[test]
    fn col_letters() {
        assert_eq!(col_to_num("A"), Some(1));
        assert_eq!(col_to_num("Z"), Some(26));
        assert_eq!(col_to_num("AA"), Some(27));
        assert_eq!(num_to_col(1), "A");
        assert_eq!(num_to_col(27), "AA");
        assert_eq!(num_to_col(702), "ZZ");
    }

    // ---- INSERT: per-endpoint (grow vs shift vs asymmetry) ----
    #[test]
    fn insert_below_shifts_whole_range() {
        // insert 1 row at k=2; A5:A10 both >= 2 → shift down to A6:A11
        assert_eq!(shift_body("A5:A10", &row_edit(Op::Insert, 2, 1)), s("A6:A11"));
    }
    #[test]
    fn insert_inside_grows_range() {
        // insert 1 at k=7; A5:A10 straddles (head 5<7<=tail 10) → grow tail: A5:A11
        assert_eq!(shift_body("A5:A10", &row_edit(Op::Insert, 7, 1)), s("A5:A11"));
    }
    #[test]
    fn insert_above_whole_range_unchanged() {
        // insert 1 at k=20; A5:A10 entirely above → unchanged
        assert_eq!(shift_body("A5:A10", &row_edit(Op::Insert, 20, 1)), Shift::Unchanged);
    }
    #[test]
    fn insert_at_head_boundary_shifts_not_grows() {
        // insert at k == head (5): head>=k → both shift → A6:A11 (range moves, blank excluded at top)
        assert_eq!(shift_body("A5:A10", &row_edit(Op::Insert, 5, 1)), s("A6:A11"));
    }
    #[test]
    fn insert_at_tail_boundary_grows() {
        // insert at k == tail (10): head 5<10 fixed, tail 10>=10 shifts → grow (blank included at bottom)
        assert_eq!(shift_body("A5:A10", &row_edit(Op::Insert, 10, 1)), s("A5:A11"));
    }

    // ---- DELETE: the 6-case clamp (the theory-review FAIL) ----
    #[test]
    fn delete_clip_head_endpoint() {
        // =SUM(A5:A10) delete rows 5-6 → SUM(A5:A8): clamp head to k=5, tail 10-2=8
        assert_eq!(shift_body("A5:A10", &row_edit(Op::Delete, 5, 2)), s("A5:A8"));
    }
    #[test]
    fn delete_straddle_shrinks() {
        // =SUM(A3:A10) delete rows 5-6 → head 3<5 fixed, tail 10-2=8 → A3:A8
        assert_eq!(shift_body("A3:A10", &row_edit(Op::Delete, 5, 2)), s("A3:A8"));
    }
    #[test]
    fn delete_head_in_band_clamps_to_k() {
        // =SUM(A5:A10) delete rows 3-6 (k=3,n=4,band[3,7)) → head 5 in band→clamp to 3, tail 10-4=6 → A3:A6
        assert_eq!(shift_body("A5:A10", &row_edit(Op::Delete, 3, 4)), s("A3:A6"));
    }
    #[test]
    fn delete_entirely_consumed_is_ref() {
        // =SUM(A5:A6) delete rows 3-8 (band[3,9)) → fully consumed → #REF!
        assert_eq!(shift_body("A5:A6", &row_edit(Op::Delete, 3, 6)), Shift::Ref);
    }
    #[test]
    fn delete_single_cell_in_band_is_ref() {
        assert_eq!(shift_body("A5", &row_edit(Op::Delete, 5, 1)), Shift::Ref);
    }
    #[test]
    fn delete_below_shifts_up() {
        assert_eq!(shift_body("A10", &row_edit(Op::Delete, 5, 2)), s("A8"));
    }
    #[test]
    fn delete_above_unchanged() {
        assert_eq!(shift_body("A3", &row_edit(Op::Delete, 5, 2)), Shift::Unchanged);
    }
    #[test]
    fn delete_tail_in_band_clamps_to_k_minus_1() {
        // A3:A6 delete rows 5-8 (band[5,9)) → head 3<5 fixed, tail 6 in band → clamp to k-1=4 → A3:A4
        assert_eq!(shift_body("A3:A6", &row_edit(Op::Delete, 5, 4)), s("A3:A4"));
    }

    // ---- absolute-flag transparency (C3, CONFIRMED) ----
    #[test]
    fn absolute_ref_still_shifts_structurally() {
        // $A$5 insert 1 at k=5 → $A$6 ($ does not exempt from structural shift)
        assert_eq!(shift_body("$A$5", &row_edit(Op::Insert, 5, 1)), s("$A$6"));
    }
    #[test]
    fn mixed_ref_preserves_dollars() {
        assert_eq!(shift_body("$A5:B$10", &row_edit(Op::Insert, 2, 3)), s("$A8:B$13"));
    }

    // ---- axis selectivity + whole-row/col forms (C7) ----
    #[test]
    fn row_op_leaves_whole_column_ref_untouched() {
        // A:A under a ROW op → unchanged (no row component)
        assert_eq!(shift_body("A:A", &row_edit(Op::Insert, 5, 1)), Shift::Unchanged);
    }
    #[test]
    fn row_op_shifts_whole_row_ref() {
        // 5:5 under a row insert at k=2 → 6:6
        assert_eq!(shift_body("5:5", &row_edit(Op::Insert, 2, 1)), s("6:6"));
    }
    #[test]
    fn col_op_shifts_columns_only() {
        // B5:D10 insert 1 col at k=3 (col C) → B fixed(<C? B=2<3 yes)… D=4>=3→E → B5:E10
        assert_eq!(shift_body("B5:D10", &col_edit(Op::Insert, 3, 1)), s("B5:E10"));
    }
    #[test]
    fn col_op_leaves_whole_row_ref_untouched() {
        assert_eq!(shift_body("5:5", &col_edit(Op::Insert, 3, 1)), Shift::Unchanged);
    }

    // ---- scoping (C4): only the edited sheet shifts ----
    #[test]
    fn same_sheet_unqualified_shifts() {
        let e = row_edit(Op::Insert, 5, 1); // sheet Sheet1
        assert_eq!(shift_ref("A5", "Sheet1", &e), s("A6"));
    }
    #[test]
    fn other_sheet_formula_unqualified_does_not_shift() {
        // formula lives on Sheet2, edit is on Sheet1 → unqualified ref is Sheet2-local → unchanged
        let e = row_edit(Op::Insert, 5, 1);
        assert_eq!(shift_ref("A5", "Sheet2", &e), Shift::Unchanged);
    }
    #[test]
    fn cross_sheet_ref_to_edited_sheet_shifts() {
        let e = row_edit(Op::Insert, 5, 1);
        assert_eq!(shift_ref("Sheet1!A5", "Sheet2", &e), s("Sheet1!A6"));
    }
    #[test]
    fn cross_sheet_ref_to_other_sheet_unchanged() {
        let e = row_edit(Op::Insert, 5, 1);
        assert_eq!(shift_ref("Sheet3!A5", "Sheet2", &e), Shift::Unchanged);
    }
    #[test]
    fn external_ref_never_shifts() {
        let e = row_edit(Op::Insert, 5, 1);
        assert_eq!(shift_ref("[1]Sheet1!A5", "Sheet1", &e), Shift::Unchanged);
    }
    #[test]
    fn quoted_sheet_name_scoping() {
        let mut e = row_edit(Op::Insert, 5, 1);
        e.sheet = "My Sheet".into();
        assert_eq!(shift_ref("'My Sheet'!A5", "Other", &e), s("'My Sheet'!A6"));
    }
    #[test]
    fn threeD_span_including_edited_sheet_shifts() {
        // Sheet1:Sheet3!A5 with edit on Sheet1 (an endpoint) → shift
        let e = row_edit(Op::Insert, 5, 1);
        assert_eq!(shift_ref("Sheet1:Sheet3!A5", "Sheet2", &e), s("Sheet1:Sheet3!A6"));
    }

    // ---- formula tokenizer (shift_formula) ----
    fn sf(f: &str, sheet: &str, e: &StructuralEdit) -> String {
        shift_formula(f, sheet, e).0
    }
    #[test]
    fn formula_shifts_simple_range() {
        assert_eq!(sf("SUM(A5:A10)", "Sheet1", &row_edit(Op::Insert, 2, 1)), "SUM(A6:A11)");
    }
    #[test]
    fn formula_leaves_function_names_alone() {
        // SUM parses as col letters but has no row and no ':' → not a ref
        assert_eq!(sf("SUM(A5)+MAX(B2)", "Sheet1", &row_edit(Op::Insert, 100, 1)), "SUM(A5)+MAX(B2)");
    }
    #[test]
    fn formula_does_not_touch_string_literals() {
        // the "A5" inside the quotes must NOT shift
        assert_eq!(
            sf(r#"IF(A5>0,"row A5 here",B10)"#, "Sheet1", &row_edit(Op::Insert, 2, 1)),
            r#"IF(A6>0,"row A5 here",B11)"#
        );
    }
    #[test]
    fn formula_shifts_cross_sheet_and_scopes() {
        // edit on Sheet1; Sheet1!A5 shifts, Sheet2!B10 (other sheet) does not
        assert_eq!(
            sf("Sheet1!A5+Sheet2!B10", "SheetX", &row_edit(Op::Insert, 3, 1)),
            "Sheet1!A6+Sheet2!B10"
        );
    }
    #[test]
    fn formula_preserves_absolute_and_mixed() {
        assert_eq!(sf("$A$5+B$10", "Sheet1", &row_edit(Op::Insert, 5, 2)), "$A$7+B$12");
    }
    #[test]
    fn formula_delete_produces_ref_error() {
        assert_eq!(sf("A5+B10", "Sheet1", &row_edit(Op::Delete, 5, 1)), "#REF!+B9");
    }
    #[test]
    fn formula_indirect_text_arg_not_shifted() {
        // INDIRECT's "A5" is a string → untouched; the bare B10 shifts
        assert_eq!(
            sf(r#"INDIRECT("A5")+B10"#, "Sheet1", &row_edit(Op::Insert, 2, 1)),
            r#"INDIRECT("A5")+B11"#
        );
    }
    #[test]
    fn formula_whole_column_under_row_op_unchanged() {
        assert_eq!(sf("SUM(A:A)", "Sheet1", &row_edit(Op::Insert, 5, 1)), "SUM(A:A)");
    }
    #[test]
    fn formula_quoted_sheet() {
        let mut e = row_edit(Op::Insert, 5, 1);
        e.sheet = "My Sheet".into();
        assert_eq!(sf("'My Sheet'!A5*2", "Other", &e), "'My Sheet'!A6*2");
    }
    #[test]
    fn formula_counts_shifts() {
        let (nf, n) = shift_formula("A5+A6+A100", "Sheet1", &row_edit(Op::Insert, 50, 1));
        assert_eq!(nf, "A5+A6+A101");
        assert_eq!(n, 1); // only A100 shifted
    }

    // ---- residual detection ----
    #[test]
    fn detects_array_and_shared_residual() {
        assert_eq!(residual_reason("t=\"array\" ref=\"C2:C10\""), Some("array_formula_spanning_edit"));
        assert_eq!(residual_reason("t=\"shared\" ref=\"B2:B100\" si=\"0\""), Some("shared_formula_interior_crossed"));
        assert_eq!(residual_reason(""), None);
    }
}
