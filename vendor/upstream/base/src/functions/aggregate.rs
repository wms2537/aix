use std::collections::HashSet;

use crate::{
    calc_result::CalcResult,
    constants::{LAST_COLUMN, LAST_ROW},
    expressions::{parser::Node, token::Error, types::CellReferenceIndex},
    functions::{
        statistical::mode_functions::find_modes,
        statistical::percentile::{percentile_exc_impl, percentile_inc_impl},
        Function,
    },
    model::Model,
};

/// AGGREGATE(function_num, options, ref1, [ref2], ...)
/// The `options` bitmask controls what is ignored while walking the refs:
///   0: nested SUBTOTAL and AGGREGATE functions
///   1: hidden rows and nested SUBTOTAL and AGGREGATE functions
///   2: error values and nested SUBTOTAL and AGGREGATE functions
///   3: hidden rows, error values and nested SUBTOTAL and AGGREGATE functions
///   4: nothing
///   5: hidden rows
///   6: error values
///   7: hidden rows and error values
/// Unlike SUBTOTAL, filtered-out rows are not special-cased: they are hidden
/// rows, skipped only when the option asks for hidden rows to be ignored.
#[derive(Clone, Copy)]
struct AggregateOptions {
    ignore_hidden: bool,
    ignore_errors: bool,
    ignore_nested: bool,
}

fn is_subtotal_or_aggregate(node: &Node) -> bool {
    matches!(
        node,
        Node::FunctionKind {
            kind: Function::Subtotal | Function::Aggregate,
            args: _
        }
    )
}

impl<'a> Model<'a> {
    fn cell_is_subtotal_or_aggregate(&self, sheet_index: u32, row: i32, column: i32) -> bool {
        let row_data = match self.workbook.worksheets[sheet_index as usize]
            .sheet_data
            .get(&row)
        {
            Some(r) => r,
            None => return false,
        };
        let cell = match row_data.get(&column) {
            Some(c) => c,
            None => return false,
        };
        match cell.get_formula() {
            Some(f) => {
                is_subtotal_or_aggregate(&self.parsed_formulas[sheet_index as usize][f as usize].0)
            }
            None => false,
        }
    }

    /// Walks the ref arguments applying `options` and returns every non-empty
    /// scalar value found. Errors either stop the walk or are skipped,
    /// depending on `options.ignore_errors`.
    fn aggregate_get_values(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> Result<Vec<CalcResult>, CalcResult> {
        let mut result: Vec<CalcResult> = Vec::new();
        for arg in args {
            if options.ignore_nested && is_subtotal_or_aggregate(arg) {
                // Only nested SUBTOTAL/AGGREGATE results stored in the cells
                // of a reference are skipped; a direct SUBTOTAL/AGGREGATE
                // call passed as a ref argument is not a reference and Excel
                // rejects it with #VALUE!.
                return Err(CalcResult::new_error(
                    Error::VALUE,
                    cell,
                    "AGGREGATE: ref arguments must be references".to_string(),
                ));
            }
            match self.evaluate_node_with_reference(arg, cell) {
                value
                @ (CalcResult::String(_) | CalcResult::Boolean(_) | CalcResult::Number(_)) => {
                    result.push(value)
                }
                error @ CalcResult::Error { .. } => {
                    if !options.ignore_errors {
                        return Err(error);
                    }
                }
                CalcResult::Range { left, right } => {
                    if left.sheet != right.sheet {
                        return Err(CalcResult::new_error(
                            Error::VALUE,
                            cell,
                            "Ranges are in different sheets".to_string(),
                        ));
                    }
                    let row1 = left.row;
                    let mut row2 = right.row;
                    let column1 = left.column;
                    let mut column2 = right.column;
                    // Clamp open (whole-column/whole-row) ranges to the used
                    // area of the sheet, like SUM does; otherwise a `B:B`
                    // reference walks all 1,048,576 rows.
                    let worksheet = match self.workbook.worksheet(left.sheet) {
                        Ok(s) => s,
                        Err(_) => {
                            return Err(CalcResult::new_error(
                                Error::ERROR,
                                cell,
                                format!("Invalid worksheet index: '{}'", left.sheet),
                            ));
                        }
                    };
                    if row1 == 1 && row2 == LAST_ROW {
                        row2 = worksheet.dimension().max_row;
                    }
                    if column1 == 1 && column2 == LAST_COLUMN {
                        column2 = worksheet.dimension().max_column;
                    }
                    // Hidden rows are collected once per range:
                    // cell_hidden_status scans the row-style list linearly on
                    // every call, which is quadratic when done row by row.
                    // AGGREGATE skips hidden and filtered rows alike, so the
                    // row-style `hidden` flag alone decides.
                    let hidden_rows: Option<HashSet<i32>> = if options.ignore_hidden {
                        Some(
                            worksheet
                                .rows
                                .iter()
                                .filter(|r| r.hidden)
                                .map(|r| r.r)
                                .collect(),
                        )
                    } else {
                        None
                    };

                    for row in row1..=row2 {
                        if let Some(hidden_rows) = &hidden_rows {
                            if hidden_rows.contains(&row) {
                                continue;
                            }
                        }
                        for column in column1..=column2 {
                            if options.ignore_nested
                                && self.cell_is_subtotal_or_aggregate(left.sheet, row, column)
                            {
                                continue;
                            }
                            match self.evaluate_cell(CellReferenceIndex {
                                sheet: left.sheet,
                                row,
                                column,
                            }) {
                                CalcResult::EmptyCell | CalcResult::EmptyArg => {
                                    // skip
                                }
                                error @ CalcResult::Error { .. } => {
                                    if !options.ignore_errors {
                                        return Err(error);
                                    }
                                }
                                value => result.push(value),
                            }
                        }
                    }
                }
                CalcResult::EmptyCell | CalcResult::EmptyArg => {
                    // skip
                }
                CalcResult::Array(_) | CalcResult::Lambda(_) => {
                    return Err(CalcResult::Error {
                        error: Error::NIMPL,
                        origin: cell,
                        message: "Arrays not supported yet".to_string(),
                    })
                }
            }
        }
        Ok(result)
    }

    fn aggregate_get_numbers(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> Result<Vec<f64>, CalcResult> {
        let values = self.aggregate_get_values(args, cell, options)?;
        Ok(values
            .into_iter()
            .filter_map(|value| match value {
                CalcResult::Number(f) => Some(f),
                _ => None,
            })
            .collect())
    }

    pub(crate) fn fn_aggregate(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() < 3 {
            return CalcResult::new_args_number_error(cell);
        }
        let function_num = match self.get_number(&args[0], cell) {
            Ok(f) => f.trunc() as i32,
            Err(s) => return s,
        };
        let options_value = match self.get_number(&args[1], cell) {
            Ok(f) => f.trunc() as i32,
            Err(s) => return s,
        };
        if !(0..=7).contains(&options_value) {
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                format!("Invalid options for AGGREGATE: {options_value}"),
            );
        }
        let options = AggregateOptions {
            ignore_hidden: options_value % 2 == 1,
            ignore_errors: options_value & 2 != 0,
            ignore_nested: options_value < 4,
        };
        match function_num {
            1 => self.aggregate_average(&args[2..], cell, options),
            2 => self.aggregate_count(&args[2..], cell, options),
            3 => self.aggregate_counta(&args[2..], cell, options),
            4 => self.aggregate_max(&args[2..], cell, options),
            5 => self.aggregate_min(&args[2..], cell, options),
            6 => self.aggregate_product(&args[2..], cell, options),
            7 => self.aggregate_stdevs(&args[2..], cell, options),
            8 => self.aggregate_stdevp(&args[2..], cell, options),
            9 => self.aggregate_sum(&args[2..], cell, options),
            10 => self.aggregate_vars(&args[2..], cell, options),
            11 => self.aggregate_varp(&args[2..], cell, options),
            12 => self.aggregate_median(&args[2..], cell, options),
            13 => self.aggregate_mode_sngl(&args[2..], cell, options),
            14..=19 => {
                // These take exactly AGGREGATE(function_num, options, array, k)
                if args.len() != 4 {
                    return CalcResult::new_args_number_error(cell);
                }
                let mut sorted = match self.aggregate_get_numbers(&args[2..3], cell, options) {
                    Ok(s) => s,
                    Err(s) => return s,
                };
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let k = match self.get_number(&args[3], cell) {
                    Ok(f) => f,
                    Err(s) => return s,
                };
                if sorted.is_empty() {
                    return CalcResult::new_error(
                        Error::NUM,
                        cell,
                        "AGGREGATE: empty array".to_string(),
                    );
                }
                match function_num {
                    14 => aggregate_large(&sorted, k, cell),
                    15 => aggregate_small(&sorted, k, cell),
                    16 => aggregate_percentile_inc(&sorted, k, cell),
                    17 => {
                        // A quart in (-1, 0) truncates to -0.0, which would
                        // pass the 0..=1 bounds check below; Excel returns
                        // #NUM! for any negative quart.
                        if k < 0.0 {
                            return CalcResult::new_error(
                                Error::NUM,
                                cell,
                                "AGGREGATE: quart must be between 0 and 4".to_string(),
                            );
                        }
                        aggregate_percentile_inc(&sorted, k.trunc() / 4.0, cell)
                    }
                    18 => aggregate_percentile_exc(&sorted, k, cell),
                    // 19: the checked range differs from the inc quartile
                    _ => {
                        let quart = k.trunc();
                        if !(1.0..=3.0).contains(&quart) {
                            return CalcResult::new_error(
                                Error::NUM,
                                cell,
                                "AGGREGATE: quart must be 1, 2, or 3".to_string(),
                            );
                        }
                        aggregate_percentile_exc(&sorted, quart / 4.0, cell)
                    }
                }
            }
            _ => CalcResult::new_error(
                Error::VALUE,
                cell,
                format!("Invalid value for AGGREGATE: {function_num}"),
            ),
        }
    }

    fn aggregate_average(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let values = match self.aggregate_get_numbers(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        let l = values.len();
        if l == 0 {
            return CalcResult::Error {
                error: Error::DIV,
                origin: cell,
                message: "Division by 0!".to_string(),
            };
        }
        CalcResult::Number(values.iter().sum::<f64>() / (l as f64))
    }

    fn aggregate_count(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let values = match self.aggregate_get_values(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        let count = values
            .iter()
            .filter(|value| matches!(value, CalcResult::Number(_)))
            .count();
        CalcResult::Number(count as f64)
    }

    fn aggregate_counta(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let values = match self.aggregate_get_values(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        CalcResult::Number(values.len() as f64)
    }

    fn aggregate_max(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let values = match self.aggregate_get_numbers(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        let mut result = f64::NAN;
        for value in values {
            result = value.max(result);
        }
        if result.is_nan() {
            return CalcResult::Number(0.0);
        }
        CalcResult::Number(result)
    }

    fn aggregate_min(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let values = match self.aggregate_get_numbers(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        let mut result = f64::NAN;
        for value in values {
            result = value.min(result);
        }
        if result.is_nan() {
            return CalcResult::Number(0.0);
        }
        CalcResult::Number(result)
    }

    fn aggregate_product(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let values = match self.aggregate_get_numbers(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        if values.is_empty() {
            // Excel returns 0 for PRODUCT/AGGREGATE(6) over no values, not
            // the empty-product identity.
            return CalcResult::Number(0.0);
        }
        let mut result = 1.0;
        for value in values {
            result *= value;
        }
        CalcResult::Number(result)
    }

    fn aggregate_sum(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let values = match self.aggregate_get_numbers(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        CalcResult::Number(values.iter().sum())
    }

    fn aggregate_stdevs(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        match self.aggregate_variance(args, cell, options, true) {
            Ok(variance) => CalcResult::Number(variance.sqrt()),
            Err(s) => s,
        }
    }

    fn aggregate_stdevp(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        match self.aggregate_variance(args, cell, options, false) {
            Ok(variance) => CalcResult::Number(variance.sqrt()),
            Err(s) => s,
        }
    }

    fn aggregate_vars(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        match self.aggregate_variance(args, cell, options, true) {
            Ok(variance) => CalcResult::Number(variance),
            Err(s) => s,
        }
    }

    fn aggregate_varp(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        match self.aggregate_variance(args, cell, options, false) {
            Ok(variance) => CalcResult::Number(variance),
            Err(s) => s,
        }
    }

    /// Variance of the collected values; `sample` selects the n-1 denominator.
    fn aggregate_variance(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
        sample: bool,
    ) -> Result<f64, CalcResult> {
        let values = self.aggregate_get_numbers(args, cell, options)?;
        let l = values.len();
        let denominator = if sample { l as f64 - 1.0 } else { l as f64 };
        if denominator < 1.0 {
            return Err(CalcResult::Error {
                error: Error::DIV,
                origin: cell,
                message: "Division by 0!".to_string(),
            });
        }
        let average = values.iter().sum::<f64>() / (l as f64);
        let mut result = 0.0;
        for value in &values {
            result += (value - average).powi(2) / denominator;
        }
        Ok(result)
    }

    fn aggregate_median(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let mut values = match self.aggregate_get_numbers(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        if values.is_empty() {
            return CalcResult::new_error(
                Error::NUM,
                cell,
                "AGGREGATE: no numeric values for MEDIAN".to_string(),
            );
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let l = values.len();
        if l % 2 == 0 {
            CalcResult::Number((values[l / 2 - 1] + values[l / 2]) / 2.0)
        } else {
            CalcResult::Number(values[l / 2])
        }
    }

    fn aggregate_mode_sngl(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
        options: AggregateOptions,
    ) -> CalcResult {
        let values = match self.aggregate_get_numbers(args, cell, options) {
            Ok(s) => s,
            Err(s) => return s,
        };
        if values.is_empty() {
            return CalcResult::new_error(
                Error::NA,
                cell,
                "AGGREGATE: no numeric values for MODE.SNGL".to_string(),
            );
        }
        let (modes, max_count) = find_modes(&values);
        if max_count < 2 {
            return CalcResult::new_error(
                Error::NA,
                cell,
                "AGGREGATE: no value appears more than once".to_string(),
            );
        }
        CalcResult::Number(modes[0])
    }
}

fn aggregate_large(sorted: &[f64], k: f64, cell: CellReferenceIndex) -> CalcResult {
    let k = k.trunc();
    if k < 1.0 || k > sorted.len() as f64 {
        return CalcResult::new_error(
            Error::NUM,
            cell,
            "AGGREGATE: k out of valid range".to_string(),
        );
    }
    CalcResult::Number(sorted[sorted.len() - k as usize])
}

fn aggregate_small(sorted: &[f64], k: f64, cell: CellReferenceIndex) -> CalcResult {
    let k = k.trunc();
    if k < 1.0 || k > sorted.len() as f64 {
        return CalcResult::new_error(
            Error::NUM,
            cell,
            "AGGREGATE: k out of valid range".to_string(),
        );
    }
    CalcResult::Number(sorted[k as usize - 1])
}

fn aggregate_percentile_inc(sorted: &[f64], k: f64, cell: CellReferenceIndex) -> CalcResult {
    if !(0.0..=1.0).contains(&k) {
        return CalcResult::new_error(
            Error::NUM,
            cell,
            "AGGREGATE: k must be between 0 and 1".to_string(),
        );
    }
    CalcResult::Number(percentile_inc_impl(sorted, k))
}

fn aggregate_percentile_exc(sorted: &[f64], k: f64, cell: CellReferenceIndex) -> CalcResult {
    match percentile_exc_impl(sorted, k) {
        Some(result) => CalcResult::Number(result),
        None => CalcResult::new_error(
            Error::NUM,
            cell,
            "AGGREGATE: k out of valid range".to_string(),
        ),
    }
}
