use crate::{
    calc_result::CalcResult,
    expressions::{parser::ArrayNode, parser::Node, token::Error, types::CellReferenceIndex},
    model::Model,
};

/// Returns true if an ArrayNode counts as a blank/empty cell for TRIMRANGE purposes.
fn is_blank(node: &ArrayNode) -> bool {
    matches!(node, ArrayNode::Empty)
}

/// Returns true if every cell in `row` is blank.
fn row_is_empty(row: &[ArrayNode]) -> bool {
    row.iter().all(is_blank)
}

/// Returns true if every row has a blank value at column index `col`.
fn col_is_empty(data: &[Vec<ArrayNode>], col: usize) -> bool {
    data.iter().all(|row| row.get(col).is_none_or(is_blank))
}

impl<'a> Model<'a> {
    /// `=TRIMRANGE(range, [trim_rows], [trim_cols])`
    ///
    /// Trims blank rows and/or columns from the outer edges of a range or array.
    ///
    /// trim_rows / trim_cols:
    ///   * 0 – no trimming
    ///   * 1 – trim leading (top / left) blanks only
    ///   * 2 – trim trailing (bottom / right) blanks only
    ///   * 3 – trim both leading and trailing blanks (default)
    ///
    /// Returns `#REF!` if every cell in the range is blank.
    pub(crate) fn fn_trimrange(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.is_empty() || args.len() > 3 {
            return CalcResult::new_args_number_error(cell);
        }

        // Note: `eval_to_array` is not used here because it converts a single
        // blank cell into `0`, while TRIMRANGE must keep it blank so that an
        // all-blank input returns #REF!.
        let data = match self.evaluate_node_in_context(&args[0], cell) {
            CalcResult::Range { left, right } => self.evaluate_range(left, right),
            CalcResult::Array(arr) => arr,
            CalcResult::Number(n) => vec![vec![ArrayNode::Number(n)]],
            CalcResult::Boolean(b) => vec![vec![ArrayNode::Boolean(b)]],
            CalcResult::String(s) => vec![vec![ArrayNode::String(s)]],
            CalcResult::EmptyCell | CalcResult::EmptyArg => vec![vec![ArrayNode::Empty]],
            err @ CalcResult::Error { .. } => return err,
            CalcResult::Lambda(_) => {
                return CalcResult::new_error(
                    Error::VALUE,
                    cell,
                    "LAMBDA cannot be used as an array".to_string(),
                )
            }
        };

        if data.is_empty() || data[0].is_empty() {
            return CalcResult::new_error(Error::REF, cell, "Range is empty".to_string());
        }

        // An explicitly empty argument (`=TRIMRANGE(range,,2)`) keeps the
        // documented default 3, like an omitted one — it must not coerce to
        // 0 ("no trim").
        let trim_rows = match args.get(1) {
            None | Some(Node::EmptyArgKind) => 3,
            Some(node) => match self.get_number(node, cell) {
                Ok(n) => n as i32,
                Err(e) => return e,
            },
        };

        let trim_cols = match args.get(2) {
            None | Some(Node::EmptyArgKind) => 3,
            Some(node) => match self.get_number(node, cell) {
                Ok(n) => n as i32,
                Err(e) => return e,
            },
        };

        if !matches!(trim_rows, 0..=3) {
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "trim_rows must be 0, 1, 2, or 3".to_string(),
            );
        }
        if !matches!(trim_cols, 0..=3) {
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "trim_cols must be 0, 1, 2, or 3".to_string(),
            );
        }

        // All-blank input -> #REF!, unconditionally (spec: "All-blank input
        // → #REF!") — even with trimming disabled (trim_rows/cols 0), where
        // the slice bounds below would otherwise keep the blank grid.
        if data.iter().all(|row| row.iter().all(is_blank)) {
            return CalcResult::new_error(
                Error::REF,
                cell,
                "TRIMRANGE input is all blank".to_string(),
            );
        }

        let num_rows = data.len();
        let num_cols = data[0].len();

        // Determine row slice bounds
        let row_start = if trim_rows == 1 || trim_rows == 3 {
            (0..num_rows)
                .find(|&r| !row_is_empty(&data[r]))
                .unwrap_or(num_rows)
        } else {
            0
        };
        let row_end = if trim_rows == 2 || trim_rows == 3 {
            (0..num_rows)
                .rev()
                .find(|&r| !row_is_empty(&data[r]))
                .map(|r| r + 1)
                .unwrap_or(0)
        } else {
            num_rows
        };

        // Determine column slice bounds
        let col_start = if trim_cols == 1 || trim_cols == 3 {
            (0..num_cols)
                .find(|&c| !col_is_empty(&data, c))
                .unwrap_or(num_cols)
        } else {
            0
        };
        let col_end = if trim_cols == 2 || trim_cols == 3 {
            (0..num_cols)
                .rev()
                .find(|&c| !col_is_empty(&data, c))
                .map(|c| c + 1)
                .unwrap_or(0)
        } else {
            num_cols
        };

        if row_start >= row_end || col_start >= col_end {
            return CalcResult::new_error(
                Error::REF,
                cell,
                "TRIMRANGE result is empty".to_string(),
            );
        }

        let result: Vec<Vec<ArrayNode>> = data[row_start..row_end]
            .iter()
            .map(|row| row[col_start..col_end].to_vec())
            .collect();

        CalcResult::Array(result)
    }
}
