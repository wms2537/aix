//! GROUPBY and PIVOTBY: pure data aggregation functions (no PivotTable object).
//!
//! Both functions group the rows of the input data by the values of one or
//! more key columns and aggregate a values column per group with either an
//! eta-reduced function name (`SUM`, `AVERAGE`, ...) or a LAMBDA.
//!
//! Microsoft documents the argument enums but not the exact output text of
//! total labels or the header-inference rule; those details are marked [U]
//! (undocumented, implementation-defined) below, following the semantics
//! spec's confidence legend.
//!
//! Option values that are recognized but not implemented return #VALUE! with
//! an explanatory message instead of a possibly wrong grid:
//!   * `field_headers` 2 (generate headers) in both functions: the generated
//!     header text is undocumented.
//!   * `field_headers` 3 in PIVOTBY: the placement of the shown headers in
//!     the pivot layout is undocumented.
//!   * `field_relationship` 1 (table) in GROUPBY.
//!   * sort indices that point at aggregate columns in PIVOTBY.
//!   * more than one values column in PIVOTBY.
//!   * eta-reduced ARRAYTOTEXT, CONCAT and MODE.SNGL.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::{
    calc_result::CalcResult,
    expressions::{parser::ArrayNode, parser::Node, token::Error, types::CellReferenceIndex},
    model::Model,
};

// ── Value helpers ─────────────────────────────────────────────────────────────

/// A hashable, equality-normalised representation of a single cell value.
/// Strings are uppercased (grouping is case-insensitive, like UNIQUE).
#[derive(Hash, Eq, PartialEq)]
enum CellKey {
    Number(u64),
    Boolean(bool),
    Str(String),
    Error(u8),
    Empty,
}

fn error_discriminant(e: &Error) -> u8 {
    match e {
        Error::REF => 0,
        Error::NAME => 1,
        Error::VALUE => 2,
        Error::DIV => 3,
        Error::NA => 4,
        Error::NUM => 5,
        Error::ERROR => 6,
        Error::NIMPL => 7,
        Error::SPILL => 8,
        Error::CALC => 9,
        Error::CIRC => 10,
        Error::NULL => 11,
        Error::BLOCKED => 12,
        Error::CONNECT => 13,
    }
}

fn cell_key(node: &ArrayNode) -> CellKey {
    match node {
        // Normalize -0.0 to 0.0 so both zeros land in one group (Excel treats
        // them as the same value; the bit patterns differ).
        ArrayNode::Number(n) => CellKey::Number(if *n == 0.0 { 0.0f64 } else { *n }.to_bits()),
        ArrayNode::Boolean(b) => CellKey::Boolean(*b),
        ArrayNode::String(s) => CellKey::Str(s.to_uppercase()),
        ArrayNode::Error(e) => CellKey::Error(error_discriminant(e)),
        ArrayNode::Empty => CellKey::Empty,
    }
}

/// Type rank following Excel's sort order: numbers < strings < booleans;
/// errors and empty cells go last (empty after everything).
fn type_rank(node: &ArrayNode) -> u8 {
    match node {
        ArrayNode::Number(_) => 0,
        ArrayNode::String(_) => 1,
        ArrayNode::Boolean(_) => 2,
        ArrayNode::Error(_) => 3,
        ArrayNode::Empty => 4,
    }
}

/// Compare two cell values following Excel's sort rules. Empty cells always
/// sort last regardless of direction (like SORT).
fn value_cmp(a: &ArrayNode, b: &ArrayNode, ascending: bool) -> Ordering {
    let a_empty = matches!(a, ArrayNode::Empty);
    let b_empty = matches!(b, ArrayNode::Empty);
    match (a_empty, b_empty) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    let ord = match (a, b) {
        (ArrayNode::Number(x), ArrayNode::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (ArrayNode::String(x), ArrayNode::String(y)) => x.to_uppercase().cmp(&y.to_uppercase()),
        (ArrayNode::Boolean(x), ArrayNode::Boolean(y)) => x.cmp(y),
        (ArrayNode::Error(x), ArrayNode::Error(y)) => {
            error_discriminant(x).cmp(&error_discriminant(y))
        }
        _ => type_rank(a).cmp(&type_rank(b)),
    };
    if ascending {
        ord
    } else {
        ord.reverse()
    }
}

fn value_text(node: &ArrayNode) -> String {
    match node {
        ArrayNode::String(s) => s.clone(),
        ArrayNode::Number(n) => n.to_string(),
        ArrayNode::Boolean(b) => (if *b { "TRUE" } else { "FALSE" }).to_string(),
        ArrayNode::Error(e) => e.to_string(),
        ArrayNode::Empty => String::new(),
    }
}

/// Label of a subtotal row/column: "<value> Total". [U] Excel does not
/// document the exact text; "<value> Total" matches the published examples.
/// An empty/blank key still gets the "<value> Total" shape (" Total", with
/// the empty value prefix) so its subtotal stays distinguishable from the
/// grand-total row's bare "Total" label.
fn total_label(key: &ArrayNode) -> ArrayNode {
    let text = value_text(key);
    ArrayNode::String(format!("{text} Total"))
}

/// A blank padding cell. ArrayNode::Empty spills as the number 0, so labels
/// are padded with empty strings instead.
fn blank() -> ArrayNode {
    ArrayNode::String(String::new())
}

fn column_of(values: &[ArrayNode]) -> Vec<Vec<ArrayNode>> {
    values.iter().map(|v| vec![v.clone()]).collect()
}

/// Intersection of two ascending lists of row indices.
fn intersect_rows(a: &[usize], b: &[usize]) -> Vec<usize> {
    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                result.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    result
}

fn subset_of(column: &[ArrayNode], rows: &[usize]) -> Vec<ArrayNode> {
    rows.iter().map(|&r| column[r].clone()).collect()
}

// ── Aggregation functions ─────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum EtaFunction {
    Sum,
    Average,
    Count,
    CountA,
    Max,
    Min,
    Median,
    Product,
    StDevS,
    StDevP,
    VarS,
    VarP,
    PercentOf,
}

enum AggFunction {
    Eta(EtaFunction),
    Lambda {
        lambda: CalcResult,
        param_count: usize,
    },
}

impl AggFunction {
    /// Two-argument aggregations receive a second, "relative to" subset
    /// (the grand total in GROUPBY, the relative_to selection in PIVOTBY).
    fn is_two_arg(&self) -> bool {
        match self {
            AggFunction::Eta(f) => *f == EtaFunction::PercentOf,
            AggFunction::Lambda { param_count, .. } => *param_count == 2,
        }
    }
}

/// Sum of the numbers in a subset. Text, booleans and empty cells are
/// ignored (range semantics); errors propagate.
fn subset_sum(subset: &[ArrayNode]) -> Result<f64, Error> {
    let mut total = 0.0;
    for node in subset {
        match node {
            ArrayNode::Number(n) => total += n,
            ArrayNode::Error(e) => return Err(e.clone()),
            _ => {}
        }
    }
    Ok(total)
}

fn subset_numbers(subset: &[ArrayNode]) -> Result<Vec<f64>, Error> {
    let mut numbers = Vec::new();
    for node in subset {
        match node {
            ArrayNode::Number(n) => numbers.push(*n),
            ArrayNode::Error(e) => return Err(e.clone()),
            _ => {}
        }
    }
    Ok(numbers)
}

fn variance(numbers: &[f64], sample: bool) -> Option<f64> {
    let n = numbers.len();
    if n < if sample { 2 } else { 1 } {
        return None;
    }
    let mean = numbers.iter().sum::<f64>() / n as f64;
    let sum_sq: f64 = numbers.iter().map(|v| (v - mean) * (v - mean)).sum();
    let denominator = if sample { n - 1 } else { n } as f64;
    Some(sum_sq / denominator)
}

fn eta_aggregate(function: EtaFunction, subset: &[ArrayNode], denom: &[ArrayNode]) -> ArrayNode {
    let numbers = match subset_numbers(subset) {
        Ok(n) => n,
        Err(e) => return ArrayNode::Error(e),
    };
    match function {
        EtaFunction::Sum => ArrayNode::Number(numbers.iter().sum()),
        EtaFunction::Average => {
            if numbers.is_empty() {
                ArrayNode::Error(Error::DIV)
            } else {
                ArrayNode::Number(numbers.iter().sum::<f64>() / numbers.len() as f64)
            }
        }
        EtaFunction::Count => ArrayNode::Number(numbers.len() as f64),
        EtaFunction::CountA => ArrayNode::Number(
            subset
                .iter()
                .filter(|n| !matches!(n, ArrayNode::Empty))
                .count() as f64,
        ),
        // MAX/MIN of a subset with no numbers is 0, like over empty ranges.
        EtaFunction::Max => {
            ArrayNode::Number(numbers.iter().cloned().fold(f64::NAN, f64::max)).nan_to_zero()
        }
        EtaFunction::Min => {
            ArrayNode::Number(numbers.iter().cloned().fold(f64::NAN, f64::min)).nan_to_zero()
        }
        EtaFunction::Median => {
            if numbers.is_empty() {
                return ArrayNode::Error(Error::NUM);
            }
            let mut sorted = numbers;
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            let n = sorted.len();
            ArrayNode::Number(if n % 2 == 1 {
                sorted[n / 2]
            } else {
                (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
            })
        }
        EtaFunction::Product => {
            if numbers.is_empty() {
                ArrayNode::Number(0.0)
            } else {
                ArrayNode::Number(numbers.iter().product())
            }
        }
        EtaFunction::StDevS | EtaFunction::StDevP | EtaFunction::VarS | EtaFunction::VarP => {
            let sample = matches!(function, EtaFunction::StDevS | EtaFunction::VarS);
            match variance(&numbers, sample) {
                Some(v) => {
                    if matches!(function, EtaFunction::StDevS | EtaFunction::StDevP) {
                        ArrayNode::Number(v.sqrt())
                    } else {
                        ArrayNode::Number(v)
                    }
                }
                None => ArrayNode::Error(Error::DIV),
            }
        }
        EtaFunction::PercentOf => {
            let total = match subset_sum(denom) {
                Ok(t) => t,
                Err(e) => return ArrayNode::Error(e),
            };
            if total == 0.0 {
                return ArrayNode::Error(Error::DIV);
            }
            ArrayNode::Number(numbers.iter().sum::<f64>() / total)
        }
    }
}

trait NanToZero {
    fn nan_to_zero(self) -> ArrayNode;
}

impl NanToZero for ArrayNode {
    fn nan_to_zero(self) -> ArrayNode {
        match self {
            ArrayNode::Number(n) if n.is_nan() => ArrayNode::Number(0.0),
            other => other,
        }
    }
}

// ── Grouping tree ─────────────────────────────────────────────────────────────

struct GroupNode {
    /// Field value of this group (Empty for the root).
    key: ArrayNode,
    /// Data row indices (ascending) covered by this subtree.
    rows: Vec<usize>,
    /// Child groups; empty at leaf level.
    children: Vec<GroupNode>,
    /// One aggregate per values column, for the whole subtree.
    aggs: Vec<ArrayNode>,
}

fn build_group_children(fields: &[Vec<ArrayNode>], rows: &[usize], level: usize) -> Vec<GroupNode> {
    let mut index_of: HashMap<CellKey, usize> = HashMap::new();
    let mut groups: Vec<(ArrayNode, Vec<usize>)> = Vec::new();
    for &row in rows {
        let value = &fields[level][row];
        let key = cell_key(value);
        match index_of.get(&key) {
            Some(&i) => groups[i].1.push(row),
            None => {
                index_of.insert(key, groups.len());
                groups.push((value.clone(), vec![row]));
            }
        }
    }
    groups
        .into_iter()
        .map(|(key, group_rows)| {
            let children = if level + 1 < fields.len() {
                build_group_children(fields, &group_rows, level + 1)
            } else {
                Vec::new()
            };
            GroupNode {
                key,
                rows: group_rows,
                children,
                aggs: Vec::new(),
            }
        })
        .collect()
}

fn build_group_tree(fields: &[Vec<ArrayNode>], rows: Vec<usize>) -> GroupNode {
    let children = build_group_children(fields, &rows, 0);
    GroupNode {
        key: ArrayNode::Empty,
        rows,
        children,
        aggs: Vec::new(),
    }
}

/// Compare two sibling groups at `level` (0-based field index) following the
/// sort specification. [U] With the hierarchy field relationship the group
/// nesting always survives sorting: entries for other field levels are
/// skipped, and entries past the field count refer to aggregate columns and
/// order every level by its subtree aggregate. Ties fall back to the field
/// value, ascending (the default hierarchy sort).
fn sibling_cmp(a: &GroupNode, b: &GroupNode, level: usize, spec: &[i32], k: usize) -> Ordering {
    for &entry in spec {
        let index = entry.unsigned_abs() as usize - 1;
        let ascending = entry > 0;
        let ord = if index < k {
            if index == level {
                value_cmp(&a.key, &b.key, ascending)
            } else {
                Ordering::Equal
            }
        } else {
            value_cmp(&a.aggs[index - k], &b.aggs[index - k], ascending)
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    value_cmp(&a.key, &b.key, true)
}

fn sort_tree(node: &mut GroupNode, level: usize, spec: &[i32], k: usize) {
    node.children
        .sort_by(|a, b| sibling_cmp(a, b, level, spec, k));
    for child in &mut node.children {
        sort_tree(child, level + 1, spec, k);
    }
}

/// One emitted line of an axis: the k label cells, the subtree aggregates,
/// the data rows of the group and the data rows of its parent group (needed
/// for relative_to 3/4).
struct AxisLine {
    labels: Vec<ArrayNode>,
    aggs: Vec<ArrayNode>,
    rows: Vec<usize>,
    parent_rows: Vec<usize>,
}

/// Flattens a group tree into lines: leaf lines carry the full key path,
/// total lines carry "Total"/"<value> Total" labels. Totals are shown for
/// nodes at depth < total_levels; totals_first places them before their
/// group (negative total_depth).
#[allow(clippy::too_many_arguments)]
fn emit_axis_lines(
    node: &GroupNode,
    depth: usize,
    k: usize,
    total_levels: usize,
    totals_first: bool,
    path: &mut Vec<ArrayNode>,
    parent_rows: &[usize],
    out: &mut Vec<AxisLine>,
) {
    if depth == k {
        out.push(AxisLine {
            labels: path.clone(),
            aggs: node.aggs.clone(),
            rows: node.rows.clone(),
            parent_rows: parent_rows.to_vec(),
        });
        return;
    }
    let total_line = if depth < total_levels {
        let mut labels = vec![blank(); k];
        if depth == 0 {
            labels[0] = ArrayNode::String("Total".to_string());
        } else {
            labels[depth - 1] = total_label(&node.key);
        }
        Some(AxisLine {
            labels,
            aggs: node.aggs.clone(),
            rows: node.rows.clone(),
            parent_rows: parent_rows.to_vec(),
        })
    } else {
        None
    };
    if totals_first {
        if let Some(line) = total_line {
            out.push(line);
            for child in &node.children {
                path.push(child.key.clone());
                emit_axis_lines(
                    child,
                    depth + 1,
                    k,
                    total_levels,
                    totals_first,
                    path,
                    &node.rows,
                    out,
                );
                path.pop();
            }
            return;
        }
    }
    for child in &node.children {
        path.push(child.key.clone());
        emit_axis_lines(
            child,
            depth + 1,
            k,
            total_levels,
            totals_first,
            path,
            &node.rows,
            out,
        );
        path.pop();
    }
    if !totals_first {
        if let Some(line) = total_line {
            out.push(line);
        }
    }
}

// ── Shared argument handling ──────────────────────────────────────────────────

/// An optional argument counts as omitted when it is missing or blank.
fn optional_arg(args: &[Node], index: usize) -> Option<&Node> {
    match args.get(index) {
        None | Some(Node::EmptyArgKind) => None,
        Some(node) => Some(node),
    }
}

/// Header inference [U]: Microsoft documents only that the default "is
/// inferred based on the data". Headers are assumed when every first-row
/// cell of the field and values arrays is text and at least one second-row
/// cell of the values is a number (text-then-number).
fn infer_headers(field_arrays: &[&Vec<Vec<ArrayNode>>], values: &[Vec<ArrayNode>]) -> bool {
    if values.len() < 2 {
        return false;
    }
    for array in field_arrays {
        if !array[0].iter().all(|c| matches!(c, ArrayNode::String(_))) {
            return false;
        }
    }
    if !values[0].iter().all(|c| matches!(c, ArrayNode::String(_))) {
        return false;
    }
    values[1].iter().any(|c| matches!(c, ArrayNode::Number(_)))
}

impl<'a> Model<'a> {
    /// Resolves the `function` argument: an eta-reduced aggregation name
    /// (a bare `SUM`, which parses as an unbound variable) or a LAMBDA value.
    fn resolve_agg_function(
        &mut self,
        node: &Node,
        cell: CellReferenceIndex,
    ) -> Result<AggFunction, CalcResult> {
        if let Node::NamedVariableKind { name, id: None } = node {
            let clean = name.trim_start_matches("_xlfn.").to_uppercase();
            let eta = match clean.as_str() {
                "SUM" => Some(EtaFunction::Sum),
                "AVERAGE" => Some(EtaFunction::Average),
                "COUNT" => Some(EtaFunction::Count),
                "COUNTA" => Some(EtaFunction::CountA),
                "MAX" => Some(EtaFunction::Max),
                "MIN" => Some(EtaFunction::Min),
                "MEDIAN" => Some(EtaFunction::Median),
                "PRODUCT" => Some(EtaFunction::Product),
                "STDEV.S" => Some(EtaFunction::StDevS),
                "STDEV.P" => Some(EtaFunction::StDevP),
                "VAR.S" => Some(EtaFunction::VarS),
                "VAR.P" => Some(EtaFunction::VarP),
                "PERCENTOF" => Some(EtaFunction::PercentOf),
                // Excel additionally accepts eta-reduced ARRAYTOTEXT, CONCAT
                // and MODE.SNGL; their group semantics (join separator, tie
                // breaking) are undocumented, so they are rejected instead of
                // producing a possibly wrong grid.
                "ARRAYTOTEXT" | "CONCAT" | "MODE.SNGL" => {
                    return Err(CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        format!("Eta-reduced {clean} is not supported"),
                    ));
                }
                _ => None,
            };
            if let Some(function) = eta {
                return Ok(AggFunction::Eta(function));
            }
        }
        match self.evaluate_node_in_context(node, cell) {
            CalcResult::Lambda(id) => {
                let param_count = match self.lambdas.get(&id) {
                    Some((parameters, _)) => parameters.len(),
                    None => 0,
                };
                if param_count == 0 || param_count > 2 {
                    return Err(CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "function LAMBDA must take one or two parameters".to_string(),
                    ));
                }
                Ok(AggFunction::Lambda {
                    lambda: CalcResult::Lambda(id),
                    param_count,
                })
            }
            error @ CalcResult::Error { .. } => Err(error),
            _ => Err(CalcResult::new_error(
                Error::VALUE,
                cell,
                "function must be a LAMBDA or an aggregation like SUM".to_string(),
            )),
        }
    }

    /// Applies the aggregation to a subset of one values column. `denom` is
    /// the second subset handed to two-argument functions.
    fn apply_agg(
        &mut self,
        function: &AggFunction,
        subset: &[ArrayNode],
        denom: &[ArrayNode],
        cell: CellReferenceIndex,
    ) -> ArrayNode {
        match function {
            AggFunction::Eta(eta) => eta_aggregate(*eta, subset, denom),
            AggFunction::Lambda {
                lambda,
                param_count,
            } => {
                let mut values = vec![CalcResult::Array(column_of(subset))];
                if *param_count == 2 {
                    values.push(CalcResult::Array(column_of(denom)));
                }
                match self.call_lambda_with_values(lambda.clone(), values, cell) {
                    CalcResult::Number(n) => ArrayNode::Number(n),
                    CalcResult::Boolean(b) => ArrayNode::Boolean(b),
                    CalcResult::String(s) => ArrayNode::String(s),
                    CalcResult::Error { error, .. } => ArrayNode::Error(error),
                    CalcResult::EmptyCell | CalcResult::EmptyArg => ArrayNode::Empty,
                    // The lambda must return a scalar; 1x1 arrays are
                    // unwrapped, anything larger would nest arrays.
                    CalcResult::Array(a) => {
                        if a.len() == 1 && a[0].len() == 1 {
                            a[0][0].clone()
                        } else {
                            ArrayNode::Error(Error::CALC)
                        }
                    }
                    _ => ArrayNode::Error(Error::VALUE),
                }
            }
        }
    }

    fn compute_tree_aggs(
        &mut self,
        node: &mut GroupNode,
        function: &AggFunction,
        values: &[Vec<ArrayNode>],
        denoms: &[Vec<ArrayNode>],
        cell: CellReferenceIndex,
    ) {
        node.aggs = (0..values.len())
            .map(|j| {
                let subset = subset_of(&values[j], &node.rows);
                self.apply_agg(function, &subset, &denoms[j], cell)
            })
            .collect();
        let mut children = std::mem::take(&mut node.children);
        for child in &mut children {
            self.compute_tree_aggs(child, function, values, denoms, cell);
        }
        node.children = children;
    }

    /// Evaluates a `field_headers` argument; `None` means "infer".
    fn get_field_headers_arg(
        &mut self,
        args: &[Node],
        index: usize,
        cell: CellReferenceIndex,
    ) -> Result<Option<i32>, CalcResult> {
        match optional_arg(args, index) {
            None => Ok(None),
            Some(node) => {
                let n = match self.get_number(node, cell) {
                    Ok(f) => f.trunc() as i32,
                    Err(s) => return Err(s),
                };
                if !(0..=3).contains(&n) {
                    return Err(CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "field_headers must be between 0 and 3".to_string(),
                    ));
                }
                Ok(Some(n))
            }
        }
    }

    /// Evaluates a total_depth argument into (levels, totals_first). The
    /// magnitude is the number of total levels (grand, then subtotals),
    /// clamped to the field count [U]; the sign selects bottom (+) or
    /// top (-) placement.
    fn get_total_depth_arg(
        &mut self,
        args: &[Node],
        index: usize,
        field_count: usize,
        cell: CellReferenceIndex,
    ) -> Result<(usize, bool), CalcResult> {
        let depth = match optional_arg(args, index) {
            None => 1,
            Some(node) => match self.get_number(node, cell) {
                Ok(f) => f.trunc() as i64,
                Err(s) => return Err(s),
            },
        };
        let levels = (depth.unsigned_abs() as usize).min(field_count);
        Ok((levels, depth < 0))
    }

    /// Evaluates a sort_order argument into signed 1-based column indices.
    /// `max_index` is the largest allowed magnitude; `sortable_by_value`
    /// controls whether indices past the field columns are accepted.
    fn get_sort_order_arg(
        &mut self,
        args: &[Node],
        index: usize,
        field_count: usize,
        max_index: usize,
        sortable_by_value: bool,
        cell: CellReferenceIndex,
    ) -> Result<Vec<i32>, CalcResult> {
        let node = match optional_arg(args, index) {
            None => return Ok(Vec::new()),
            Some(node) => node,
        };
        let data = self.eval_to_array(node, cell)?;
        let mut spec = Vec::new();
        for row in &data {
            for entry in row {
                let n = match entry {
                    ArrayNode::Number(n) => n.trunc() as i32,
                    _ => {
                        return Err(CalcResult::new_error(
                            Error::VALUE,
                            cell,
                            "sort_order entries must be numbers".to_string(),
                        ));
                    }
                };
                let magnitude = n.unsigned_abs() as usize;
                if n == 0 || magnitude > max_index {
                    return Err(CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "sort_order index out of range".to_string(),
                    ));
                }
                if magnitude > field_count && !sortable_by_value {
                    // PIVOTBY axis sorting by aggregate values is not
                    // implemented; only field columns can be sort keys.
                    return Err(CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "sorting by aggregate columns is not supported here".to_string(),
                    ));
                }
                spec.push(n);
            }
        }
        Ok(spec)
    }

    /// Evaluates a filter_array argument into included data-row indices.
    /// The filter must have one entry per data row; a filter that still
    /// includes the stripped header row is also accepted [U].
    fn get_filter_rows_arg(
        &mut self,
        args: &[Node],
        index: usize,
        data_rows: usize,
        raw_rows: usize,
        cell: CellReferenceIndex,
    ) -> Result<Vec<usize>, CalcResult> {
        let node = match optional_arg(args, index) {
            None => return Ok((0..data_rows).collect()),
            Some(node) => node,
        };
        let data = self.eval_to_array(node, cell)?;
        let flat: Vec<&ArrayNode> = if !data.is_empty() && data[0].len() == 1 {
            data.iter().map(|row| &row[0]).collect()
        } else if data.len() == 1 {
            data[0].iter().collect()
        } else {
            return Err(CalcResult::new_error(
                Error::VALUE,
                cell,
                "filter_array must be a single row or column".to_string(),
            ));
        };
        let entries: &[&ArrayNode] = if flat.len() == data_rows {
            &flat
        } else if flat.len() == raw_rows && raw_rows != data_rows {
            &flat[1..]
        } else {
            return Err(CalcResult::new_error(
                Error::VALUE,
                cell,
                "filter_array length must match the data rows".to_string(),
            ));
        };
        let mut rows = Vec::new();
        for (row, entry) in entries.iter().enumerate() {
            let include = match entry {
                ArrayNode::Boolean(b) => *b,
                ArrayNode::Number(n) => *n != 0.0,
                ArrayNode::Empty => false,
                ArrayNode::String(_) => {
                    return Err(CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "filter_array entries must be logical values".to_string(),
                    ));
                }
                ArrayNode::Error(e) => {
                    return Err(CalcResult::new_error(e.clone(), cell, String::new()));
                }
            };
            if include {
                rows.push(row);
            }
        }
        Ok(rows)
    }

    // ── GROUPBY ───────────────────────────────────────────────────────────────

    /// `=GROUPBY(row_fields, values, function, [field_headers], [total_depth],
    ///           [sort_order], [filter_array], [field_relationship])`
    ///
    /// Groups the rows of `values` by the key columns in `row_fields` and
    /// aggregates every values column per group. Pure data aggregation, no
    /// PivotTable object involved.
    pub(crate) fn fn_groupby(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() < 3 || args.len() > 8 {
            return CalcResult::new_args_number_error(cell);
        }
        let row_fields = match self.eval_to_array(&args[0], cell) {
            Ok(d) => d,
            Err(e) => return e,
        };
        let values = match self.eval_to_array(&args[1], cell) {
            Ok(d) => d,
            Err(e) => return e,
        };
        if row_fields.is_empty()
            || row_fields[0].is_empty()
            || values.len() != row_fields.len()
            || values[0].is_empty()
        {
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "row_fields and values must have the same number of rows".to_string(),
            );
        }
        let function = match self.resolve_agg_function(&args[2], cell) {
            Ok(f) => f,
            Err(e) => return e,
        };
        let field_headers = match self.get_field_headers_arg(args, 3, cell) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if field_headers == Some(2) {
            // Excel generates header text for this option; the generated
            // labels are undocumented, so the option is rejected rather
            // than guessed.
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "field_headers 2 (generate headers) is not supported".to_string(),
            );
        }
        // field_relationship: 0 hierarchy (default) / 1 table.
        if let Some(node) = optional_arg(args, 7) {
            let n = match self.get_number(node, cell) {
                Ok(f) => f.trunc() as i32,
                Err(s) => return s,
            };
            match n {
                0 => {}
                1 => {
                    // The table relationship changes subtotal and sort
                    // semantics in ways Microsoft does not document; it is
                    // not implemented.
                    return CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "field_relationship 1 (table) is not supported".to_string(),
                    );
                }
                _ => {
                    return CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "field_relationship must be 0 or 1".to_string(),
                    );
                }
            }
        }

        let raw_rows = row_fields.len();
        let has_headers = match field_headers {
            Some(1) | Some(3) => true,
            Some(_) => false,
            None => infer_headers(&[&row_fields], &values),
        };
        let first_data_row = usize::from(has_headers);
        if raw_rows <= first_data_row {
            return CalcResult::new_error(Error::CALC, cell, "empty array".to_string());
        }
        let k = row_fields[0].len();
        let m = values[0].len();

        // Column-major copies of the data rows.
        let field_columns: Vec<Vec<ArrayNode>> = (0..k)
            .map(|j| {
                row_fields[first_data_row..]
                    .iter()
                    .map(|r| r[j].clone())
                    .collect()
            })
            .collect();
        let value_columns: Vec<Vec<ArrayNode>> = (0..m)
            .map(|j| {
                values[first_data_row..]
                    .iter()
                    .map(|r| r[j].clone())
                    .collect()
            })
            .collect();
        let data_rows = raw_rows - first_data_row;

        let (total_levels, totals_first) = match self.get_total_depth_arg(args, 4, k, cell) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let sort_spec = match self.get_sort_order_arg(args, 5, k, k + m, true, cell) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let included = match self.get_filter_rows_arg(args, 6, data_rows, raw_rows, cell) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if included.is_empty() {
            return CalcResult::new_error(Error::CALC, cell, "empty array".to_string());
        }

        // Two-argument functions compare each group against the grand total.
        let denoms: Vec<Vec<ArrayNode>> = if function.is_two_arg() {
            value_columns
                .iter()
                .map(|column| subset_of(column, &included))
                .collect()
        } else {
            vec![Vec::new(); m]
        };

        let mut tree = build_group_tree(&field_columns, included);
        self.compute_tree_aggs(&mut tree, &function, &value_columns, &denoms, cell);
        sort_tree(&mut tree, 0, &sort_spec, k);

        let all_rows = tree.rows.clone();
        let mut lines = Vec::new();
        emit_axis_lines(
            &tree,
            0,
            k,
            total_levels,
            totals_first,
            &mut Vec::new(),
            &all_rows,
            &mut lines,
        );

        let mut result: Vec<Vec<ArrayNode>> = Vec::with_capacity(lines.len() + 1);
        if field_headers == Some(3) {
            let mut header: Vec<ArrayNode> = row_fields[0].clone();
            header.extend(values[0].iter().cloned());
            result.push(header);
        }
        for line in lines {
            let mut row = line.labels;
            row.extend(line.aggs);
            result.push(row);
        }
        CalcResult::Array(result)
    }

    // ── PIVOTBY ───────────────────────────────────────────────────────────────

    /// `=PIVOTBY(row_fields, col_fields, values, function, [field_headers],
    ///           [row_total_depth], [row_sort_order], [col_total_depth],
    ///           [col_sort_order], [filter_array], [relative_to])`
    ///
    /// Groups the data along two axes and aggregates the values column at
    /// each intersection. Pure data aggregation, "not directly related to
    /// Excel's PivotTable feature".
    pub(crate) fn fn_pivotby(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() < 4 || args.len() > 11 {
            return CalcResult::new_args_number_error(cell);
        }
        let row_fields = match self.eval_to_array(&args[0], cell) {
            Ok(d) => d,
            Err(e) => return e,
        };
        let col_fields = match self.eval_to_array(&args[1], cell) {
            Ok(d) => d,
            Err(e) => return e,
        };
        let values = match self.eval_to_array(&args[2], cell) {
            Ok(d) => d,
            Err(e) => return e,
        };
        if row_fields.is_empty()
            || row_fields[0].is_empty()
            || col_fields.len() != row_fields.len()
            || col_fields[0].is_empty()
            || values.len() != row_fields.len()
            || values[0].is_empty()
        {
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "row_fields, col_fields and values must have the same number of rows".to_string(),
            );
        }
        if values[0].len() != 1 {
            // The layout of several aggregate columns per pivot column is
            // undocumented; only one values column is supported.
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "PIVOTBY supports a single values column".to_string(),
            );
        }
        let function = match self.resolve_agg_function(&args[3], cell) {
            Ok(f) => f,
            Err(e) => return e,
        };
        let field_headers = match self.get_field_headers_arg(args, 4, cell) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if field_headers == Some(2) || field_headers == Some(3) {
            // Generated header text (2) and the placement of shown headers
            // within the pivot layout (3) are undocumented; both options are
            // rejected rather than guessed.
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "field_headers 2 and 3 are not supported in PIVOTBY".to_string(),
            );
        }
        // relative_to: 0 column totals (default), 1 row totals, 2 grand
        // total, 3 parent column total, 4 parent row total. Only used by
        // two-argument functions.
        let relative_to = match optional_arg(args, 10) {
            None => 0,
            Some(node) => {
                let n = match self.get_number(node, cell) {
                    Ok(f) => f.trunc() as i32,
                    Err(s) => return s,
                };
                if !(0..=4).contains(&n) {
                    return CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "relative_to must be between 0 and 4".to_string(),
                    );
                }
                n
            }
        };

        let raw_rows = row_fields.len();
        let has_headers = match field_headers {
            Some(1) => true,
            Some(_) => false,
            None => infer_headers(&[&row_fields, &col_fields], &values),
        };
        let first_data_row = usize::from(has_headers);
        if raw_rows <= first_data_row {
            return CalcResult::new_error(Error::CALC, cell, "empty array".to_string());
        }
        let k_row = row_fields[0].len();
        let k_col = col_fields[0].len();

        let row_columns: Vec<Vec<ArrayNode>> = (0..k_row)
            .map(|j| {
                row_fields[first_data_row..]
                    .iter()
                    .map(|r| r[j].clone())
                    .collect()
            })
            .collect();
        let col_columns: Vec<Vec<ArrayNode>> = (0..k_col)
            .map(|j| {
                col_fields[first_data_row..]
                    .iter()
                    .map(|r| r[j].clone())
                    .collect()
            })
            .collect();
        let value_column: Vec<ArrayNode> = values[first_data_row..]
            .iter()
            .map(|r| r[0].clone())
            .collect();
        let data_rows = raw_rows - first_data_row;

        let (row_total_levels, row_totals_first) =
            match self.get_total_depth_arg(args, 5, k_row, cell) {
                Ok(v) => v,
                Err(e) => return e,
            };
        let row_sort_spec = match self.get_sort_order_arg(args, 6, k_row, k_row, false, cell) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let (col_total_levels, col_totals_first) =
            match self.get_total_depth_arg(args, 7, k_col, cell) {
                Ok(v) => v,
                Err(e) => return e,
            };
        let col_sort_spec = match self.get_sort_order_arg(args, 8, k_col, k_col, false, cell) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let included = match self.get_filter_rows_arg(args, 9, data_rows, raw_rows, cell) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if included.is_empty() {
            return CalcResult::new_error(Error::CALC, cell, "empty array".to_string());
        }

        let mut row_tree = build_group_tree(&row_columns, included.clone());
        sort_tree(&mut row_tree, 0, &row_sort_spec, k_row);
        let mut col_tree = build_group_tree(&col_columns, included.clone());
        sort_tree(&mut col_tree, 0, &col_sort_spec, k_col);

        let all_row_rows = row_tree.rows.clone();
        let mut row_lines = Vec::new();
        emit_axis_lines(
            &row_tree,
            0,
            k_row,
            row_total_levels,
            row_totals_first,
            &mut Vec::new(),
            &all_row_rows,
            &mut row_lines,
        );
        let all_col_rows = col_tree.rows.clone();
        let mut col_lines = Vec::new();
        emit_axis_lines(
            &col_tree,
            0,
            k_col,
            col_total_levels,
            col_totals_first,
            &mut Vec::new(),
            &all_col_rows,
            &mut col_lines,
        );

        let two_arg = function.is_two_arg();
        let width = k_row + col_lines.len();
        let mut result: Vec<Vec<ArrayNode>> = Vec::with_capacity(k_col + row_lines.len());

        // Column header block: one row per column field, with blank corner
        // cells above the row-field columns.
        for level in 0..k_col {
            let mut header = vec![blank(); width];
            for (i, col_line) in col_lines.iter().enumerate() {
                header[k_row + i] = col_line.labels[level].clone();
            }
            result.push(header);
        }
        for row_line in &row_lines {
            let mut row: Vec<ArrayNode> = Vec::with_capacity(width);
            row.extend(row_line.labels.iter().cloned());
            for col_line in &col_lines {
                let cell_rows = intersect_rows(&row_line.rows, &col_line.rows);
                let subset = subset_of(&value_column, &cell_rows);
                let denom = if two_arg {
                    let denom_rows = match relative_to {
                        0 => col_line.rows.clone(),
                        1 => row_line.rows.clone(),
                        2 => included.clone(),
                        3 => intersect_rows(&row_line.rows, &col_line.parent_rows),
                        // relative_to == 4
                        _ => intersect_rows(&row_line.parent_rows, &col_line.rows),
                    };
                    subset_of(&value_column, &denom_rows)
                } else {
                    Vec::new()
                };
                row.push(self.apply_agg(&function, &subset, &denom, cell));
            }
            result.push(row);
        }
        CalcResult::Array(result)
    }
}
