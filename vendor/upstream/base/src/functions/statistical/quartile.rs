use crate::expressions::types::CellReferenceIndex;
use crate::{
    calc_result::CalcResult, expressions::parser::Node, expressions::token::Error, model::Model,
};

use super::percentile::{percentile_exc_impl, percentile_inc_impl};

impl<'a> Model<'a> {
    // QUARTILE.INC(array, quart) — quart: 0..4 → 0%, 25%, 50%, 75%, 100%
    pub(crate) fn fn_quartile_inc(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        if args.len() != 2 {
            return CalcResult::new_args_number_error(cell);
        }

        let sorted = match self.collect_sorted_values(&args[0], cell) {
            Ok(v) => v,
            Err(e) => return e,
        };

        if sorted.is_empty() {
            return CalcResult::new_error(
                Error::NUM,
                cell,
                "QUARTILE.INC: empty array".to_string(),
            );
        }

        let quart = match self.get_number(&args[1], cell) {
            Ok(v) => v.floor() as i64,
            Err(e) => return e,
        };

        if !(0..=4).contains(&quart) {
            return CalcResult::new_error(
                Error::NUM,
                cell,
                "QUARTILE.INC: quart must be 0, 1, 2, 3, or 4".to_string(),
            );
        }

        let k = quart as f64 / 4.0;
        CalcResult::Number(percentile_inc_impl(&sorted, k))
    }

    // QUARTILE.EXC(array, quart) — quart: 1, 2, or 3 → 25%, 50%, 75%
    pub(crate) fn fn_quartile_exc(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        if args.len() != 2 {
            return CalcResult::new_args_number_error(cell);
        }

        let sorted = match self.collect_sorted_values(&args[0], cell) {
            Ok(v) => v,
            Err(e) => return e,
        };

        if sorted.is_empty() {
            return CalcResult::new_error(
                Error::NUM,
                cell,
                "QUARTILE.EXC: empty array".to_string(),
            );
        }

        let quart = match self.get_number(&args[1], cell) {
            Ok(v) => v.floor() as i64,
            Err(e) => return e,
        };

        if !(1..=3).contains(&quart) {
            return CalcResult::new_error(
                Error::NUM,
                cell,
                "QUARTILE.EXC: quart must be 1, 2, or 3".to_string(),
            );
        }

        // Reuse PERCENTILE.EXC logic directly
        let k = quart as f64 / 4.0;
        match percentile_exc_impl(&sorted, k) {
            Some(result) => CalcResult::Number(result),
            None => CalcResult::new_error(
                Error::NUM,
                cell,
                "QUARTILE.EXC: not enough data points for this quartile".to_string(),
            ),
        }
    }
}
