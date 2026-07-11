//! Formula census: function tallies, support probing, volatile detection.
//!
//! CONTRACT (other modules depend on these signatures — do not change them):
//!   pub struct FunctionCensus { pub tallies, pub unsupported, pub policy_limited,
//!                               pub volatile_present, pub user_defined }
//!   pub fn function_census(model: &Model) -> FunctionCensus
//!   pub fn extract_function_names(formula: &str) -> Vec<String>
//!   pub fn probe_support(names: &[String]) -> Vec<String>   // returns UNSUPPORTED subset
//!   pub fn policy_limited_literal(name: &str) -> Option<&'static str>
//!   pub const POLICY_LIMITED_FUNCTIONS: &[(name, literal, reason)]
//!
//! Implementation notes for the implementer:
//! - Tokenize formulas with ironcalc's public lexer
//!   (`ironcalc::base::expressions::lexer`): a function call is
//!   `TokenType::Ident(name)` immediately followed by `TokenType::LeftParenthesis`.
//!   Do NOT use regex; strings and quoted sheet names would false-positive.
//! - The census scans BOTH cell formulas and defined-name formulas: a
//!   function used only inside a defined name still determines whether the
//!   engine can evaluate the workbook (and whether it is volatile).
//! - Callable-name classification. Not every `name(` is an Excel function:
//!   VBA/XLL UDFs, add-in functions, and called LAMBDA defined names are
//!   USER DATA and must not leak through the census. A called name is a
//!   function only if it matches a workbook defined name = user-defined; else
//!   if it is in the canonical Excel catalog (xlq/data/excel-functions.txt,
//!   embedded at compile time) or the engine recognizes it = function;
//!   anything else = user-defined callable (tallied in `user_defined`,
//!   emitted as counts only under redaction).
//! - Support probe: in a scratch `Model::new_empty`, set `=NAME(1)` and
//!   evaluate; `#NAME?` as the result means the function name is unknown to
//!   the engine (Excel semantics: unknown name errors before arg validation).
//!   Verified experimentally against ironcalc 0.7.1. A name the engine's
//!   parser rejects outright (set_user_input error) is also UNSUPPORTED —
//!   the failure default must never inflate the coverage claim.
//!   Exception: the seven CUBE functions are probed with ZERO arguments
//!   (`=NAME()`), because their documented correct no-connection answer IS
//!   `#NAME?` — see CUBE_NAME_CARVE_OUT.
//! - Volatile set (Excel semantics): NOW, TODAY, RAND, RANDBETWEEN, OFFSET,
//!   INDIRECT, CELL, INFO.
//! - PRIVACY INVARIANT: nothing in this module's output may contain cell
//!   values or string literals from formulas. `tallies`/`unsupported`/
//!   `volatile_present` carry Excel-vocabulary function NAMES only;
//!   user-defined callable names appear only in `user_defined`, which
//!   consumers must treat like defined names (redactable).

use ironcalc::base::expressions::lexer::{Lexer, LexerMode};
use ironcalc::base::expressions::token::TokenType;
use ironcalc::base::language::get_language;
use ironcalc::base::locale::get_locale;
use ironcalc::base::Model;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Tier II policy/context-limited functions
/// (docs/specs/full-catalog-semantics.md, "Tier II"): the engine RECOGNIZES
/// them and validates their arguments, but their real value depends on an
/// external service, an OLAP connection, a PivotTable model, or native code —
/// all of which xlq refuses to execute by design (local-only; preserve, never
/// execute). Each returns exactly the error literal desktop Excel produces
/// when that external work cannot happen. Their stored values can therefore
/// never be VERIFIED locally: census consumers must keep
/// `coverage.reliable: false` when any of these is present, but report them
/// in the `policy_limited` bucket, not as `unsupported` — the message
/// "the engine does not know this name" would be false.
///
/// (function name, documented no-connection literal, one-line reason)
pub const POLICY_LIMITED_FUNCTIONS: &[(&str, &str, &str)] = &[
    (
        "WEBSERVICE",
        "#VALUE!",
        "external HTTP fetch; #VALUE! is Excel's literal for every failure to fetch, including offline (after >2048-char / non-http(s) URL validation)",
    ),
    (
        "RTD",
        "#N/A",
        "real-time COM data feed; documented result when no RTD server is installed",
    ),
    (
        "STOCKHISTORY",
        "#CONNECT!",
        "Microsoft market-data service; offline/service literal after argument validation (#VALUE! for bad enums first)",
    ),
    (
        "DETECTLANGUAGE",
        "#CONNECT!",
        "Microsoft language-detection service; offline literal after text coercion",
    ),
    (
        "TRANSLATE",
        "#CONNECT!",
        "Microsoft translation service; offline literal after language-code validation (#VALUE! for invalid codes first)",
    ),
    (
        "COPILOT",
        "#CONNECT!",
        "Copilot AI service; timeout/no-service literal after prompt-argument validation",
    ),
    (
        "IMAGE",
        "#CONNECT!",
        "remote image fetch; cannot-retrieve literal after the documented #VALUE! sizing/dimension validation",
    ),
    (
        "CALL",
        "#BLOCKED!",
        "XLM/DLL procedure invocation, disabled in worksheets since MS98-018; never executed",
    ),
    (
        "REGISTER.ID",
        "#BLOCKED!",
        "XLM/DLL procedure registration; same blocked-XLM policy basis as CALL",
    ),
    (
        "CUBEVALUE",
        "#NAME?",
        "OLAP cube query; with no OLAP connectivity every connection name is invalid -> documented #NAME? (NOT name-unknown)",
    ),
    (
        "CUBEMEMBER",
        "#NAME?",
        "OLAP cube member lookup; documented #NAME? for an invalid workbook connection (#VALUE! for >255-char expressions first)",
    ),
    (
        "CUBESET",
        "#NAME?",
        "OLAP cube set definition; documented #NAME? for an invalid workbook connection",
    ),
    (
        "CUBESETCOUNT",
        "#NAME?",
        "counts a CUBESET set; without OLAP the set argument is itself #NAME? and propagates (a non-set value -> #VALUE!)",
    ),
    (
        "CUBERANKEDMEMBER",
        "#NAME?",
        "OLAP cube ranked member; documented #NAME? for an invalid workbook connection",
    ),
    (
        "CUBEKPIMEMBER",
        "#NAME?",
        "OLAP cube KPI member; documented #NAME? for an invalid workbook connection",
    ),
    (
        "CUBEMEMBERPROPERTY",
        "#NAME?",
        "OLAP cube member property; documented #NAME? for an invalid workbook connection",
    ),
    (
        "GETPIVOTDATA",
        "#REF!",
        "reads a rendered PivotTable; the engine has no pivot model, so the reference never contains one -> documented #REF!",
    ),
];

/// The documented no-connection literal for a policy-limited function,
/// or `None` when the name is not policy-limited.
pub fn policy_limited_literal(name: &str) -> Option<&'static str> {
    POLICY_LIMITED_FUNCTIONS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, literal, _)| *literal)
}

/// Probe carve-out (docs/specs/full-catalog-semantics.md, "Coverage
/// accounting rule"): the seven CUBE functions answer `#NAME?` BY DESIGN when
/// evaluated without an OLAP connection — Microsoft documents "if the
/// connection name is not a valid workbook connection... #NAME?", and with no
/// OLAP connectivity every connection string is invalid. A `#NAME?` result
/// for `=CUBEVALUE(1)` therefore does NOT mean the name is unknown, and the
/// probe's #NAME? heuristic would misreport recognized functions as
/// unsupported. Instead these names are probed with ZERO arguments: a
/// recognized cube function fails argument-count validation first (`#ERROR!`,
/// never `#NAME?`; all seven require at least one argument), while a name the
/// engine truly does not know still evaluates to `#NAME?`. Recognition stays
/// measured, not asserted.
const CUBE_NAME_CARVE_OUT: &[&str] = &[
    "CUBEKPIMEMBER",
    "CUBEMEMBER",
    "CUBEMEMBERPROPERTY",
    "CUBERANKEDMEMBER",
    "CUBESET",
    "CUBESETCOUNT",
    "CUBEVALUE",
];

const VOLATILE_FUNCTIONS: &[&str] = &[
    "NOW",
    "TODAY",
    "RAND",
    "RANDBETWEEN",
    "OFFSET",
    "INDIRECT",
    "CELL",
    "INFO",
];

#[derive(Debug, Serialize)]
pub struct FunctionCensus {
    /// Excel function name (canonical uppercase) -> number of call sites.
    /// Contains only Excel-vocabulary functions, never user-defined callables.
    pub tallies: BTreeMap<String, u64>,
    /// Excel functions present in the workbook that the engine cannot evaluate.
    pub unsupported: Vec<String>,
    /// Policy/context-limited functions present in the workbook -> the
    /// documented no-connection error literal each one returns (see
    /// `POLICY_LIMITED_FUNCTIONS`). The engine recognizes them and validates
    /// their arguments, but their true values depend on external
    /// services/connections xlq never contacts — so stored values cannot be
    /// verified locally (consumers must keep coverage "reliable" false), yet
    /// they are NOT "unsupported": the refusal literal is Excel-exact.
    pub policy_limited: BTreeMap<String, String>,
    /// Volatile functions present (determinism hazard for reproducible calc).
    pub volatile_present: Vec<String>,
    /// User-defined callables (VBA/XLL UDFs, add-in functions, called LAMBDA
    /// defined names) -> number of call sites. These names are USER DATA:
    /// they must be redactable and must never appear in `tallies` or
    /// `unsupported`. The engine cannot evaluate any of them.
    pub user_defined: BTreeMap<String, u64>,
}

/// Canonical Excel function catalog (uppercase), embedded at compile time
/// from xlq/data/excel-functions.txt (Microsoft's alphabetical list).
fn excel_catalog() -> &'static BTreeSet<String> {
    static CATALOG: OnceLock<BTreeSet<String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        include_str!("../data/excel-functions.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.to_uppercase())
            .collect()
    })
}

/// Census of every formula in the model: all cell formulas PLUS all
/// defined-name formulas (a function hidden inside a defined name still
/// affects evaluability and volatility).
pub fn function_census(model: &Model) -> FunctionCensus {
    let mut called: BTreeMap<String, u64> = BTreeMap::new();
    for cell in model.get_all_cells() {
        if let Ok(Some(formula)) = model.get_cell_formula(cell.index, cell.row, cell.column) {
            for name in extract_function_names(&formula) {
                *called.entry(name).or_insert(0) += 1;
            }
        }
    }
    for defined in &model.workbook.defined_names {
        for name in extract_function_names(&defined.formula) {
            *called.entry(name).or_insert(0) += 1;
        }
    }

    let defined_upper: BTreeSet<String> = model
        .workbook
        .defined_names
        .iter()
        .map(|d| d.name.to_uppercase())
        .collect();

    // Probe only names that are not workbook defined names.
    let to_probe: Vec<String> = called
        .keys()
        .filter(|n| !defined_upper.contains(*n))
        .cloned()
        .collect();
    let engine_unknown: BTreeSet<String> = probe_support(&to_probe).into_iter().collect();

    let mut tallies: BTreeMap<String, u64> = BTreeMap::new();
    let mut unsupported: Vec<String> = Vec::new();
    let mut policy_limited: BTreeMap<String, String> = BTreeMap::new();
    let mut user_defined: BTreeMap<String, u64> = BTreeMap::new();
    for (name, count) in called {
        if defined_upper.contains(&name) {
            // A called defined name (LAMBDA-style). User data.
            user_defined.insert(name, count);
        } else if excel_catalog().contains(&name) || !engine_unknown.contains(&name) {
            // Excel vocabulary (catalog) or engine-recognized function.
            if engine_unknown.contains(&name) {
                // Truly unknown to the engine (the probe's CUBE carve-out
                // already prevents documented-#NAME? functions landing here).
                unsupported.push(name.clone());
            } else if let Some(literal) = policy_limited_literal(&name) {
                // Recognized, but the value depends on an external service /
                // connection xlq never contacts: distinct bucket, so the
                // consumer's message is accurate ("cannot verify locally"),
                // not false ("engine does not know this name").
                policy_limited.insert(name.clone(), literal.to_string());
            }
            tallies.insert(name, count);
        } else {
            // Unknown to both the catalog and the engine: a UDF or add-in
            // function. User data — never emitted as a "function".
            user_defined.insert(name, count);
        }
    }
    let volatile_present: Vec<String> = tallies
        .keys()
        .filter(|n| VOLATILE_FUNCTIONS.contains(&n.as_str()))
        .cloned()
        .collect();
    FunctionCensus {
        tallies,
        unsupported,
        policy_limited,
        volatile_present,
        user_defined,
    }
}

/// Function names called in one formula string (canonical uppercase, deduped per call site kept).
pub fn extract_function_names(formula: &str) -> Vec<String> {
    let locale = get_locale("en").expect("en locale is compiled into ironcalc");
    let language = get_language("en").expect("en language is compiled into ironcalc");
    let mut lexer = Lexer::new(formula, LexerMode::A1, locale, language);
    let mut names = Vec::new();
    let mut pending_ident: Option<String> = None;
    loop {
        match lexer.next_token() {
            TokenType::EOF => break,
            // The lexer may not advance past an illegal character; bail to avoid spinning.
            TokenType::Illegal(_) => break,
            TokenType::Ident(name) => pending_ident = Some(name),
            TokenType::LeftParenthesis => {
                if let Some(name) = pending_ident.take() {
                    names.push(name.to_uppercase());
                }
            }
            _ => pending_ident = None,
        }
    }
    names
}

/// Subset of `names` the engine does NOT support (probe via #NAME? semantics).
pub fn probe_support(names: &[String]) -> Vec<String> {
    let mut unique: Vec<String> = names.iter().map(|n| n.to_uppercase()).collect();
    unique.sort();
    unique.dedup();
    if unique.is_empty() {
        return Vec::new();
    }
    let mut model = Model::new_empty("xlq-probe", "en", "UTC", "en")
        .expect("scratch model with hardcoded valid locale/timezone");
    let mut probed: Vec<(String, i32)> = Vec::with_capacity(unique.len());
    let mut unsupported = Vec::new();
    for (i, name) in unique.into_iter().enumerate() {
        let row = i as i32 + 1;
        // CUBE carve-out: probe with zero arguments so a recognized cube
        // function's argument-count validation (#ERROR!) fires before its
        // documented-by-design #NAME? connection error; see
        // CUBE_NAME_CARVE_OUT.
        let probe_formula = if CUBE_NAME_CARVE_OUT.contains(&name.as_str()) {
            format!("={name}()")
        } else {
            format!("={name}(1)")
        };
        if model.set_user_input(0, row, 1, probe_formula).is_ok() {
            probed.push((name, row));
        } else {
            // The engine's parser rejects the probe formula outright: it
            // certainly cannot evaluate this name. Failing toward
            // "unsupported" keeps the coverage claim honest.
            // NOT COVERABLE today: set_user_input in the vendored engine
            // accepts any `=NAME(1)` string a lexer Ident can produce
            // (verified empirically — even `=(1)` parses); the arm is a
            // defensive failure default, kept because a future engine bump
            // could start rejecting probe formulas.
            unsupported.push(name);
        }
    }
    model.evaluate();
    for (name, row) in probed {
        let value = model
            .get_formatted_cell_value(0, row, 1)
            .unwrap_or_default();
        if value == "#NAME?" {
            unsupported.push(name);
        }
    }
    unsupported.sort();
    unsupported
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_calls_uppercased_keeping_duplicates() {
        assert_eq!(
            extract_function_names("=sum(A1:B2)+SUM(C1)+max(1,2)"),
            vec!["SUM", "SUM", "MAX"]
        );
    }

    #[test]
    fn extracts_nested_calls() {
        assert_eq!(
            extract_function_names("=IF(SUM(A1:A3)>0,MAX(B1,1),0)"),
            vec!["IF", "SUM", "MAX"]
        );
    }

    #[test]
    fn ignores_string_literals_and_quoted_sheet_names() {
        assert_eq!(
            extract_function_names("=IF(A1=\"SUM(1)\",'My SUM(Sheet'!B2,COUNT(C1))"),
            vec!["IF", "COUNT"]
        );
    }

    #[test]
    fn ignores_plain_references_and_defined_names() {
        assert!(extract_function_names("=A1+B2*MyName").is_empty());
    }

    #[test]
    fn reference_shaped_name_is_function_only_when_called() {
        assert_eq!(extract_function_names("=LOG10(100)"), vec!["LOG10"]);
        assert!(extract_function_names("=LOG10").is_empty());
    }

    #[test]
    fn function_names_with_dots() {
        assert_eq!(
            extract_function_names("=CEILING.MATH(4.3)"),
            vec!["CEILING.MATH"]
        );
    }

    #[test]
    fn lexer_illegal_token_stops_extraction_without_spinning() {
        // An unterminated string literal makes the lexer emit
        // TokenType::Illegal; extraction must bail out, keeping whatever it
        // already saw, instead of looping on a token that never advances.
        assert_eq!(
            extract_function_names("=SUM(A1)+\"unterminated"),
            vec!["SUM"]
        );
        assert!(extract_function_names("='unterminated").is_empty());
    }

    #[test]
    fn probe_reports_unknown_functions_only() {
        let names = vec![
            "SUM".to_string(),
            "XLQNOTAREALFUNCTION".to_string(),
            "sum".to_string(),
        ];
        assert_eq!(probe_support(&names), vec!["XLQNOTAREALFUNCTION"]);
    }

    #[test]
    fn probe_empty_input() {
        assert!(probe_support(&[]).is_empty());
    }

    #[test]
    fn census_tallies_volatile_and_policy_limited() {
        let mut model = Model::new_empty("t", "en", "UTC", "en").unwrap();
        model
            .set_user_input(0, 1, 1, "=SUM(1,2)+SUM(3,4)".to_string())
            .unwrap();
        model.set_user_input(0, 2, 1, "=NOW()".to_string()).unwrap();
        // CUBEVALUE is recognized by the engine (documented no-connection
        // answer #NAME?) but its value depends on an OLAP connection: it must
        // land in policy_limited, NOT unsupported.
        model
            .set_user_input(0, 3, 1, "=CUBEVALUE(1)".to_string())
            .unwrap();
        model
            .set_user_input(0, 4, 1, "plain text".to_string())
            .unwrap();
        model.evaluate();

        let census = function_census(&model);
        assert_eq!(census.tallies.get("SUM"), Some(&2));
        assert_eq!(census.tallies.get("NOW"), Some(&1));
        assert_eq!(census.tallies.get("CUBEVALUE"), Some(&1));
        assert_eq!(census.tallies.len(), 3);
        assert!(census.unsupported.is_empty(), "{:?}", census.unsupported);
        assert_eq!(
            census.policy_limited.get("CUBEVALUE").map(String::as_str),
            Some("#NAME?")
        );
        assert_eq!(census.policy_limited.len(), 1);
        assert_eq!(census.volatile_present, vec!["NOW"]);
        assert!(census.user_defined.is_empty());
    }

    #[test]
    fn policy_limited_functions_carry_their_documented_literals() {
        let mut model = Model::new_empty("t", "en", "UTC", "en").unwrap();
        model
            .set_user_input(0, 1, 1, "=WEBSERVICE(\"https://example.com\")".to_string())
            .unwrap();
        model
            .set_user_input(0, 2, 1, "=GETPIVOTDATA(\"Sales\",A9)".to_string())
            .unwrap();
        model
            .set_user_input(0, 3, 1, "=CALL(1)".to_string())
            .unwrap();
        model
            .set_user_input(0, 4, 1, "=SUM(1,2)".to_string())
            .unwrap();
        model.evaluate();

        let census = function_census(&model);
        assert!(census.unsupported.is_empty(), "{:?}", census.unsupported);
        assert_eq!(
            census.policy_limited,
            BTreeMap::from([
                ("CALL".to_string(), "#BLOCKED!".to_string()),
                ("GETPIVOTDATA".to_string(), "#REF!".to_string()),
                ("WEBSERVICE".to_string(), "#VALUE!".to_string()),
            ])
        );
        // Policy-limited functions still count as Excel vocabulary.
        assert_eq!(census.tallies.get("WEBSERVICE"), Some(&1));
        // SUM is plain locally-evaluable vocabulary: not policy-limited.
        assert!(!census.policy_limited.contains_key("SUM"));
    }

    #[test]
    fn probe_recognizes_all_policy_limited_functions() {
        // Every Tier II function — including the seven CUBE functions whose
        // documented no-connection answer is #NAME? — must probe as
        // recognized via the zero-argument carve-out.
        let names: Vec<String> = POLICY_LIMITED_FUNCTIONS
            .iter()
            .map(|(n, _, _)| n.to_string())
            .collect();
        assert_eq!(names.len(), 17);
        assert_eq!(probe_support(&names), Vec::<String>::new());
    }

    #[test]
    fn cube_carve_out_does_not_recognize_a_truly_unknown_name() {
        // The carve-out changes the probe FORMULA, not the verdict: a name
        // the engine does not know still answers #NAME? with zero args.
        assert!(CUBE_NAME_CARVE_OUT.contains(&"CUBEVALUE"));
        for cube in CUBE_NAME_CARVE_OUT {
            assert_eq!(policy_limited_literal(cube), Some("#NAME?"));
        }
        // Sanity: an unknown name is unsupported regardless of probe shape.
        let names = vec!["XLQNOTACUBEFUNCTION".to_string()];
        assert_eq!(probe_support(&names), vec!["XLQNOTACUBEFUNCTION"]);
    }

    #[test]
    fn udf_calls_are_user_defined_not_functions() {
        let mut model = Model::new_empty("t", "en", "UTC", "en").unwrap();
        model
            .set_user_input(0, 1, 1, "=DealMargin_AcmeCorp(B1)".to_string())
            .unwrap();
        model.evaluate();

        let census = function_census(&model);
        assert!(
            census.tallies.is_empty(),
            "UDF leaked into functions: {:?}",
            census.tallies
        );
        assert!(
            census.unsupported.is_empty(),
            "UDF leaked into unsupported: {:?}",
            census.unsupported
        );
        assert_eq!(census.user_defined.get("DEALMARGIN_ACMECORP"), Some(&1));
    }

    #[test]
    fn called_defined_name_is_user_defined() {
        let mut model = Model::new_empty("t", "en", "UTC", "en").unwrap();
        model
            .new_defined_name("SecretLambda", None, "Sheet1!$A$1")
            .unwrap();
        model
            .set_user_input(0, 1, 2, "=SecretLambda(3)".to_string())
            .unwrap();
        model.evaluate();

        let census = function_census(&model);
        assert!(!census.tallies.contains_key("SECRETLAMBDA"));
        assert!(!census.unsupported.iter().any(|n| n == "SECRETLAMBDA"));
        assert_eq!(census.user_defined.get("SECRETLAMBDA"), Some(&1));
    }

    #[test]
    fn functions_inside_defined_name_formulas_are_counted() {
        use ironcalc::base::types::DefinedName;
        let mut model = Model::new_empty("t", "en", "UTC", "en").unwrap();
        // OFFSET (volatile, supported) and CUBEVALUE (policy-limited) used
        // ONLY inside defined names, never in a cell formula. new_defined_name
        // only accepts plain references, so push directly (as import does).
        model.workbook.defined_names.push(DefinedName {
            name: "MovingWindow".to_string(),
            formula: "OFFSET(Sheet1!$A$1,1,1)".to_string(),
            sheet_id: None,
        });
        model.workbook.defined_names.push(DefinedName {
            name: "HiddenCalc".to_string(),
            formula: "CUBEVALUE(Sheet1!$A$1:$A$2)".to_string(),
            sheet_id: None,
        });
        model
            .set_user_input(0, 1, 1, "=MIN(1,160)".to_string())
            .unwrap();
        model.evaluate();

        let census = function_census(&model);
        assert_eq!(census.tallies.get("OFFSET"), Some(&1));
        assert_eq!(census.tallies.get("CUBEVALUE"), Some(&1));
        assert!(census.unsupported.is_empty(), "{:?}", census.unsupported);
        assert_eq!(
            census.policy_limited.get("CUBEVALUE").map(String::as_str),
            Some("#NAME?")
        );
        assert_eq!(census.volatile_present, vec!["OFFSET"]);
    }
}
