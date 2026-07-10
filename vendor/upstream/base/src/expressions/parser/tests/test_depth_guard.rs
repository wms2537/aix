#![allow(clippy::unwrap_used)]
//! Recursion-depth guard: a pathologically nested formula must return a parse
//! error, not overflow the stack and abort the process. Local hardening added
//! for the xlq vendored engine (untrusted .xlsx input); see `MAX_PARSE_DEPTH`.

use std::collections::HashMap;

use crate::expressions::parser::tests::utils::new_parser;
use crate::expressions::parser::Node;
use crate::expressions::types::CellReferenceRC;

fn ctx() -> CellReferenceRC {
    CellReferenceRC { sheet: "Sheet1".to_string(), row: 1, column: 1 }
}

#[test]
fn deeply_nested_parens_return_parse_error_not_overflow() {
    let mut parser = new_parser(vec!["Sheet1".to_string()], vec![], HashMap::new());
    // ~3000 levels — well past the process's ~2200 stack-overflow point.
    let formula = format!("{}1{}", "(".repeat(3000), ")".repeat(3000));
    let node = parser.parse(&formula, &ctx());
    assert!(
        matches!(node, Node::ParseErrorKind { .. }),
        "expected a parse error, got {node:?}"
    );
}

#[test]
fn deeply_nested_function_calls_return_parse_error() {
    let mut parser = new_parser(vec!["Sheet1".to_string()], vec![], HashMap::new());
    // nested via function arguments (a different recursion path than bare parens)
    let formula = format!("{}A1{}", "SUM(".repeat(2000), ")".repeat(2000));
    let node = parser.parse(&formula, &ctx());
    assert!(matches!(node, Node::ParseErrorKind { .. }), "got {node:?}");
}

#[test]
fn normal_and_moderately_nested_formulas_still_parse() {
    let mut parser = new_parser(vec!["Sheet1".to_string()], vec![], HashMap::new());
    // a realistic formula parses to a non-error node
    let node = parser.parse("SUM(A1:A5)*((1+2)*(3+4))", &ctx());
    assert!(!matches!(node, Node::ParseErrorKind { .. }), "got {node:?}");
    // depth ~200 (< the 256 cap) is still accepted
    let deep = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    let node = parser.parse(&deep, &ctx());
    assert!(!matches!(node, Node::ParseErrorKind { .. }), "got {node:?}");
}
