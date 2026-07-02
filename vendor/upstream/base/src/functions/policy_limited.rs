//! Functions that are recognized and fully argument-checked but whose result
//! depends on an external service, an OLAP connection, a PivotTable or native
//! code, none of which the engine executes. After validating the arguments
//! each function returns exactly the error desktop Excel produces when that
//! external work cannot happen (see docs/specs/full-catalog-semantics.md,
//! "Tier II").

use crate::{
    calc_result::CalcResult,
    expressions::{parser::Node, token::Error, types::CellReferenceIndex},
    model::Model,
};

/// Maximum length Excel accepts for a CUBE member/set expression.
const CUBE_EXPRESSION_MAX_LENGTH: usize = 255;

/// Maximum length Excel accepts for a WEBSERVICE url.
const WEBSERVICE_URL_MAX_LENGTH: usize = 2048;

impl<'a> Model<'a> {
    /// The Excel-documented result for every CUBE function evaluated without
    /// OLAP connectivity: "if the connection name is not a valid workbook
    /// connection... #NAME?". With no OLAP support every connection string is
    /// invalid, so #NAME? is the Excel-exact result. Spec "Tier II", CUBE row
    /// [P].
    fn cube_connection_error(&self, cell: CellReferenceIndex) -> CalcResult {
        CalcResult::new_error(
            Error::NAME,
            cell,
            "The connection name is not a valid workbook connection".to_string(),
        )
    }

    /// Coerces a CUBE member/set expression argument and applies the
    /// documented >255-character check ("If the member_expression is longer
    /// than 255 characters... #VALUE!") [P].
    fn get_cube_expression(
        &mut self,
        node: &Node,
        cell: CellReferenceIndex,
    ) -> Result<String, CalcResult> {
        let expression = self.get_string(node, cell)?;
        if expression.chars().count() > CUBE_EXPRESSION_MAX_LENGTH {
            return Err(CalcResult::new_error(
                Error::VALUE,
                cell,
                "Expression is longer than 255 characters".to_string(),
            ));
        }
        Ok(expression)
    }

    // WEBSERVICE(url)
    // Excel returns #VALUE! for every failure to fetch, including being
    // offline, and also for a url longer than 2048 characters or one that
    // does not use the http(s) protocol; the length/protocol checks come
    // first. Spec "Tier II", WEBSERVICE row [P].
    pub(crate) fn fn_webservice(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() != 1 {
            return CalcResult::new_args_number_error(cell);
        }
        let url = match self.get_string(&args[0], cell) {
            Ok(s) => s,
            Err(error) => return error,
        };
        if url.chars().count() > WEBSERVICE_URL_MAX_LENGTH {
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "URL is longer than 2048 characters".to_string(),
            );
        }
        let lower = url.to_ascii_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "URL does not use the http or https protocol".to_string(),
            );
        }
        CalcResult::new_error(
            Error::VALUE,
            cell,
            "External requests are not performed".to_string(),
        )
    }

    // RTD(prog_id, server, topic1, [topic2], ...)
    // "If you haven't installed a real-time data server... #N/A". No RTD
    // server is ever available here. Spec "Tier II", RTD row [P].
    pub(crate) fn fn_rtd(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() < 3 {
            return CalcResult::new_args_number_error(cell);
        }
        for arg in args {
            if let Err(error) = self.get_string(arg, cell) {
                return error;
            }
        }
        CalcResult::new_error(
            Error::NA,
            cell,
            "No real-time data server is available".to_string(),
        )
    }

    // STOCKHISTORY(stock, start_date, [end_date], [interval], [headers],
    //              [property0], ..., [property5])
    // Argument validation first (#VALUE! for out-of-range enums [S/U]), then
    // the offline/service literal #CONNECT!. Spec "Tier II", STOCKHISTORY row.
    pub(crate) fn fn_stockhistory(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        if args.len() < 2 || args.len() > 11 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        if let Err(error) = self.get_number(&args[1], cell) {
            return error;
        }
        if args.len() > 2 {
            if let Err(error) = self.get_number(&args[2], cell) {
                return error;
            }
        }
        // interval: 0 daily, 1 weekly, 2 monthly
        if args.len() > 3 {
            match self.get_number(&args[3], cell) {
                Ok(interval) => {
                    if !matches!(interval as i32, 0..=2) || interval.fract() != 0.0 {
                        return CalcResult::new_error(
                            Error::VALUE,
                            cell,
                            "Interval must be 0, 1 or 2".to_string(),
                        );
                    }
                }
                Err(error) => return error,
            }
        }
        // headers: 0 none, 1 headers, 2 instrument identifier and headers
        if args.len() > 4 {
            match self.get_number(&args[4], cell) {
                Ok(headers) => {
                    if !matches!(headers as i32, 0..=2) || headers.fract() != 0.0 {
                        return CalcResult::new_error(
                            Error::VALUE,
                            cell,
                            "Headers must be 0, 1 or 2".to_string(),
                        );
                    }
                }
                Err(error) => return error,
            }
        }
        // properties: 0 date, 1 close, 2 open, 3 high, 4 low, 5 volume
        for arg in args.iter().skip(5) {
            match self.get_number(arg, cell) {
                Ok(property) => {
                    if !matches!(property as i32, 0..=5) || property.fract() != 0.0 {
                        return CalcResult::new_error(
                            Error::VALUE,
                            cell,
                            "Properties must be between 0 and 5".to_string(),
                        );
                    }
                }
                Err(error) => return error,
            }
        }
        CalcResult::new_error(
            Error::CONNECT,
            cell,
            "The stock data service cannot be reached".to_string(),
        )
    }

    // DETECTLANGUAGE(text)
    // Standard text coercion first, then the offline literal #CONNECT!.
    // Spec "Tier II", DETECTLANGUAGE row [P generic + S].
    pub(crate) fn fn_detectlanguage(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        if args.len() != 1 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        CalcResult::new_error(
            Error::CONNECT,
            cell,
            "The language detection service cannot be reached".to_string(),
        )
    }

    // TRANSLATE(text, [source_language], [target_language])
    // Invalid language code -> #VALUE! [S]; otherwise the offline literal
    // #CONNECT!. The accepted code shape (BCP 47-like tags such as "en",
    // "fr-CA") is implementation-defined [U]. Spec "Tier II", TRANSLATE row.
    pub(crate) fn fn_translate(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.is_empty() || args.len() > 3 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        for arg in args.iter().skip(1) {
            match self.get_string(arg, cell) {
                Ok(code) => {
                    // An omitted/empty code means "auto-detect".
                    if !code.is_empty() && !is_language_code(&code) {
                        return CalcResult::new_error(
                            Error::VALUE,
                            cell,
                            format!("Invalid language code: '{code}'"),
                        );
                    }
                }
                Err(error) => return error,
            }
        }
        CalcResult::new_error(
            Error::CONNECT,
            cell,
            "The translation service cannot be reached".to_string(),
        )
    }

    // COPILOT(prompt_part1, [context1], [prompt_part2], [context2], ...)
    // Bad prompt arguments -> #VALUE! [P]; otherwise the timeout/no-service
    // literal #CONNECT!, matching Excel's own error table [P]. Spec
    // "Tier II", COPILOT row.
    pub(crate) fn fn_copilot(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.is_empty() {
            return CalcResult::new_args_number_error(cell);
        }
        // Odd arguments are prompt parts (text), even arguments are contexts
        // (any value or range); errors propagate in argument order.
        for (index, arg) in args.iter().enumerate() {
            if index % 2 == 0 {
                if let Err(error) = self.get_string(arg, cell) {
                    return error;
                }
            } else if let error @ CalcResult::Error { .. } =
                self.evaluate_node_in_context(arg, cell)
            {
                return error;
            }
        }
        CalcResult::new_error(
            Error::CONNECT,
            cell,
            "The Copilot service cannot be reached".to_string(),
        )
    }

    // IMAGE(source, [alt_text], [sizing], [height], [width])
    // Local #VALUE! validation per the documented sizing rules first [P]:
    // sizing must be 0-3 and custom size (3) requires both height and width;
    // height/width with sizing 0-2 is rejected [U]. Then #CONNECT! because
    // the image cannot be retrieved [P]. Spec "Tier II", IMAGE row.
    pub(crate) fn fn_image(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.is_empty() || args.len() > 5 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        if args.len() > 1 {
            if let Err(error) = self.get_string(&args[1], cell) {
                return error;
            }
        }
        let sizing = if args.len() > 2 {
            match self.evaluate_node_in_context(&args[2], cell) {
                CalcResult::EmptyArg => 0,
                result => match self.cast_to_number(result, cell) {
                    Ok(sizing) => {
                        if !matches!(sizing as i32, 0..=3) || sizing.fract() != 0.0 {
                            return CalcResult::new_error(
                                Error::VALUE,
                                cell,
                                "Sizing must be between 0 and 3".to_string(),
                            );
                        }
                        sizing as i32
                    }
                    Err(error) => return error,
                },
            }
        } else {
            0
        };
        let mut dimensions = [None, None];
        for (index, slot) in dimensions.iter_mut().enumerate() {
            if args.len() > 3 + index {
                match self.evaluate_node_in_context(&args[3 + index], cell) {
                    CalcResult::EmptyArg => {}
                    result => match self.cast_to_number(result, cell) {
                        Ok(value) => {
                            if value <= 0.0 {
                                return CalcResult::new_error(
                                    Error::VALUE,
                                    cell,
                                    "Height and width must be positive".to_string(),
                                );
                            }
                            *slot = Some(value);
                        }
                        Err(error) => return error,
                    },
                }
            }
        }
        if sizing == 3 {
            if dimensions[0].is_none() || dimensions[1].is_none() {
                return CalcResult::new_error(
                    Error::VALUE,
                    cell,
                    "Custom size requires both height and width".to_string(),
                );
            }
        } else if dimensions[0].is_some() || dimensions[1].is_some() {
            return CalcResult::new_error(
                Error::VALUE,
                cell,
                "Height and width are only used when sizing is 3".to_string(),
            );
        }
        CalcResult::new_error(
            Error::CONNECT,
            cell,
            "The image cannot be retrieved".to_string(),
        )
    }

    // CALL(register_id, [argument1], ...)
    // CALL(module_text, procedure, type_text, [argument1], ...)
    // Worksheet CALL has been disabled since MS98-018; #BLOCKED! is the only
    // documented literal for blocked XLM evaluation [P]. The arguments are
    // never evaluated. Spec "Tier II", CALL row.
    pub(crate) fn fn_call(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.is_empty() {
            return CalcResult::new_args_number_error(cell);
        }
        CalcResult::new_error(
            Error::BLOCKED,
            cell,
            "Calling external procedures is disabled".to_string(),
        )
    }

    // REGISTER.ID(module_text, procedure, [type_text])
    // Same XLM policy basis as CALL -> #BLOCKED! [P generic; the exact
    // modern literal is undocumented, U]. The arguments are never evaluated.
    // Spec "Tier II", REGISTER.ID row.
    pub(crate) fn fn_register_id(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() < 2 || args.len() > 3 {
            return CalcResult::new_args_number_error(cell);
        }
        CalcResult::new_error(
            Error::BLOCKED,
            cell,
            "Registering external procedures is disabled".to_string(),
        )
    }

    // CUBEVALUE(connection, [member_expression1], ...)
    // Spec "Tier II", CUBE row [P].
    pub(crate) fn fn_cubevalue(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.is_empty() {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        for arg in args.iter().skip(1) {
            if let Err(error) = self.get_cube_expression(arg, cell) {
                return error;
            }
        }
        self.cube_connection_error(cell)
    }

    // CUBEMEMBER(connection, member_expression, [caption])
    // Spec "Tier II", CUBE row [P]; >255-character member_expression ->
    // #VALUE! [P].
    pub(crate) fn fn_cubemember(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() < 2 || args.len() > 3 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        if let Err(error) = self.get_cube_expression(&args[1], cell) {
            return error;
        }
        if args.len() == 3 {
            if let Err(error) = self.get_string(&args[2], cell) {
                return error;
            }
        }
        self.cube_connection_error(cell)
    }

    // CUBESET(connection, set_expression, [caption], [sort_order], [sort_by])
    // Spec "Tier II", CUBE row [P]; >255-character set_expression ->
    // #VALUE! [P].
    pub(crate) fn fn_cubeset(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() < 2 || args.len() > 5 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        if let Err(error) = self.get_cube_expression(&args[1], cell) {
            return error;
        }
        if args.len() > 2 {
            if let Err(error) = self.get_string(&args[2], cell) {
                return error;
            }
        }
        // sort_order: 0 natural, 1-2 ascending/descending, 3-4 alpha, 5-6 natural
        if args.len() > 3 {
            match self.get_number(&args[3], cell) {
                Ok(sort_order) => {
                    if !matches!(sort_order as i32, 0..=6) || sort_order.fract() != 0.0 {
                        return CalcResult::new_error(
                            Error::VALUE,
                            cell,
                            "Sort order must be between 0 and 6".to_string(),
                        );
                    }
                }
                Err(error) => return error,
            }
        }
        if args.len() > 4 {
            if let Err(error) = self.get_string(&args[4], cell) {
                return error;
            }
        }
        self.cube_connection_error(cell)
    }

    // CUBESETCOUNT(set)
    // The set argument is itself a CUBESET result, which without OLAP
    // connectivity is an error to propagate; a non-error value is not a set
    // -> #VALUE! [U]. Spec "Tier II", CUBE row.
    pub(crate) fn fn_cubesetcount(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        if args.len() != 1 {
            return CalcResult::new_args_number_error(cell);
        }
        if let error @ CalcResult::Error { .. } = self.evaluate_node_in_context(&args[0], cell) {
            return error;
        }
        CalcResult::new_error(Error::VALUE, cell, "Argument must be a set".to_string())
    }

    // CUBERANKEDMEMBER(connection, set_expression, rank, [caption])
    // Spec "Tier II", CUBE row [P].
    pub(crate) fn fn_cuberankedmember(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        if args.len() < 3 || args.len() > 4 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        if let Err(error) = self.get_cube_expression(&args[1], cell) {
            return error;
        }
        if let Err(error) = self.get_number(&args[2], cell) {
            return error;
        }
        if args.len() == 4 {
            if let Err(error) = self.get_string(&args[3], cell) {
                return error;
            }
        }
        self.cube_connection_error(cell)
    }

    // CUBEKPIMEMBER(connection, kpi_name, kpi_property, [caption])
    // Spec "Tier II", CUBE row [P].
    pub(crate) fn fn_cubekpimember(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        if args.len() < 3 || args.len() > 4 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        if let Err(error) = self.get_string(&args[1], cell) {
            return error;
        }
        // kpi_property: 1 value, 2 goal, 3 status, 4 trend, 5 weight, 6 current time member
        match self.get_number(&args[2], cell) {
            Ok(kpi_property) => {
                if !matches!(kpi_property as i32, 1..=6) || kpi_property.fract() != 0.0 {
                    return CalcResult::new_error(
                        Error::VALUE,
                        cell,
                        "KPI property must be between 1 and 6".to_string(),
                    );
                }
            }
            Err(error) => return error,
        }
        if args.len() == 4 {
            if let Err(error) = self.get_string(&args[3], cell) {
                return error;
            }
        }
        self.cube_connection_error(cell)
    }

    // CUBEMEMBERPROPERTY(connection, member_expression, property)
    // Spec "Tier II", CUBE row [P]; >255-character member_expression ->
    // #VALUE! [P].
    pub(crate) fn fn_cubememberproperty(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        if args.len() != 3 {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        if let Err(error) = self.get_cube_expression(&args[1], cell) {
            return error;
        }
        if let Err(error) = self.get_string(&args[2], cell) {
            return error;
        }
        self.cube_connection_error(cell)
    }

    // GETPIVOTDATA(data_field, pivot_table, [field1, item1], ...)
    // #REF! is the sole documented literal: not-a-PivotTable-range, invisible
    // field/item and filtered-out data all produce it [P]. The engine has no
    // PivotTable model, so the reference never contains one. Spec "Tier II",
    // GETPIVOTDATA row.
    pub(crate) fn fn_getpivotdata(
        &mut self,
        args: &[Node],
        cell: CellReferenceIndex,
    ) -> CalcResult {
        // field/item arguments come in pairs
        if args.len() < 2 || !(args.len() - 2).is_multiple_of(2) {
            return CalcResult::new_args_number_error(cell);
        }
        if let Err(error) = self.get_string(&args[0], cell) {
            return error;
        }
        if let error @ CalcResult::Error { .. } = self.evaluate_node_in_context(&args[1], cell) {
            return error;
        }
        for arg in args.iter().skip(2) {
            if let error @ CalcResult::Error { .. } = self.evaluate_node_in_context(arg, cell) {
                return error;
            }
        }
        CalcResult::new_error(
            Error::REF,
            cell,
            "The reference does not contain a PivotTable".to_string(),
        )
    }
}

/// Returns true if `code` looks like a BCP 47 language tag: alphanumeric
/// subtags of 1 to 8 characters separated by hyphens, starting with a 2 or 3
/// letter primary subtag. The exact set Excel accepts is undocumented [U].
fn is_language_code(code: &str) -> bool {
    let mut subtags = code.split('-');
    let Some(primary) = subtags.next() else {
        return false;
    };
    if !matches!(primary.len(), 2..=3) || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    subtags.all(|subtag| {
        matches!(subtag.len(), 1..=8) && subtag.chars().all(|c| c.is_ascii_alphanumeric())
    })
}
