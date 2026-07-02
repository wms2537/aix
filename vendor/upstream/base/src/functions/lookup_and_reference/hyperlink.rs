use crate::{
    calc_result::CalcResult,
    expressions::{parser::Node, token::Error, types::CellReferenceIndex},
    model::Model,
};

impl<'a> Model<'a> {
    // HYPERLINK(link_location, [friendly_name])
    // IronCalc does not attach link objects to cells; the function returns the
    // value the cell displays: `friendly_name` if given (keeping its type, so a
    // numeric friendly name stays a number), `link_location` as text otherwise.
    pub(crate) fn fn_hyperlink(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.is_empty() || args.len() > 2 {
            return CalcResult::new_args_number_error(cell);
        }
        let link_location = match self.get_string(&args[0], cell) {
            Ok(s) => s,
            Err(error) => return error,
        };
        if args.len() == 1 {
            return CalcResult::String(link_location);
        }
        match self.evaluate_node_in_context(&args[1], cell) {
            // An empty friendly name displays as 0, like a plain reference
            CalcResult::EmptyCell | CalcResult::EmptyArg => CalcResult::Number(0.0),
            CalcResult::Range { .. } => {
                // Implicit Intersection not implemented
                CalcResult::Error {
                    error: Error::NIMPL,
                    origin: cell,
                    message: "Implicit Intersection not implemented".to_string(),
                }
            }
            CalcResult::Array(_) | CalcResult::Lambda(_) => CalcResult::Error {
                error: Error::NIMPL,
                origin: cell,
                message: "Arrays not supported yet".to_string(),
            },
            value => value,
        }
    }
}
