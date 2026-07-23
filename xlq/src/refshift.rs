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
    /// Relocate a contiguous block of `count` lines starting at `at` to *before*
    /// the (original-coordinate) line `dest`. Only the Row axis is supported.
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Col,
}

/// A structural edit: `op` `count` lines on `axis`, starting at 1-based `at`,
/// on sheet `sheet`. `dest` is the 1-based ORIGINAL-coordinate destination for
/// `Op::Move` (relocate the block to *before* line `dest`); it is unused (0) for
/// insert/delete.
#[derive(Clone, Debug)]
pub struct StructuralEdit {
    pub axis: Axis,
    pub at: u32,    // k, 1-based
    pub count: u32, // n
    pub op: Op,
    pub sheet: String, // the edited sheet
    pub dest: u32,     // b, 1-based (Move only; 0 for insert/delete)
}

/// The Move permutation σ on a 1-based line `pos`: relocate the block
/// `[a, a+n)` (a = `at`, n = `count`) to *before* line `b` = `dest` (b in
/// ORIGINAL coordinates). σ is a bijection on `[1, maxrow]` and preserves order
/// within the moved block (proven in `formal/shift_laws.py`).
///
/// * `a <= b <= a+n`  → identity (moving within/adjacent to itself — a no-op).
/// * `b > a+n` (down) → block lands on `[b-n, b)`, the gap `[a+n, b)` shifts up.
/// * `b < a`   (up)   → block lands on `[b, b+n)`, the gap `[b, a)` shifts down.
///
/// All arithmetic is u32-safe: `b >= 1` in the move-up case (the caller requires
/// `dest >= 1`), and in the move-down case `b > a+n ⇒ b >= a+n+1 ⇒ b-n >= a+1 > 0`.
pub fn move_row_sigma(pos: u32, a: u32, n: u32, b: u32) -> u32 {
    if a <= b && b <= a + n {
        // moving within / immediately adjacent to itself: no reordering
        pos
    } else if b > a + n {
        // move DOWN
        if pos >= a && pos < a + n {
            b - n + (pos - a) // moved block → [b-n, b)
        } else if pos >= a + n && pos < b {
            pos - n // rows the block jumped over shift up by n
        } else {
            pos // outside [a, b): fixed
        }
    } else {
        // b < a: move UP
        if pos >= a && pos < a + n {
            b + (pos - a) // moved block → [b, b+n)
        } else if pos >= b && pos < a {
            pos + n // rows the block jumped over shift down by n
        } else {
            pos // outside [b, a+n): fixed
        }
    }
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

/// The last valid 1-based index on an axis: row 1048576, column XFD (16384).
pub(crate) fn grid_max(axis: Axis) -> u32 {
    match axis {
        Axis::Row => 1_048_576,
        Axis::Col => 16_384,
    }
}

/// Shift a single 1-based line index (row number or column number) on the
/// edit's axis. Returns `Some(new_index)` or `None` if that single line is
/// consumed by a delete, or (for insert) pushed past the last row/column — an
/// overflow is #REF!, never a silently out-of-grid reference.
pub(crate) fn shift_index(pos: u32, edit: &StructuralEdit) -> Option<u32> {
    let (k, n) = (edit.at, edit.count);
    match edit.op {
        Op::Insert => {
            let np = if pos >= k { pos + n } else { pos };
            (np <= grid_max(edit.axis)).then_some(np)
        }
        Op::Delete => {
            if pos < k {
                Some(pos)
            } else if pos >= k + n {
                Some(pos - n)
            } else {
                None // inside deleted band
            }
        }
        // Move is a total bijection: a single cell always has an image, never #REF!.
        Op::Move => Some(move_row_sigma(pos, k, n, edit.dest)),
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
            let max = grid_max(edit.axis);
            if h > max {
                // The whole range starts past the last row/column -> #REF!.
                None
            } else {
                // A range whose TAIL overflows is clamped to the last line: a full-height
                // range (e.g. SUM(A2:A1048576)) cannot grow past the grid, so it stays valid
                // — #REF!-ing it would silently turn a real value into an error.
                Some((h, t.min(max)))
            }
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
        Op::Move => {
            // Both endpoints map under σ. σ is monotone WITHIN each contiguous
            // region but reorders regions, so a range that STRADDLES the move
            // boundary cannot be a shifted rectangle. A straddle can leave the
            // endpoints in order (h' <= t') yet SPREAD them apart — enlarging the
            // range (e.g. A4:A6 → A4:A18) and silently changing every dependent
            // value. So a Move range is a valid rectangle ONLY if it moves rigidly:
            // endpoints stay ordered AND the span size is preserved. Otherwise we
            // return None (→ #REF!); the command layer detects it as a
            // `move_straddles_range` residual and refuses BEFORE committing.
            //
            // FIRST, the invariant case: a range that fully CONTAINS the moved block AND its
            // destination only permutes its rows internally — the cell SET is unchanged, so
            // the range stays [head, tail]. The endpoint-size check alone would refuse it
            // (a displaced endpoint breaks the size relation) even though it is value-safe.
            let block_end = k + n - 1;
            let contains_move =
                head <= k && block_end <= tail && edit.dest >= head && edit.dest <= tail + 1;
            if contains_move {
                return Some((head, tail));
            }
            let h = move_row_sigma(head, k, n, edit.dest);
            let t = move_row_sigma(tail, k, n, edit.dest);
            if h <= t && t - h == tail - head {
                Some((h, t))
            } else {
                None
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
        n = n
            .checked_mul(26)?
            .checked_add((c.to_ascii_uppercase() - b'A' + 1) as u32)?;
    }
    Some(n)
}

/// True if `name` is spelled exactly like a GRID-VALID A1 cell reference — a
/// single `[$]col[$]row` token with col in A..XFD (1..=16384) and row in
/// 1..=1048576, consuming the whole string. Such a defined name is
/// indistinguishable from a cell reference to the shift tokenizer, so a formula
/// that uses the name would be silently mis-shifted. The edit layer refuses when
/// any defined name collides this way (it cannot be resolved from formula text
/// alone; this is the fail-closed boundary for that undecidable case).
pub fn looks_like_cell_ref(name: &str) -> bool {
    let b = name.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'$' {
        i += 1;
    }
    let col_start = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    let col = &name[col_start..i];
    if col.is_empty() || !col_to_num(col).is_some_and(|n| (1..=16384).contains(&n)) {
        return false;
    }
    if i < b.len() && b[i] == b'$' {
        i += 1;
    }
    let row_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    // must consume the ENTIRE name and have a valid in-grid row
    i == b.len()
        && i > row_start
        && name[row_start..i]
            .parse::<u32>()
            .is_ok_and(|r| (1..=1048576).contains(&r))
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
    // Tentatively read a leading '$'. It is the COLUMN-absolute marker only if a column
    // letter follows; for a whole-ROW endpoint (`$5`) there is no column, so that same '$'
    // is the ROW-absolute marker. Attributing it eagerly to the column left `$5:$10`
    // unparseable (col_abs with no col) and the whole range was committed STALE.
    let first_dollar = i < b.len() && b[i] == b'$';
    if first_dollar {
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
    // If a column is present the leading '$' was its marker; otherwise it belongs to the row.
    let (col_abs, mut row_abs) = if col.is_some() {
        (first_dollar, false)
    } else {
        (false, first_dollar)
    };
    if col.is_some() {
        row_abs = i < b.len() && b[i] == b'$';
        if row_abs {
            i += 1;
        }
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
        // Excel/IronCalc accept whitespace around the range colon (`A2 : A8`); trim it so
        // the endpoints parse and the range shifts (and normalizes to `A3:A9`) — otherwise
        // parse_endpoint fails on the padded token and the whole range is left stale.
        let (h, t) = (h.trim(), t.trim());
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
    let (h_new, t_new) = if head_was_lo {
        (new_lo, new_hi)
    } else {
        (new_hi, new_lo)
    };
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
/// Length of the UTF-8 sequence starting with lead byte `b0`. The scanner walks
/// bytes but must copy WHOLE scalars: `b[i] as char` reinterprets each byte as a
/// Latin-1 codepoint, which re-encodes to two bytes on write — double-encoding
/// that silently corrupted non-ASCII string literals (found by the in-the-wild
/// locked test on a Japanese workbook, research-log/017). Inputs are valid &str,
/// so lead bytes are well-formed and `i` always lands on a boundary.
#[inline]
fn utf8_len(b0: u8) -> usize {
    if b0 < 0x80 {
        1
    } else if b0 >= 0xF0 {
        4
    } else if b0 >= 0xE0 {
        3
    } else {
        2
    }
}

/// Would a reference token be allowed to START at the current position, given the previously
/// emitted char `prev`? A reference candidate must be preceded by a NON-identifier char, so we
/// never grab the digits of a numeric literal or the cell-shaped tail of a NAME. Excel names
/// admit ASCII word chars, `.`, a leading/embedded backslash, and Unicode letters/digits — plus
/// `$`/`!`/`'` are reference syntax that also continues a token. A preceding non-ASCII scalar in
/// a formula body is only ever a name char here (operators/functions are ASCII; a non-ASCII
/// sheet qualifier is fail-closed upstream; string literals are skipped), so it continues the
/// name. SINGLE SOURCE OF TRUTH for both `shift_formula` and `offset_formula` — the two drifted
/// once (offset_formula lacked the `\`/non-ASCII clauses, mis-shifting `名A5`→`名A6` in a
/// materialized shared-formula dependent), which this shared predicate prevents.
fn ref_start_boundary(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(p) => {
            !(p.is_ascii_alphanumeric()
                || p == '_'
                || p == '.'
                || p == '$'
                || p == '!'
                || p == '\''
                || p == '\\'
                || !p.is_ascii())
        }
    }
}

pub fn shift_formula(formula: &str, current_sheet: &str, edit: &StructuralEdit) -> (String, u32) {
    let b = formula.as_bytes();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    let mut shifted = 0u32;
    while i < b.len() {
        let c = b[i];
        // string literal — copy verbatim (whole UTF-8 scalars, see utf8_len)
        if c == b'"' {
            out.push('"');
            i += 1;
            while i < b.len() {
                if b[i] == b'"' {
                    out.push('"');
                    i += 1;
                    if i < b.len() && b[i] == b'"' {
                        out.push('"');
                        i += 1;
                        continue;
                    }
                    break;
                }
                let l = utf8_len(b[i]);
                out.push_str(&formula[i..i + l]);
                i += l;
            }
            continue;
        }
        // A reference candidate begins at a sheet qualifier / column letter / `$` / digit —
        // but only at a token boundary, so it is not glued to the tail of a NAME (`売上A5`,
        // `\A5`). Boundary predicate shared with offset_formula (see ref_start_boundary).
        let boundary = ref_start_boundary(out.chars().last());
        if boundary
            && (c == b'\''
                || c == b'['
                || c.is_ascii_alphabetic()
                || c == b'$'
                || c.is_ascii_digit())
        {
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
        let l = utf8_len(c);
        out.push_str(&formula[i..i + l]);
        i += l;
    }
    (out, shifted)
}

/// The (col_lo, col_hi, row_lo, row_hi) 1-based grid box a reference BODY spans (a single cell is a
/// degenerate 1x1 box; a whole-row range `5:10` spans all columns; a whole-column range `A:B` spans
/// all rows). None if the body is not a clean A1 reference.
fn ref_box(body: &str) -> Option<(u32, u32, u32, u32)> {
    let (h, t) = match body.split_once(':') {
        Some((h, t)) => (h.trim(), t.trim()),
        None => (body, body),
    };
    let hp = parse_endpoint(h)?;
    let tp = parse_endpoint(t)?;
    let (col_lo, col_hi) = match (hp.1, tp.1) {
        (Some(a), Some(b)) => (a.min(b), a.max(b)),
        (None, None) => (1, 16384), // whole-row range: every column
        _ => return None,
    };
    let (row_lo, row_hi) = match (hp.3, tp.3) {
        (Some(a), Some(b)) => (a.min(b), a.max(b)),
        (None, None) => (1, 1_048_576), // whole-column range: every row
        _ => return None,
    };
    Some((col_lo, col_hi, row_lo, row_hi))
}

/// Whether a single reference `token` (possibly sheet-qualified) covers the target cell.
fn ref_token_covers(
    token: &str,
    home_sheet: &str,
    target_sheet: &str,
    target_col: u32,
    target_row: u32,
    sheets: &[String],
) -> bool {
    if token.starts_with('[') {
        return false; // external-workbook reference — never our cell
    }
    let (sheet_matches, body) = if let Some(bang) = token.rfind('!') {
        let (sheet_part, rest) = token.split_at(bang);
        let target = unquote_sheet(sheet_part);
        let m = if let Some((s1, s2)) = target.split_once(':') {
            // A 3D span `S1:S2!ref` references the cell on EVERY sheet in the span's tab-order range,
            // not just the two named endpoints (the vendored engine now evaluates 3D-span aggregates
            // across the interior, so an interior-sheet consumer is a real dependency). With the
            // ordered sheet list, resolve both endpoints to their tab indices and cover any target
            // whose index lies within [min..=max]; fail-closed (cover) if an endpoint is unresolved.
            let (s1, s2) = (s1.trim(), s2.trim());
            if sheets.is_empty() {
                eq_sheet(s1, target_sheet) || eq_sheet(s2, target_sheet)
            } else {
                let idx = |nm: &str| sheets.iter().position(|s| eq_sheet(s, nm));
                match (idx(s1), idx(s2), idx(target_sheet)) {
                    (Some(a), Some(b), Some(t)) => (a.min(b)..=a.max(b)).contains(&t),
                    _ => true,
                }
            }
        } else {
            eq_sheet(&target, target_sheet)
        };
        (m, &rest[1..])
    } else {
        (eq_sheet(home_sheet, target_sheet), token)
    };
    if !sheet_matches {
        return false;
    }
    match ref_box(body) {
        Some((cl, ch, rl, rh)) => {
            (cl..=ch).contains(&target_col) && (rl..=rh).contains(&target_row)
        }
        None => true, // sheet matches but body opaque -> conservatively a potential reference
    }
}

/// True if `formula` (living on `home_sheet`) references the cell at (`target_sheet`, 1-based
/// `target_col`, `target_row`) — directly, or via a range / whole-row / whole-column that CONTAINS
/// it. A SOUND over-approximation for fail-closed reachability: it walks references with the same
/// boundary/tokenizer logic as [`shift_formula`], and a token it cannot cleanly parse (but whose
/// sheet matches) is treated as a potential reference. It therefore never UNDER-reports a dependency
/// (it may over-report, which only over-refuses). Defined-name and structured (table) references are
/// not resolved here — a caller relying on transitive closure through those must handle them
/// separately (as `defined_names_reaching` does for names). `sheets` is the workbook's tab-ordered
/// sheet-name list, used to resolve a 3D span's INTERIOR sheets (empty = endpoint-only, no interior).
pub(crate) fn formula_references_cell(
    formula: &str,
    home_sheet: &str,
    target_sheet: &str,
    target_col: u32,
    target_row: u32,
    sheets: &[String],
) -> bool {
    let b = formula.as_bytes();
    let mut prev: Option<char> = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            // Skip a string literal (with `""` escapes) — a ref inside it is not a reference.
            i += 1;
            while i < b.len() {
                if b[i] == b'"' {
                    i += 1;
                    if i < b.len() && b[i] == b'"' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += utf8_len(b[i]);
            }
            prev = Some('"');
            continue;
        }
        if ref_start_boundary(prev)
            && (c == b'\''
                || c == b'['
                || c.is_ascii_alphabetic()
                || c == b'$'
                || c.is_ascii_digit())
        {
            let s = &formula[i..];
            if let Some(body_start) = parse_ref_prefix(s) {
                let (body_len, is_ref) = scan_ref_body(&s[body_start..]);
                if is_ref && body_len > 0 {
                    let total = body_start + body_len;
                    let token = &s[..total];
                    if ref_token_covers(
                        token,
                        home_sheet,
                        target_sheet,
                        target_col,
                        target_row,
                        sheets,
                    ) {
                        return true;
                    }
                    prev = token.chars().last();
                    i += total;
                    continue;
                }
            }
        }
        let l = utf8_len(c);
        prev = formula[i..i + l].chars().next();
        i += l;
    }
    false
}

/// True if `f` contains an UNQUOTED sheet qualifier (`…!`) whose name token
/// contains non-ASCII bytes. The unquoted-qualifier grammar below and the
/// scanner's boundary predicate are ASCII-only, so a name like `集計01` is
/// mis-tokenized (`集計01!CI3` parses as sheet `01` + body `CI3`) — the shift
/// then silently leaves the reference stale or shifts a foreign one. Found by
/// the granted post-locked-test review (research-log/017 §post-review); the
/// edit layer FAIL-CLOSES on this detector rather than guessing. Quoted
/// qualifiers (`'集計01'!A1`) are byte-transparent and handled correctly, so
/// they do not trip the detector; string literals are skipped.
pub fn has_unquoted_non_ascii_qualifier(f: &str) -> bool {
    let b = f.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                // string literal with "" escapes
                i += 1;
                while i < b.len() {
                    if b[i] == b'"' {
                        if i + 1 < b.len() && b[i + 1] == b'"' {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'\'' => {
                // quoted qualifier with '' escapes — safe, skip verbatim
                i += 1;
                while i < b.len() {
                    if b[i] == b'\'' {
                        if i + 1 < b.len() && b[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'!' => {
                // backwalk the unquoted qualifier token; delimiters are the
                // ASCII operator/punctuation set (':' stays IN the token — 3D
                // qualifiers like Sheet1:Sheet2! are one token)
                let mut j = i;
                while j > 0 {
                    let p = b[j - 1];
                    if p < 0x80
                        && matches!(
                            p,
                            b'(' | b')'
                                | b','
                                | b'+'
                                | b'-'
                                | b'*'
                                | b'/'
                                | b'^'
                                | b'&'
                                | b'='
                                | b'<'
                                | b'>'
                                | b';'
                                | b' '
                                | b'{'
                                | b'}'
                                | b'%'
                                | b'"'
                                | b'\''
                        )
                    {
                        break;
                    }
                    j -= 1;
                }
                if b[j..i].iter().any(|&c| c >= 0x80) {
                    return true;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    false
}

/// Replace every reference qualified by an UNQUOTED, NON-ASCII sheet name
/// (`集計!A5`, `A1計!B2`) with a neutral `0`, leaving all other references
/// untouched. Returns `None` when the formula carries a non-ASCII 3D SPAN
/// qualifier (`集計:売上!A5`) — such a span may enclose the edited sheet as an
/// interior tab, so it cannot be neutralized soundly and the caller must
/// fall back to refusing.
///
/// This lets a caller decide, on an ASCII-named EDITED sheet, whether an edit
/// touches a formula that also carries non-ASCII qualifiers: a non-ASCII
/// qualifier cannot name the (ASCII) edited sheet, so it references a sheet the
/// edit never moves. After neutralizing those refs, `shift_formula` sees only
/// the ASCII/unqualified (edited-sheet) references and reliably reports whether
/// the edit shifts any of them. The back-walk captures the FULL qualifier —
/// including an ASCII cell-like prefix such as `A1` in `A1計!` — so the danger
/// of `shift_formula` mis-tokenizing that prefix as an edited-sheet cell is
/// removed along with the rest of the qualified reference.
pub(crate) fn neutralize_non_ascii_quals(f: &str) -> Option<String> {
    let b = f.as_bytes();
    // Pass 1: collect [qualifier_start, ref_end) spans of non-ASCII-qualified refs.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() {
                    if b[i] == b'"' {
                        if i + 1 < b.len() && b[i + 1] == b'"' {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'\'' => {
                // Quoted qualifier — safe (shift_formula parses it); skip verbatim.
                i += 1;
                while i < b.len() {
                    if b[i] == b'\'' {
                        if i + 1 < b.len() && b[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'!' => {
                // Back-walk the unquoted qualifier token (same delimiter set as
                // has_unquoted_non_ascii_qualifier; ':' stays IN the token so a
                // 3D span is captured whole).
                let mut j = i;
                while j > 0 {
                    let p = b[j - 1];
                    if p < 0x80
                        && matches!(
                            p,
                            b'(' | b')'
                                | b','
                                | b'+'
                                | b'-'
                                | b'*'
                                | b'/'
                                | b'^'
                                | b'&'
                                | b'='
                                | b'<'
                                | b'>'
                                | b';'
                                | b' '
                                | b'{'
                                | b'}'
                                | b'%'
                                | b'"'
                                | b'\''
                        )
                    {
                        break;
                    }
                    j -= 1;
                }
                if b[j..i].iter().any(|&c| c >= 0x80) {
                    if b[j..i].contains(&b':') {
                        // Non-ASCII 3D span: may enclose the edited sheet — cannot relax.
                        return None;
                    }
                    // Ref body after '!': A1-style cell/range chars.
                    let mut k = i + 1;
                    while k < b.len()
                        && (b[k].is_ascii_alphanumeric() || b[k] == b'$' || b[k] == b':')
                    {
                        k += 1;
                    }
                    spans.push((j, k));
                    i = k;
                    continue;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    if spans.is_empty() {
        return Some(f.to_string());
    }
    // Pass 2: rebuild, replacing each recorded span with a neutral `0`.
    let mut out = String::with_capacity(f.len());
    let mut pos = 0;
    for (s, e) in spans {
        out.push_str(&f[pos..s]);
        out.push('0');
        pos = e;
    }
    out.push_str(&f[pos..]);
    Some(out)
}

/// Parse the optional `[n]` external prefix and `'sheet'!`/`sheet!` qualifier
/// at the start of `s`. Returns the index where the reference BODY begins, or
/// None if `s` opens with a quoted token that is not a sheet qualifier.
fn parse_ref_prefix(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'[' {
        let close = s[i..].find(']')? + i;
        i = close + 1;
    }
    if i < b.len() && b[i] == b'\'' {
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
        if j + 1 < b.len() && b[j + 1] == b'!' {
            Some(j + 2)
        } else {
            None
        }
    } else {
        let mut j = i;
        while j < b.len()
            && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'.' || b[j] == b':')
        {
            j += 1;
        }
        if j < b.len() && b[j] == b'!' && j > i {
            Some(j + 1)
        } else {
            Some(i)
        }
    }
}

/// Try to parse a (possibly sheet-qualified) reference at the start of `s`.
/// Returns (consumed_bytes, replacement_text, did_shift) or None if not a ref.
fn try_reference(
    s: &str,
    current_sheet: &str,
    edit: &StructuralEdit,
) -> Option<(usize, String, bool)> {
    let body_start = parse_ref_prefix(s)?;
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
            let qual = &s[..body_start];
            Some((total, format!("{}#REF!", qual), true))
        }
    }
}

/// Materialize a shared-formula dependent: translate every RELATIVE reference in
/// `formula` by (`dr` rows, `dc` cols) — the dependent's offset from the master.
/// Absolute (`$`) components stay fixed. This reconstructs a dependent cell's
/// explicit formula from the shared master, exactly as autofill does. A
/// component driven below row/column 1 becomes `#REF!`.
pub fn offset_formula(formula: &str, dr: i64, dc: i64) -> String {
    let b = formula.as_bytes();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            out.push('"');
            i += 1;
            while i < b.len() {
                if b[i] == b'"' {
                    out.push('"');
                    i += 1;
                    if i < b.len() && b[i] == b'"' {
                        out.push('"');
                        i += 1;
                        continue;
                    }
                    break;
                }
                let l = utf8_len(b[i]);
                out.push_str(&formula[i..i + l]);
                i += l;
            }
            continue;
        }
        // Same boundary predicate as shift_formula (shared): a candidate glued to a NAME's tail
        // (`名A5`, `\A5`) must NOT be offset as a relative ref — the bug this shared fn prevents.
        let boundary = ref_start_boundary(out.chars().last());
        if boundary
            && (c == b'\''
                || c == b'['
                || c.is_ascii_alphabetic()
                || c == b'$'
                || c.is_ascii_digit())
        {
            if let Some((len, repl)) = try_offset_reference(&formula[i..], dr, dc) {
                out.push_str(&repl);
                i += len;
                continue;
            }
        }
        let l = utf8_len(c);
        out.push_str(&formula[i..i + l]);
        i += l;
    }
    out
}

fn try_offset_reference(s: &str, dr: i64, dc: i64) -> Option<(usize, String)> {
    let body_start = parse_ref_prefix(s)?;
    let (body_len, is_ref) = scan_ref_body(&s[body_start..]);
    if !is_ref || body_len == 0 {
        return None;
    }
    let total = body_start + body_len;
    let qual = &s[..body_start];
    let body = &s[body_start..total];
    let new_body = offset_body(body, dr, dc)?;
    Some((total, format!("{}{}", qual, new_body)))
}

fn offset_endpoint(ep: (bool, Option<u32>, bool, Option<u32>), dr: i64, dc: i64) -> Option<String> {
    let (col_abs, col, row_abs, row) = ep;
    let new_col = match (col, col_abs) {
        (Some(c), false) => {
            let v = c as i64 + dc;
            // Off-sheet in EITHER direction is #REF! — below column A, or past XFD (16384).
            // The upper clamp mirrors shift_index/shift_span; without it a shared dependent
            // materialized an off-grid token (XFE1) instead of #REF!, invalid output that also
            // changed the error class #REF!→#NAME?.
            if v < 1 || v > grid_max(Axis::Col) as i64 {
                return None;
            }
            Some(v as u32)
        }
        (c, _) => c,
    };
    let new_row = match (row, row_abs) {
        (Some(r), false) => {
            let v = r as i64 + dr;
            if v < 1 || v > grid_max(Axis::Row) as i64 {
                return None;
            }
            Some(v as u32)
        }
        (r, _) => r,
    };
    Some(fmt_endpoint(col_abs, new_col, row_abs, new_row))
}

fn offset_body(body: &str, dr: i64, dc: i64) -> Option<String> {
    if let Some((h, t)) = body.split_once(':') {
        let hp = parse_endpoint(h)?;
        let tp = parse_endpoint(t)?;
        match (offset_endpoint(hp, dr, dc), offset_endpoint(tp, dr, dc)) {
            (Some(nh), Some(nt)) => Some(format!("{}:{}", nh, nt)),
            _ => Some("#REF!".to_string()),
        }
    } else {
        let ep = parse_endpoint(body)?;
        Some(offset_endpoint(ep, dr, dc).unwrap_or_else(|| "#REF!".to_string()))
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
        // GRID VALIDITY (not a syntactic proxy): a real column is A..XFD, i.e. its
        // numeric value is in 1..=16384. This rejects `Sales2020` (col far past XFD)
        // and `XFE9`/`ZZZ9` (letter-count 1..=3 but numerically > XFD) alike.
        let has_col =
            i > col_start && col_to_num(&s[col_start..i]).is_some_and(|n| (1..=16384).contains(&n));
        let mut had_row_dollar = false;
        if i < b.len() && b[i] == b'$' {
            had_row_dollar = true;
            i += 1;
        }
        let row_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        // GRID VALIDITY: a real row is 1..=1048576. Rejects `A2000000` (a name that
        // scans like a column+row but whose row is past the sheet limit).
        let has_row = i > row_start
            && s[row_start..i]
                .parse::<u32>()
                .is_ok_and(|r| (1..=1048576).contains(&r));
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
    let sb = s.as_bytes();
    // A reference immediately followed by a letter, '_', '.', or '(' is NOT a reference:
    //  - letter/'_' -> the head of a longer identifier (`BIN2DEC`: prefix `BIN2`
    //    scans as col BIN row 2; a row insert would corrupt it to `BIN3DEC`).
    //  - '.' -> a defined name with a period (`A1.tax`, legal in Excel): its `A1` prefix
    //    scans as a live cell and a row insert would corrupt the NAME to `A2.tax` (→
    //    `#NAME?`). This matches shift_formula's own boundary predicate, which treats '.'
    //    as identifier-continuation.
    //  - '(' -> a function call whose name ends in a digit (`LOG10(...)`: `LOG10`
    //    scans as col LOG row 10; a row insert would corrupt it to `LOG11`).
    // Excel cell refs are never immediately followed by any of these.
    let ident_tail = |end: usize| {
        end < sb.len()
            && (sb[end].is_ascii_alphabetic()
                || sb[end] == b'_'
                || sb[end] == b'.'
                || sb[end] == b'(')
    };
    // range? Excel/IronCalc accept whitespace around the range colon (`A2 : A8` is the
    // range A2:A8), so we must skip it — otherwise the head and tail tokenize as two
    // independent single cells and shift separately, bypassing shift_span's straddle
    // residual and delete clamp (a silent value corruption). Whitespace here is ANY of
    // space/tab/newline/CR — Excel's formula bar (Alt+Enter) writes a newline, e.g.
    // `A1\n:A10`; skipping only 0x20 left those tail cells to #REF! silently. (Whitespace
    // with NO colon is the intersection operator, a different construct, and is left alone.)
    let is_ws = |b: Option<&u8>| matches!(b, Some(b' ' | b'\t' | b'\n' | b'\r'));
    let mut colon = l1;
    while is_ws(sb.get(colon)) {
        colon += 1;
    }
    if sb.get(colon) == Some(&b':') {
        let mut tail_start = colon + 1;
        while is_ws(sb.get(tail_start)) {
            tail_start += 1;
        }
        let (l2, c2, r2) = scan_endpoint(&s[tail_start..]);
        if l2 > 0 {
            // A valid range is one of three KINDS, both endpoints the same kind:
            //   full-cell : A1:B2  (col AND row on both)
            //   whole-col : A:C    (col only on both)   -> shifts under col ops
            //   whole-row : 1:5    (row only on both)   -> shifts under row ops
            // Mixed forms (A1:B, A:B2) are not valid Excel refs.
            let total = tail_start + l2;
            let both_full = (c1 && r1) && (c2 && r2);
            let both_wholecol = (c1 && !r1) && (c2 && !r2);
            let both_wholerow = (!c1 && r1) && (!c2 && r2);
            if both_full || both_wholecol || both_wholerow {
                return (total, !ident_tail(total));
            }
            // The tail is NOT a valid range endpoint (e.g. `A2:CHOOSE(...)`,
            // `B1:OFFSET(...)` — a range whose tail is a function call). Do NOT
            // swallow the head: fall through and treat the head as a single cell
            // ref, exactly as Excel does (the head still shifts; the function
            // tail is protected by its own ident-tail guard). Swallowing it left
            // range heads UNSHIFTED — found by the verified-reference tokenizer
            // differential (3 disagreements in 1.81M comparisons, all this shape).
        }
    }
    // single endpoint: a real cell ref needs a row number (else it's a name), and
    // must not be the prefix of a longer identifier (BIN2DEC, Sales2024, …).
    (l1, c1 && r1 && !ident_tail(l1))
}

/// Walk back from `end` over ONE sheet-qualifier token — a quoted `'…'` (handling `''`
/// escapes) or an unquoted run of `[A-Za-z0-9_.]` — and return its start byte index.
fn walk_qual_token_back(b: &[u8], end: usize) -> usize {
    if end == 0 {
        return 0;
    }
    if b[end - 1] == b'\'' {
        let mut j = end - 1;
        while j > 0 {
            j -= 1;
            if b[j] == b'\'' {
                if j > 0 && b[j - 1] == b'\'' {
                    j -= 1; // an escaped '' — keep walking
                    continue;
                }
                return j; // the opening quote
            }
        }
        j
    } else {
        let mut j = end;
        while j > 0 {
            let c = b[j - 1];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
                j -= 1;
            } else {
                break;
            }
        }
        j
    }
}

/// Walk back over a WHOLE sheet qualifier — one or more tokens joined by top-level `:`
/// (`Sheet1:Sheet2`, `'A':'B'`, `A:'B'`) — returning its start byte index. A fully-quoted span
/// (`'A:B'`) is a single quoted token here; its interior `:` is handled by the splitter.
fn walk_full_qualifier_back(b: &[u8], end: usize) -> usize {
    let mut pos = end;
    loop {
        let tok_start = walk_qual_token_back(b, pos);
        if tok_start == pos {
            break; // consumed nothing
        }
        pos = tok_start;
        if pos > 0 && b[pos - 1] == b':' {
            pos -= 1; // a span separator between two tokens — keep walking
            continue;
        }
        break;
    }
    pos
}

/// Split a sheet qualifier into 3D-span endpoints, or None if it is a single sheet. Handles a
/// top-level `:` between tokens (`Sheet1:Sheet2`, `'A':'B'`, `Sheet1:'Sheet2'`) AND a fully
/// quoted span whose `:` is INSIDE the quotes (`'A-Sheet:B-Sheet'`). Endpoints are normalized.
fn split_span_qualifier(qual: &str) -> Option<(String, String)> {
    let qual = qual.trim();
    let b = qual.as_bytes();
    let mut in_q = false;
    let mut k = 0;
    while k < b.len() {
        match b[k] {
            b'\'' => {
                if in_q && b.get(k + 1) == Some(&b'\'') {
                    k += 2; // '' escape
                    continue;
                }
                in_q = !in_q;
            }
            b':' if !in_q => {
                return Some((
                    normalize_sheet_token(&qual[..k]),
                    normalize_sheet_token(&qual[k + 1..]),
                ));
            }
            _ => {}
        }
        k += 1;
    }
    // No top-level `:` — a fully-quoted span `'X:Y'` carries its `:` inside the single token.
    if qual.len() >= 2 && qual.starts_with('\'') && qual.ends_with('\'') {
        let inner = &qual[1..qual.len() - 1];
        if let Some((a, c)) = inner.split_once(':') {
            return Some((
                a.replace("''", "'").trim().to_string(),
                c.replace("''", "'").trim().to_string(),
            ));
        }
    }
    None
}

/// Normalize a parsed sheet-qualifier token to the bare sheet name: strip surrounding quotes
/// and unescape `''` -> `'`.
fn normalize_sheet_token(raw: &str) -> String {
    let t = raw.trim();
    let t = t
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(t);
    t.replace("''", "'")
}

/// The leading ASCII cell/range reference of `after` (the text just past a span's `!`) shifts
/// under `edit` on `sheet`. An unparseable/empty ref fails closed (true).
fn span_ref_shifts(after: &str, sheet: &str, edit: &StructuralEdit) -> bool {
    let refstr: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '$' || *c == ':')
        .collect();
    if refstr.is_empty() {
        return true;
    }
    shift_formula(&refstr, sheet, edit).0 != refstr
}

/// True if the edited sheet lies WITHIN the tab range `[s1..s2]` (inclusive) of a 3D span. An
/// endpoint name absent from the workbook order, or an edited sheet absent from it, fails
/// closed (true) — we cannot rule out coverage.
fn span_covers_edited(s1: &str, s2: &str, order: &[String], edited: &str) -> bool {
    let idx = |name: &str| order.iter().position(|s| s.eq_ignore_ascii_case(name));
    let (Some(i1), Some(i2), Some(ie)) = (idx(s1), idx(s2), idx(edited)) else {
        return true;
    };
    let (lo, hi) = if i1 <= i2 { (i1, i2) } else { (i2, i1) };
    lo <= ie && ie <= hi
}

/// Detect a 3D span reference (`SheetA:SheetB!…`) the edit cannot faithfully shift: a genuine
/// multi-sheet span whose single shared coordinate the edit would MOVE while the edited sheet
/// lies WITHIN the span's tab range. Such a shift moves cells on only the edited tab, so
/// applying the new coordinate across the whole span orphans the other tabs' data — a silent
/// value change; the edit must be refused. Requires the workbook `sheet_order` (to place the
/// edited sheet relative to the span's endpoints) and `edit` (to test whether the referenced
/// cell actually moves). Returns true only when both hold — an edit OUTSIDE the span, or one
/// that moves nothing the span references, is safe. A self-span (`Sheet1:Sheet1`) is a normal
/// reference. Partial/mixed quoting (`Sheet1:'Sheet2'!`) is parsed correctly.
pub fn has_unverifiable_3d_span(
    formula: &str,
    sheet_order: &[String],
    edit: &StructuralEdit,
) -> bool {
    let b = formula.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            // skip string literal
            i += 1;
            while i < b.len() {
                if b[i] == b'"' {
                    i += 1;
                    if i < b.len() && b[i] == b'"' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            continue;
        }
        // A `!` ends a sheet qualifier; the token(s) before it may be a 3D span `A:B`. Walk
        // back the last token, then — handling `A:B`, `'A':'B'`, `A:'B'`, `'A':B` — the span
        // partner if a top-level `:` precedes it.
        if b[i] == b'!' {
            let qstart = walk_full_qualifier_back(b, i);
            if let Some((s1, s2)) = split_span_qualifier(&formula[qstart..i]) {
                if !s1.eq_ignore_ascii_case(&s2)
                    && span_covers_edited(&s1, &s2, sheet_order, &edit.sheet)
                    && span_ref_shifts(&formula[i + 1..], &edit.sheet, edit)
                {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// True if the formula contains ANY 3D (multi-sheet) span reference `SheetA:SheetB!…` with
/// DISTINCT endpoints. The vendored engine now EVALUATES 3D-span aggregates (SUM/AVERAGE/COUNT/MIN/
/// MAX/… iterate the tab-order range), so the oracle value-gates a span cell: it stays vouchable when
/// the engine returns a number and is excluded only when the span still yields an error (see
/// `build_cache_oracle`'s `three_d_span_cells`). The certify-side date-consumer reachability resolves
/// a span's INTERIOR sheets (round-62). Unlike `has_unverifiable_3d_span` (which additionally requires
/// the span to COVER the edited sheet — the restructure REFUSE condition), this takes no edit.
pub fn formula_contains_3d_span(formula: &str) -> bool {
    let b = formula.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'"' {
                    i += 1;
                    if i < b.len() && b[i] == b'"' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b[i] == b'!' {
            let qstart = walk_full_qualifier_back(b, i);
            if let Some((s1, s2)) = split_span_qualifier(&formula[qstart..i]) {
                if !s1.eq_ignore_ascii_case(&s2) {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Residual detection: does this formula body use a construct the minimal-patch
/// invariant cannot preserve by token surgery? Returns the reason, or None.
// The integrator detects residuals via structural::detect_residual on the parsed
// element; this attribute-string form is kept as documented API and covered by
// tests below (mirrors ooxml::part_names/sheet_part).
#[allow(dead_code)]
pub fn residual_reason(formula_attrs: &str) -> Option<&'static str> {
    // shared/array formula stubs and INDIRECT/OFFSET text refs
    if formula_attrs.contains("t=\"array\"") {
        return Some("array_formula_present");
    }
    if formula_attrs.contains("t=\"shared\"") {
        return Some("shared_formula_present");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_references_cell_covers_single_range_sheet_and_boundaries() {
        // Home sheet "S", target cell B2 (col 2, row 2). No 3D sheet order needed here.
        let refs = |f: &str| formula_references_cell(f, "S", "S", 2, 2, &[]);
        // Direct single-cell ref, with/without $ anchors.
        assert!(refs("=B2+1"));
        assert!(refs("=$B$2*2"));
        assert!(refs("=SUM(A1,B2,C3)"));
        // A range / whole-row / whole-column that CONTAINS B2.
        assert!(refs("=SUM(A1:C3)"));
        assert!(refs("=SUM(2:2)"));
        assert!(refs("=SUM(B:B)"));
        // Refs that do NOT cover B2.
        assert!(!refs("=B3+A2"));
        assert!(!refs("=SUM(C1:D9)"));
        assert!(!refs("=B20")); // not glued: B2 is not a prefix-match of B20
        assert!(!refs("=ABB2")); // a name, not a ref
        assert!(!refs("=\"B2 in a string\""));
        // Sheet qualification: an unqualified ref on a DIFFERENT home sheet does not cover S!B2.
        assert!(!formula_references_cell("=B2", "Other", "S", 2, 2, &[]));
        assert!(formula_references_cell("=S!B2", "Other", "S", 2, 2, &[]));
        assert!(formula_references_cell(
            "='S'!$B$2",
            "Other",
            "S",
            2,
            2,
            &[]
        ));
        assert!(!formula_references_cell(
            "=Other!B2",
            "Other",
            "S",
            2,
            2,
            &[]
        ));
        // An external-workbook ref never covers our cell.
        assert!(!formula_references_cell("=[1]S!B2", "S", "S", 2, 2, &[]));
        // A 3D span naming S as an endpoint covers it.
        assert!(formula_references_cell("=SUM(S:T!B2)", "X", "S", 2, 2, &[]));
        // A 3D span's INTERIOR sheet: with tab order [S1,S2,S3], SUM(S1:S3!B2) covers S2 (round-62).
        let order = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let sheets = order(&["S1", "S2", "S3"]);
        assert!(formula_references_cell(
            "=SUM(S1:S3!B2)",
            "X",
            "S2",
            2,
            2,
            &sheets
        ));
        assert!(formula_references_cell(
            "=SUM(S1:S3!B2)",
            "X",
            "S1",
            2,
            2,
            &sheets
        ));
        assert!(formula_references_cell(
            "=SUM(S1:S3!B2)",
            "X",
            "S3",
            2,
            2,
            &sheets
        ));
        // A sheet OUTSIDE the span range is not covered.
        let sheets4 = order(&["S1", "S2", "S3", "S4"]);
        assert!(!formula_references_cell(
            "=SUM(S1:S2!B2)",
            "X",
            "S4",
            2,
            2,
            &sheets4
        ));
        assert!(!formula_references_cell(
            "=SUM(S1:S2!B2)",
            "X",
            "S3",
            2,
            2,
            &sheets4
        ));
    }

    fn row_edit(op: Op, at: u32, count: u32) -> StructuralEdit {
        StructuralEdit {
            axis: Axis::Row,
            at,
            count,
            op,
            sheet: "Sheet1".into(),
            dest: 0,
        }
    }

    /// Regression for the in-the-wild locked test's real defect (research-log/017):
    /// non-ASCII string literals were double-encoded (`b[i] as char` = Latin-1
    /// misread) — refs shifted correctly but literal TEXT silently corrupted.
    /// The exact shape from the Japanese EUSES workbook, plus mixed-plane checks.
    #[test]
    fn shift_preserves_non_ascii_literals() {
        let f = r#"IF(C8="","",IF(C8=$IA$4,"大当たり！","はずれ！もう一度考えよう！"))"#;
        let (out, n) = shift_formula(f, "Sheet1", &row_edit(Op::Insert, 2, 1));
        assert_eq!(
            out,
            r#"IF(C9="","",IF(C9=$IA$5,"大当たり！","はずれ！もう一度考えよう！"))"#
        );
        assert_eq!(n, 3);
        // 2-byte (é), 3-byte (○), 4-byte (𝄞) scalars inside and outside literals
        let g = r#"IF(A5=1,"café ○ 𝄞","×")&B5"#;
        let (out2, n2) = shift_formula(g, "Sheet1", &row_edit(Op::Insert, 2, 1));
        assert_eq!(out2, r#"IF(A6=1,"café ○ 𝄞","×")&B6"#);
        assert_eq!(n2, 2);
    }

    #[test]
    fn offset_preserves_non_ascii_literals() {
        let out = offset_formula(r#"IF(B2="","",$A$1&"○×表")"#, 1, 0);
        assert_eq!(out, r#"IF(B3="","",$A$1&"○×表")"#);
    }

    /// Regression for the function-endpoint-range head defect, found by the
    /// verified-reference tokenizer differential: `A2:CHOOSE(...)` must shift
    /// its HEAD cell ref (Excel semantics) — the failed range-kind parse must
    /// not swallow it.
    #[test]
    fn function_endpoint_range_head_shifts() {
        let e = row_edit(Op::Insert, 2, 1);
        let (o1, n1) = shift_formula("SUM(A2:CHOOSE(3,A3,A4,A5))", "Sheet1", &e);
        assert_eq!(o1, "SUM(A3:CHOOSE(3,A4,A5,A6))");
        assert_eq!(n1, 4);
        let (o2, _) = shift_formula("SUM(A9:CHOOSE(2,A10,A11,A12))", "Sheet1", &e);
        assert_eq!(o2, "SUM(A10:CHOOSE(2,A11,A12,A13))");
        let ec = col_edit(Op::Insert, 2, 1);
        let (o3, _) = shift_formula("SUM(B1:OFFSET(B1,3,0))", "Sheet1", &ec);
        assert_eq!(o3, "SUM(C1:OFFSET(C1,3,0))");
        // valid ranges and whole-col ranges unaffected by the fallback
        let (o4, _) = shift_formula("SUM(A2:B5)+SUM(F:F)", "Sheet1", &e);
        assert_eq!(o4, "SUM(A3:B6)+SUM(F:F)");
    }

    /// Regression for the post-review sibling defect: unquoted non-ASCII sheet
    /// qualifiers are outside the ASCII tokenizer grammar — the edit layer must
    /// detect and fail-close, never mis-shift.
    #[test]
    fn detects_unquoted_non_ascii_qualifier() {
        assert!(has_unquoted_non_ascii_qualifier("集計01!CI3"));
        assert!(has_unquoted_non_ascii_qualifier("SUM(集計01!CI3:CI9)+A1"));
        assert!(has_unquoted_non_ascii_qualifier("データ!B2"));
        // quoted qualifiers are handled correctly — must NOT trip
        assert!(!has_unquoted_non_ascii_qualifier("'集計01'!CI3"));
        assert!(!has_unquoted_non_ascii_qualifier("SUM('データ'!B2:B9)"));
        // non-ASCII only inside string literals — must NOT trip
        assert!(!has_unquoted_non_ascii_qualifier(
            r#"IF(A1=1,"集計!","x")&Sheet2!B1"#
        ));
        // plain ASCII cross-sheet and same-sheet — must NOT trip
        assert!(!has_unquoted_non_ascii_qualifier("Sheet2!A1+SUM(B1:B9)"));
        assert!(!has_unquoted_non_ascii_qualifier("SUM(A1:B2)"));
    }

    /// The affect-based relaxation for non-ASCII qualifiers rests on neutralizing
    /// exactly the non-ASCII-qualified references (they name non-edited sheets on an
    /// ASCII edited sheet) while leaving edited-sheet references intact, and bailing
    /// out on non-ASCII 3D spans.
    #[test]
    fn neutralizes_non_ascii_qualified_refs() {
        // A lone non-ASCII-qualified ref -> replaced whole; nothing edited-sheet remains.
        assert_eq!(neutralize_non_ascii_quals("集計!A5").as_deref(), Some("0"));
        // Bare edited-sheet ref alongside a non-ASCII-qualified ref -> only the latter goes.
        assert_eq!(
            neutralize_non_ascii_quals("集計!A5+A5").as_deref(),
            Some("0+A5")
        );
        // ASCII CELL-LIKE prefix in the qualifier (`A1計!`) is captured WHOLE by the
        // back-walk, so the `A1` cannot leak out to be mis-shifted as an edited cell.
        assert_eq!(neutralize_non_ascii_quals("A1計!B5").as_deref(), Some("0"));
        assert_eq!(
            neutralize_non_ascii_quals("A1計!B5:B9+Sheet1!A5").as_deref(),
            Some("0+Sheet1!A5")
        );
        // A non-ASCII 3D SPAN may enclose the edited sheet -> cannot neutralize soundly.
        assert_eq!(neutralize_non_ascii_quals("SUM(集計:売上!A5)"), None);
        // Quoted qualifiers and string literals are left untouched (shift_formula parses them).
        assert_eq!(
            neutralize_non_ascii_quals("'集計'!A5").as_deref(),
            Some("'集計'!A5")
        );
        assert_eq!(
            neutralize_non_ascii_quals(r#"IF(A1=1,"集計!",A5)"#).as_deref(),
            Some(r#"IF(A1=1,"集計!",A5)"#)
        );
        // Pure ASCII -> identity.
        assert_eq!(
            neutralize_non_ascii_quals("Sheet2!A1+A5").as_deref(),
            Some("Sheet2!A1+A5")
        );
    }
    fn col_edit(op: Op, at: u32, count: u32) -> StructuralEdit {
        StructuralEdit {
            axis: Axis::Col,
            at,
            count,
            op,
            sheet: "Sheet1".into(),
            dest: 0,
        }
    }
    fn move_edit(at: u32, count: u32, dest: u32) -> StructuralEdit {
        StructuralEdit {
            axis: Axis::Row,
            at,
            count,
            op: Op::Move,
            sheet: "Sheet1".into(),
            dest,
        }
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
        assert_eq!(
            shift_body("A5:A10", &row_edit(Op::Insert, 2, 1)),
            s("A6:A11")
        );
    }
    #[test]
    fn insert_inside_grows_range() {
        // insert 1 at k=7; A5:A10 straddles (head 5<7<=tail 10) → grow tail: A5:A11
        assert_eq!(
            shift_body("A5:A10", &row_edit(Op::Insert, 7, 1)),
            s("A5:A11")
        );
    }
    #[test]
    fn insert_above_whole_range_unchanged() {
        // insert 1 at k=20; A5:A10 entirely above → unchanged
        assert_eq!(
            shift_body("A5:A10", &row_edit(Op::Insert, 20, 1)),
            Shift::Unchanged
        );
    }
    #[test]
    fn insert_at_head_boundary_shifts_not_grows() {
        // insert at k == head (5): head>=k → both shift → A6:A11 (range moves, blank excluded at top)
        assert_eq!(
            shift_body("A5:A10", &row_edit(Op::Insert, 5, 1)),
            s("A6:A11")
        );
    }
    #[test]
    fn insert_at_tail_boundary_grows() {
        // insert at k == tail (10): head 5<10 fixed, tail 10>=10 shifts → grow (blank included at bottom)
        assert_eq!(
            shift_body("A5:A10", &row_edit(Op::Insert, 10, 1)),
            s("A5:A11")
        );
    }

    // ---- DELETE: the 6-case clamp (the theory-review FAIL) ----
    #[test]
    fn delete_clip_head_endpoint() {
        // =SUM(A5:A10) delete rows 5-6 → SUM(A5:A8): clamp head to k=5, tail 10-2=8
        assert_eq!(
            shift_body("A5:A10", &row_edit(Op::Delete, 5, 2)),
            s("A5:A8")
        );
    }
    #[test]
    fn delete_straddle_shrinks() {
        // =SUM(A3:A10) delete rows 5-6 → head 3<5 fixed, tail 10-2=8 → A3:A8
        assert_eq!(
            shift_body("A3:A10", &row_edit(Op::Delete, 5, 2)),
            s("A3:A8")
        );
    }
    #[test]
    fn delete_head_in_band_clamps_to_k() {
        // =SUM(A5:A10) delete rows 3-6 (k=3,n=4,band[3,7)) → head 5 in band→clamp to 3, tail 10-4=6 → A3:A6
        assert_eq!(
            shift_body("A5:A10", &row_edit(Op::Delete, 3, 4)),
            s("A3:A6")
        );
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
        assert_eq!(
            shift_body("A3", &row_edit(Op::Delete, 5, 2)),
            Shift::Unchanged
        );
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
        assert_eq!(
            shift_body("$A5:B$10", &row_edit(Op::Insert, 2, 3)),
            s("$A8:B$13")
        );
    }

    // ---- axis selectivity + whole-row/col forms (C7) ----
    #[test]
    fn row_op_leaves_whole_column_ref_untouched() {
        // A:A under a ROW op → unchanged (no row component)
        assert_eq!(
            shift_body("A:A", &row_edit(Op::Insert, 5, 1)),
            Shift::Unchanged
        );
    }
    #[test]
    fn row_op_shifts_whole_row_ref() {
        // 5:5 under a row insert at k=2 → 6:6
        assert_eq!(shift_body("5:5", &row_edit(Op::Insert, 2, 1)), s("6:6"));
    }
    #[test]
    fn col_op_shifts_columns_only() {
        // B5:D10 insert 1 col at k=3 (col C) → B fixed(<C? B=2<3 yes)… D=4>=3→E → B5:E10
        assert_eq!(
            shift_body("B5:D10", &col_edit(Op::Insert, 3, 1)),
            s("B5:E10")
        );
    }
    #[test]
    fn col_op_leaves_whole_row_ref_untouched() {
        assert_eq!(
            shift_body("5:5", &col_edit(Op::Insert, 3, 1)),
            Shift::Unchanged
        );
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
    fn three_d_span_including_edited_sheet_shifts() {
        // Sheet1:Sheet3!A5 with edit on Sheet1 (an endpoint) → shift
        let e = row_edit(Op::Insert, 5, 1);
        assert_eq!(
            shift_ref("Sheet1:Sheet3!A5", "Sheet2", &e),
            s("Sheet1:Sheet3!A6")
        );
    }

    // ---- formula tokenizer (shift_formula) ----
    fn sf(f: &str, sheet: &str, e: &StructuralEdit) -> String {
        shift_formula(f, sheet, e).0
    }
    #[test]
    fn formula_shifts_simple_range() {
        assert_eq!(
            sf("SUM(A5:A10)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "SUM(A6:A11)"
        );
    }
    #[test]
    fn formula_leaves_function_names_alone() {
        // SUM parses as col letters but has no row and no ':' → not a ref
        assert_eq!(
            sf("SUM(A5)+MAX(B2)", "Sheet1", &row_edit(Op::Insert, 100, 1)),
            "SUM(A5)+MAX(B2)"
        );
    }
    #[test]
    fn formula_does_not_touch_string_literals() {
        // the "A5" inside the quotes must NOT shift
        assert_eq!(
            sf(
                r#"IF(A5>0,"row A5 here",B10)"#,
                "Sheet1",
                &row_edit(Op::Insert, 2, 1)
            ),
            r#"IF(A6>0,"row A5 here",B11)"#
        );
    }
    #[test]
    fn formula_shifts_cross_sheet_and_scopes() {
        // edit on Sheet1; Sheet1!A5 shifts, Sheet2!B10 (other sheet) does not
        assert_eq!(
            sf(
                "Sheet1!A5+Sheet2!B10",
                "SheetX",
                &row_edit(Op::Insert, 3, 1)
            ),
            "Sheet1!A6+Sheet2!B10"
        );
    }
    #[test]
    fn formula_preserves_absolute_and_mixed() {
        assert_eq!(
            sf("$A$5+B$10", "Sheet1", &row_edit(Op::Insert, 5, 2)),
            "$A$7+B$12"
        );
    }
    #[test]
    fn formula_delete_produces_ref_error() {
        assert_eq!(
            sf("A5+B10", "Sheet1", &row_edit(Op::Delete, 5, 1)),
            "#REF!+B9"
        );
    }
    #[test]
    fn formula_indirect_text_arg_not_shifted() {
        // INDIRECT's "A5" is a string → untouched; the bare B10 shifts
        assert_eq!(
            sf(
                r#"INDIRECT("A5")+B10"#,
                "Sheet1",
                &row_edit(Op::Insert, 2, 1)
            ),
            r#"INDIRECT("A5")+B11"#
        );
    }
    #[test]
    fn formula_whole_column_under_row_op_unchanged() {
        assert_eq!(
            sf("SUM(A:A)", "Sheet1", &row_edit(Op::Insert, 5, 1)),
            "SUM(A:A)"
        );
    }
    #[test]
    fn formula_whole_column_under_col_op_shifts() {
        // the FATAL scanner-routing bug: SUM(A:A) under a column insert must
        // become SUM(B:B), not stay unchanged.
        assert_eq!(
            sf("SUM(A:A)", "Sheet1", &col_edit(Op::Insert, 1, 1)),
            "SUM(B:B)"
        );
        assert_eq!(
            sf("SUM(A:C)", "Sheet1", &col_edit(Op::Insert, 1, 1)),
            "SUM(B:D)"
        );
        assert_eq!(
            sf("SUM($A:$C)", "Sheet1", &col_edit(Op::Insert, 1, 1)),
            "SUM($B:$D)"
        );
    }
    #[test]
    fn formula_whole_column_delete_consumed_is_ref() {
        // deleting column A entirely consumes SUM(A:A) -> #REF!
        assert_eq!(
            sf("SUM(A:A)", "Sheet1", &col_edit(Op::Delete, 1, 1)),
            "SUM(#REF!)"
        );
    }
    #[test]
    fn formula_whole_row_under_row_op_shifts() {
        assert_eq!(
            sf("SUM(5:5)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "SUM(6:6)"
        );
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
    #[test]
    fn formula_function_name_with_digits_not_shifted() {
        // REGRESSION: `BIN2DEC` prefix `BIN2` scans as a cell ref (col BIN, row 2);
        // a row insert must NOT rewrite it to `BIN3DEC`. The real cell arg shifts.
        assert_eq!(
            sf("BIN2DEC(A2)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "BIN2DEC(A3)"
        );
        assert_eq!(
            sf("BIN2HEX(A2,B2)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "BIN2HEX(A3,B3)"
        );
        // a defined-name-like identifier with a digit tail is also left alone
        assert_eq!(
            sf("Sales2020+A2", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "Sales2020+A3"
        );
        // function name ENDING in a digit before '(' (LOG10 = col LOG, row 10) is a
        // call, not a ref — must not become LOG11; the real arg A10 shifts.
        assert_eq!(
            sf("LOG10(A10)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "LOG10(A11)"
        );
        // and a ref genuinely followed by a paren-less context still shifts
        assert_eq!(
            sf("A10+LOG10(A10)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "A11+LOG10(A11)"
        );
        // REGRESSION (round-21): a defined name with a PERIOD (`A1.tax`, legal in Excel) has an
        // `A1` prefix; a row insert must NOT rewrite it to `A2.tax` (which is `#NAME?`). The
        // real cell arg still shifts.
        assert_eq!(
            sf("A1.tax+A2", "Sheet1", &row_edit(Op::Insert, 1, 1)),
            "A1.tax+A3"
        );
        assert_eq!(
            sf("Q3.total*2", "Sheet1", &row_edit(Op::Insert, 1, 1)),
            "Q3.total*2"
        );
    }
    #[test]
    fn formula_non_ascii_or_backslash_prefixed_name_suffix_not_shifted() {
        // REGRESSION (round-31): a defined name whose spelling is a non-ASCII (CJK) or
        // backslash prefix immediately followed by a grid-valid A1 spelling (`売上A5`,
        // `\A5`) is ONE name, not a name + cell ref. A row insert must NOT rewrite the
        // trailing `A5` (it did → `売上A6`, an undefined name → `#NAME?`, a silent value
        // corruption). A genuinely separate ref in the same formula still shifts.
        let ins1 = row_edit(Op::Insert, 1, 1);
        assert_eq!(sf("売上A5", "Sheet1", &ins1), "売上A5");
        assert_eq!(sf("予算Q1", "Sheet1", &ins1), "予算Q1");
        assert_eq!(sf("\\A5", "Sheet1", &ins1), "\\A5");
        assert_eq!(sf("SUM(売上A5)", "Sheet1", &ins1), "SUM(売上A5)");
        // name is left alone while a real, separately-tokenized ref shifts
        assert_eq!(
            sf("IF(A1=1,\"x\",売上A5)", "Sheet1", &ins1),
            "IF(A2=1,\"x\",売上A5)"
        );
        // the non-ASCII char must not shield a following, genuinely separate ref:
        // `売上&A5` — the `A5` after the ASCII `&` operator is its own ref and shifts.
        assert_eq!(sf("売上&A5", "Sheet1", &ins1), "売上&A6");
    }
    #[test]
    fn formula_out_of_grid_tokens_not_shifted() {
        // GRID VALIDITY: XFE/ZZZ are 1..=3 letters but numerically past XFD(16384),
        // and row 2000000 is past 1048576 — these are names, not cells, so a row
        // insert must leave them alone while shifting the real ref A5.
        assert_eq!(
            sf("XFE9+A5", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "XFE9+A6"
        );
        assert_eq!(
            sf("ZZZ9+A5", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "ZZZ9+A6"
        );
        assert_eq!(
            sf("A2000000+A5", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "A2000000+A6"
        );
        // the boundary column XFD and boundary row ARE valid and shift
        assert_eq!(
            sf("XFD9+A5", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "XFD10+A6"
        );
    }

    // ---- offset_formula (shared-formula materialization) ----
    #[test]
    fn offset_translates_relative_refs() {
        // master =A2*2 at B2; dependent at B5 (offset +3 rows) => =A5*2
        assert_eq!(offset_formula("A2*2", 3, 0), "A5*2");
        // dependent at B2 (offset 0) => unchanged
        assert_eq!(offset_formula("A2*2", 0, 0), "A2*2");
    }
    #[test]
    fn offset_keeps_absolute_fixed() {
        // $A$1 stays; relative B2 shifts
        assert_eq!(offset_formula("$A$1+B2", 3, 0), "$A$1+B5");
        assert_eq!(offset_formula("$A2+A$1", 3, 2), "$A5+C$1");
    }
    #[test]
    fn offset_range_and_cross_sheet() {
        assert_eq!(offset_formula("SUM(A2:A10)", 5, 0), "SUM(A7:A15)");
        assert_eq!(offset_formula("Sheet2!A2*Q1", 0, 1), "Sheet2!B2*R1");
    }
    #[test]
    fn offset_leaves_non_ascii_or_backslash_prefixed_name_intact() {
        // REGRESSION (round-33): shared-formula dependents are MATERIALIZED through
        // offset_formula. A defined name with a non-ASCII (CJK) or backslash prefix and a
        // cell-shaped ASCII tail (`名A5`, `\A5`) is ONE name, invariant under autofill — it must
        // NOT be offset as a relative cell ref (that rewrote a dependent to `名A6`, an undefined
        // name → `#NAME?`). Boundary predicate is now shared with shift_formula, so the two
        // cannot drift again. A genuinely relative ref in the same body still offsets.
        assert_eq!(offset_formula("名A5*2", 1, 0), "名A5*2");
        assert_eq!(offset_formula("予算Q1+1", 3, 0), "予算Q1+1");
        assert_eq!(offset_formula("\\A5", 5, 0), "\\A5");
        // name left intact, but the separately-tokenized B2 offsets by +3 rows
        assert_eq!(offset_formula("名A5+B2", 3, 0), "名A5+B5");
    }
    #[test]
    fn offset_underflow_is_ref() {
        // relative A2 offset up by 5 -> row -3 -> #REF!
        assert_eq!(offset_formula("A2", -5, 0), "#REF!");
    }
    #[test]
    fn offset_overflow_is_ref() {
        // REGRESSION (round-27): a shared dependent offset PAST the grid edge must be #REF!,
        // not a materialized off-grid token (XFE1 / A1048580). Mirrors shift_index's clamp.
        assert_eq!(offset_formula("XFC1", 0, 2), "#REF!"); // col 16383 + 2 -> 16385 > XFD
        assert_eq!(offset_formula("A1048575", 5, 0), "#REF!"); // row past 1048576
                                                               // ...but an offset that stays on the grid still shifts.
        assert_eq!(offset_formula("XFC1", 0, 1), "XFD1"); // 16383 + 1 = 16384 = XFD (last col)
    }
    #[test]
    fn offset_leaves_strings_and_functions() {
        assert_eq!(
            offset_formula(r#"IF(A2,"A2",B2)"#, 1, 0),
            r#"IF(A3,"A2",B3)"#
        );
    }

    #[test]
    fn defined_name_cell_ref_collision_detection() {
        // names spelled like grid-valid cells -> collide (must be refused upstream)
        assert!(looks_like_cell_ref("FY2021")); // col FY, row 2021
        assert!(looks_like_cell_ref("Q1"));
        assert!(looks_like_cell_ref("$A$5"));
        assert!(looks_like_cell_ref("XFD1"));
        // names that are NOT grid-valid cells -> safe (no collision)
        assert!(!looks_like_cell_ref("TaxRate")); // no row digits
        assert!(!looks_like_cell_ref("XFE9")); // col past XFD
        assert!(!looks_like_cell_ref("A2000000")); // row past 1048576
        assert!(!looks_like_cell_ref("Sales2020")); // col SALES past XFD
        assert!(!looks_like_cell_ref("FY2021x")); // trailing junk
        assert!(!looks_like_cell_ref("Total"));
    }

    // ---- residual detection ----
    #[test]
    fn detects_array_and_shared_residual() {
        assert_eq!(
            residual_reason("t=\"array\" ref=\"C2:C10\""),
            Some("array_formula_present")
        );
        assert_eq!(
            residual_reason("t=\"shared\" ref=\"B2:B100\" si=\"0\""),
            Some("shared_formula_present")
        );
        assert_eq!(residual_reason(""), None);
    }

    // ---- MOVE: the σ permutation (both directions), computed by hand ----
    #[test]
    fn move_sigma_down_matches_hand() {
        // block rows [5,7) → before row 9 (b=9>a+n=7). gap rows 7,8 shift up.
        // σ: 5→7, 6→8, 7→5, 8→6, {<5,>=9}→fixed.
        assert_eq!(move_row_sigma(5, 5, 2, 9), 7);
        assert_eq!(move_row_sigma(6, 5, 2, 9), 8);
        assert_eq!(move_row_sigma(7, 5, 2, 9), 5);
        assert_eq!(move_row_sigma(8, 5, 2, 9), 6);
        assert_eq!(move_row_sigma(4, 5, 2, 9), 4);
        assert_eq!(move_row_sigma(9, 5, 2, 9), 9);
        assert_eq!(move_row_sigma(10, 5, 2, 9), 10);
    }
    #[test]
    fn move_sigma_up_matches_hand() {
        // block row 6 → before row 3 (b=3<a=6). gap rows 3,4,5 shift down.
        // σ: 6→3, 3→4, 4→5, 5→6, {<3,>=7}→fixed.
        assert_eq!(move_row_sigma(6, 6, 1, 3), 3);
        assert_eq!(move_row_sigma(3, 6, 1, 3), 4);
        assert_eq!(move_row_sigma(4, 6, 1, 3), 5);
        assert_eq!(move_row_sigma(5, 6, 1, 3), 6);
        assert_eq!(move_row_sigma(2, 6, 1, 3), 2);
        assert_eq!(move_row_sigma(7, 6, 1, 3), 7);
    }
    #[test]
    fn move_sigma_identity_when_adjacent() {
        // a <= b <= a+n is a no-op: dest == a and dest == a+n both identity.
        for pos in 1..=12u32 {
            assert_eq!(move_row_sigma(pos, 5, 2, 5), pos, "dest==a identity");
            assert_eq!(move_row_sigma(pos, 5, 2, 7), pos, "dest==a+n identity");
            assert_eq!(
                move_row_sigma(pos, 5, 2, 6),
                pos,
                "dest inside block identity"
            );
        }
    }
    #[test]
    fn move_sigma_is_a_bijection_on_1_to_maxrow() {
        // exhaustively: for many (a,n,b), σ restricted to [1,maxrow] is a
        // permutation (every image distinct and in-range) whenever the block and
        // destination stay in grid.
        let maxrow = 20u32;
        for a in 1..=maxrow {
            for n in 1..=(maxrow + 1 - a) {
                for b in 1..=(maxrow + 1) {
                    // dest must keep image in grid: skip out-of-grid destinations
                    // (moving a block so its landing exceeds maxrow).
                    if b > a + n && b > maxrow + 1 {
                        continue;
                    }
                    let mut seen = std::collections::BTreeSet::new();
                    for pos in 1..=maxrow {
                        let img = move_row_sigma(pos, a, n, b);
                        assert!(
                            (1..=maxrow).contains(&img),
                            "σ out of grid: a={a} n={n} b={b} pos={pos} -> {img}"
                        );
                        assert!(
                            seen.insert(img),
                            "σ not injective: a={a} n={n} b={b} collision at {img}"
                        );
                    }
                    // injective + closed range on a finite set ⇒ bijection
                    assert_eq!(seen.len(), maxrow as usize);
                }
            }
        }
    }
    #[test]
    fn move_sigma_preserves_block_order() {
        // order within the moved block is preserved (strictly increasing).
        for &(a, n, b) in &[(5u32, 3u32, 12u32), (8, 4, 2), (6, 1, 3), (2, 5, 15)] {
            for p in a..(a + n - 1) {
                assert!(
                    move_row_sigma(p, a, n, b) < move_row_sigma(p + 1, a, n, b),
                    "block order broken a={a} n={n} b={b} at p={p}"
                );
            }
        }
    }

    // ---- MOVE: single-cell refs follow σ; ranges straddling → #REF! ----
    #[test]
    fn move_single_cell_refs_follow_sigma_down() {
        // A5 → A7 (block moved down); A10 fixed (>= dest=9).
        assert_eq!(shift_body("A5", &move_edit(5, 2, 9)), s("A7"));
        assert_eq!(shift_body("A10", &move_edit(5, 2, 9)), Shift::Unchanged);
        // A7 (a jumped-over gap row) shifts up to A5.
        assert_eq!(shift_body("A7", &move_edit(5, 2, 9)), s("A5"));
        // absolute flag transparent to the move
        assert_eq!(shift_body("$A$5", &move_edit(5, 2, 9)), s("$A$7"));
    }
    #[test]
    fn move_single_cell_refs_follow_sigma_up() {
        // A6 → A3 (block moved up); A3 → A4, A5 → A6 (gap rows shift down).
        assert_eq!(shift_body("A6", &move_edit(6, 1, 3)), s("A3"));
        assert_eq!(shift_body("A3", &move_edit(6, 1, 3)), s("A4"));
        assert_eq!(shift_body("A5", &move_edit(6, 1, 3)), s("A6"));
        assert_eq!(shift_body("A2", &move_edit(6, 1, 3)), Shift::Unchanged);
    }
    #[test]
    fn move_range_within_block_or_gap_shifts_as_rectangle() {
        // whole block [5,6] → [7,8]; monotone within block, not a straddle.
        assert_eq!(shift_body("A5:A6", &move_edit(5, 2, 9)), s("A7:A8"));
        // whole gap [3,5] → [4,6] under move-up (monotone within gap).
        assert_eq!(shift_body("A3:A5", &move_edit(6, 1, 3)), s("A4:A6"));
    }
    #[test]
    fn move_range_straddling_boundary_is_ref() {
        // A4:A6 with row 6 moved before row 3: σ(4)=5, σ(6)=3 → 5>3 → straddle.
        assert_eq!(shift_body("A4:A6", &move_edit(6, 1, 3)), Shift::Ref);
        // A6:A8 with rows 5,6 moved down before 9: σ(6)=8, σ(8)=6 → straddle.
        assert_eq!(shift_body("A6:A8", &move_edit(5, 2, 9)), Shift::Ref);
    }
    #[test]
    fn move_identity_leaves_refs_unchanged() {
        assert_eq!(shift_body("A5", &move_edit(5, 2, 5)), Shift::Unchanged); // b==a
        assert_eq!(shift_body("A6", &move_edit(5, 2, 7)), Shift::Unchanged); // b==a+n
    }
    #[test]

    fn move_formula_shifts_single_cells_and_detects_straddle() {
        // the task's worked example: A5→A7, A10 fixed.
        assert_eq!(sf("A5+A10", "Sheet1", &move_edit(5, 2, 9)), "A7+A10");
        // move-up example: A6→A3, A3→A4.
        assert_eq!(sf("A6+A3", "Sheet1", &move_edit(6, 1, 3)), "A3+A4");
        // a straddling range introduces a NEW #REF! (the residual-gate signal).
        assert!(sf("SUM(A4:A6)", "Sheet1", &move_edit(6, 1, 3)).contains("#REF!"));
        // REGRESSION (round-7): a NON-inverting straddle — endpoints stay ordered under σ
        // but the span SIZE changes — was silently enlarged (A4:A6 -> A4:A18). It must #REF!.
        assert!(sf("SUM(A4:A6)", "Sheet1", &move_edit(5, 3, 20)).contains("#REF!"));
        // REGRESSION (round-10): a range that fully CONTAINS the moved block (and its dest)
        // only permutes rows internally — the SET is invariant, so it must NOT be refused.
        assert_eq!(
            sf("SUM(A1:A10)", "Sheet1", &move_edit(1, 1, 3)),
            "SUM(A1:A10)"
        );
        assert_eq!(
            sf("SUM(A1:A10)", "Sheet1", &move_edit(10, 1, 3)),
            "SUM(A1:A10)"
        );
        // REGRESSION (round-11): an absolute/mixed whole-ROW range ($5:$10) was left stale
        // (parse_endpoint mis-read the row's `$` as the column's). It now shifts.
        assert_eq!(
            sf("SUM($5:$10)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "SUM($6:$11)"
        );
        assert_eq!(
            sf("SUM(5:$10)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "SUM(6:$11)"
        );
        assert_eq!(
            sf("SUM($5:$5)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "SUM($6:$6)"
        );
        // whole-COLUMN absolute is unaffected by a row op (asymmetry guard).
        assert_eq!(
            sf("SUM($A:$C)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "SUM($A:$C)"
        );
    }

    #[test]
    fn whitespace_range_and_grid_boundary() {
        // REGRESSION (round-8): whitespace around the range colon (`A2 : A8`, which
        // IronCalc parses as A2:A8) tokenized as two independent cells and bypassed the
        // straddle/clamp logic. It now shifts as a range (normalizing away the spaces)...
        assert_eq!(
            sf("SUM(A2 : A8)", "Sheet1", &row_edit(Op::Insert, 3, 1)),
            "SUM(A2:A9)"
        );
        // ...and enters the straddle path: a spaced range whose interior block is moved OUT
        // of the range (a genuine straddle) -> #REF!.
        assert!(sf("SUM(A2 : A8)", "Sheet1", &move_edit(5, 1, 20)).contains("#REF!"));
        // REGRESSION (round-8): a reference to the LAST row/column overflows to #REF! on
        // insert, never a silently out-of-grid reference (A1048577 / XFE1).
        assert!(sf("A1048576", "Sheet1", &row_edit(Op::Insert, 1, 1)).contains("#REF!"));
        assert!(sf("XFD1", "Sheet1", &col_edit(Op::Insert, 1, 1)).contains("#REF!"));
        // a normal boundary-adjacent ref still shifts cleanly.
        assert_eq!(
            sf("A1048575", "Sheet1", &row_edit(Op::Insert, 1, 1)),
            "A1048576"
        );
        // REGRESSION (round-9): a full-height RANGE whose TAIL overflows must CLAMP to the
        // last line, not collapse the whole range to #REF! — Excel keeps it valid.
        assert_eq!(
            sf("SUM(A1:A1048576)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "SUM(A1:A1048576)"
        );
        assert_eq!(
            sf("SUM(A2:A1048576)", "Sheet1", &row_edit(Op::Insert, 2, 1)),
            "SUM(A3:A1048576)"
        );
        // a clean move introduces none.
        assert!(!sf("A5+A10", "Sheet1", &move_edit(5, 2, 9)).contains("#REF!"));
        // string literals and function names are still untouched.
        assert_eq!(
            sf(r#"IF(A6>0,"A6",A3)"#, "Sheet1", &move_edit(6, 1, 3)),
            r#"IF(A3>0,"A6",A4)"#
        );
    }

    fn parse_res(sh: &Shift, lo: u32, hi: u32) -> Option<(u32, u32)> {
        match sh {
            Shift::Unchanged => Some((lo, hi)),
            Shift::Ref => None,
            Shift::Shifted(s) => {
                if let Some((a, b)) = s.split_once(':') {
                    let pa: u32 = a.trim_start_matches('A').parse().unwrap();
                    let pb: u32 = b.trim_start_matches('A').parse().unwrap();
                    Some((pa.min(pb), pa.max(pb)))
                } else {
                    let p: u32 = s.trim_start_matches('A').parse().unwrap();
                    Some((p, p))
                }
            }
        }
    }
    fn body_for(lo: u32, hi: u32) -> String {
        if lo == hi {
            format!("A{}", lo)
        } else {
            format!("A{}:A{}", lo, hi)
        }
    }

    #[test]
    fn fuzz_delete_against_set_oracle() {
        let g = 14u32;
        let mut fails = Vec::new();
        for lo in 1..=g {
            for hi in lo..=g {
                for k in 1..=g {
                    for n in 1..=g {
                        let mut imgs = Vec::new();
                        for r in lo..=hi {
                            if r < k {
                                imgs.push(r);
                            } else if r >= k + n {
                                imgs.push(r - n);
                            }
                        }
                        let oracle = if imgs.is_empty() {
                            None
                        } else {
                            Some((*imgs.iter().min().unwrap(), *imgs.iter().max().unwrap()))
                        };
                        let got = parse_res(
                            &shift_body(&body_for(lo, hi), &row_edit(Op::Delete, k, n)),
                            lo,
                            hi,
                        );
                        if got != oracle {
                            fails.push(format!(
                                "DEL lo={} hi={} k={} n={} oracle={:?} got={:?}",
                                lo, hi, k, n, oracle, got
                            ));
                        }
                    }
                }
            }
        }
        assert!(fails.is_empty(), "DELETE mismatches:\n{}", fails.join("\n"));
    }

    #[test]
    fn fuzz_move_against_set_oracle() {
        let g = 12u32;
        let mut fails = Vec::new();
        for lo in 1..=g {
            for hi in lo..=g {
                for a in 1..=g {
                    for n in 1..=g {
                        if a + n - 1 > g {
                            continue;
                        }
                        for b in 1..=(g + 1) {
                            let imgs: Vec<u32> =
                                (lo..=hi).map(|r| move_row_sigma(r, a, n, b)).collect();
                            let mn = *imgs.iter().min().unwrap();
                            let mx = *imgs.iter().max().unwrap();
                            let oracle = if mx - mn == hi - lo {
                                Some((mn, mx))
                            } else {
                                None
                            };
                            let got = parse_res(
                                &shift_body(&body_for(lo, hi), &move_edit(a, n, b)),
                                lo,
                                hi,
                            );
                            if got != oracle {
                                fails.push(format!(
                                    "MOV lo={} hi={} a={} n={} b={} oracle={:?} got={:?}",
                                    lo, hi, a, n, b, oracle, got
                                ));
                            }
                        }
                    }
                }
            }
        }
        assert!(fails.is_empty(), "MOVE mismatches:\n{}", fails.join("\n"));
    }

    #[test]
    fn fuzz_move_sigma_independent() {
        let g = 12u32;
        let mut fails = Vec::new();
        for a in 1..=g {
            for n in 1..=g {
                if a + n - 1 > g {
                    continue;
                }
                for b in 1..=(g + 1) {
                    // Physically build the new order: remove block [a,a+n), reinsert
                    // before the first surviving element whose ORIGINAL index >= b.
                    let block: Vec<u32> = (a..a + n).collect();
                    let rem: Vec<u32> = (1..=g).filter(|r| !(*r >= a && *r < a + n)).collect();
                    let pos = rem.iter().position(|&r| r >= b).unwrap_or(rem.len());
                    let mut newlist = rem[..pos].to_vec();
                    newlist.extend(block.iter().copied());
                    newlist.extend_from_slice(&rem[pos..]);
                    // sigma_oracle(orig) = 1-based new index of element==orig
                    for orig in 1..=g {
                        let want = newlist.iter().position(|&r| r == orig).unwrap() as u32 + 1;
                        let got = move_row_sigma(orig, a, n, b);
                        if got != want {
                            fails.push(format!(
                                "a={} n={} b={} orig={} want={} got={}",
                                a, n, b, orig, want, got
                            ));
                        }
                    }
                }
            }
        }
        assert!(fails.is_empty(), "sigma mismatches:\n{}", fails.join("\n"));
    }
}
