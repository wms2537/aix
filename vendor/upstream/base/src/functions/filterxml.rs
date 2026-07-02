use std::collections::HashMap;

use roxmltree::{Document, Node as XmlNode};

use crate::{
    calc_result::CalcResult,
    expressions::{
        parser::{ArrayNode, Node},
        token::Error,
        types::CellReferenceIndex,
    },
    model::Model,
};

// FILTERXML evaluates an XPath 1.0 subset (the constructs Excel's MSXML
// backend is documented to support) over an XML string:
//   * absolute location paths: `/a/b/c`
//   * the descendant-or-self shorthand `//`, anywhere in the path
//   * attribute selection as a final step: `@attr`
//   * the text node test as a final step: `text()`
//   * positional predicates: `[n]` (1-based) and `[last()]`
//   * boolean predicates: `contains()`, `starts-with()` and `not()`, with
//     `text()`, `@attr` or a string literal as arguments
// Anything outside this subset is a bad xpath and produces #VALUE!.

// ── XPath subset AST ──────────────────────────────────────────────────────────

/// A single location step.
struct Step<'x> {
    /// `true` when the step was preceded by `//` instead of `/`.
    descendant: bool,
    test: NodeTest<'x>,
    predicates: Vec<Predicate<'x>>,
}

enum NodeTest<'x> {
    /// `name` or `prefix:name`
    Element {
        prefix: Option<&'x str>,
        name: &'x str,
    },
    /// `@name` or `@prefix:name`. Only valid as the final step.
    Attribute {
        prefix: Option<&'x str>,
        name: &'x str,
    },
    /// `text()`. Only valid as the final step.
    Text,
}

enum Predicate<'x> {
    /// `[n]`, 1-based like XPath
    Position(usize),
    /// `[last()]`
    Last,
    /// `[contains(..)]`, `[starts-with(..)]` or `[not(..)]`
    Condition(BoolExpr<'x>),
}

enum BoolExpr<'x> {
    Contains(StrExpr<'x>, StrExpr<'x>),
    StartsWith(StrExpr<'x>, StrExpr<'x>),
    Not(Box<BoolExpr<'x>>),
}

enum StrExpr<'x> {
    /// `'literal'` or `"literal"`
    Literal(&'x str),
    /// `text()`
    Text,
    /// `@name` or `@prefix:name`
    Attribute {
        prefix: Option<&'x str>,
        name: &'x str,
    },
}

// ── XPath parser ──────────────────────────────────────────────────────────────

struct Cursor<'x> {
    input: &'x str,
    position: usize,
}

impl<'x> Cursor<'x> {
    fn new(input: &'x str) -> Cursor<'x> {
        Cursor { input, position: 0 }
    }

    fn rest(&self) -> &'x str {
        &self.input[self.position..]
    }

    fn is_done(&self) -> bool {
        self.position == self.input.len()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.position += expected.len_utf8();
            return true;
        }
        false
    }

    fn eat_str(&mut self, expected: &str) -> bool {
        if self.rest().starts_with(expected) {
            self.position += expected.len();
            return true;
        }
        false
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if !c.is_whitespace() {
                break;
            }
            self.position += c.len_utf8();
        }
    }

    /// Parses an XML NCName (no colon). Returns `None` if there is none.
    fn ncname(&mut self) -> Option<&'x str> {
        let start = self.position;
        match self.peek() {
            Some(c) if c.is_alphabetic() || c == '_' => self.position += c.len_utf8(),
            _ => return None,
        }
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                self.position += c.len_utf8();
            } else {
                break;
            }
        }
        Some(&self.input[start..self.position])
    }

    /// Parses `name` or `prefix:name`.
    fn qname(&mut self) -> Option<(Option<&'x str>, &'x str)> {
        let first = self.ncname()?;
        if self.eat(':') {
            let second = self.ncname()?;
            Some((Some(first), second))
        } else {
            Some((None, first))
        }
    }

    /// Parses a string literal delimited by `'` or `"`. XPath 1.0 literals
    /// have no escape sequences: the literal simply ends at the next quote.
    fn literal(&mut self) -> Option<&'x str> {
        let quote = match self.peek() {
            Some(q @ ('\'' | '"')) => q,
            _ => return None,
        };
        self.position += 1;
        let start = self.position;
        let end = start + self.rest().find(quote)?;
        self.position = end + 1;
        Some(&self.input[start..end])
    }
}

/// Parses the supported XPath subset. `None` means a bad xpath.
fn parse_xpath(xpath: &str) -> Option<Vec<Step<'_>>> {
    let mut cursor = Cursor::new(xpath);
    let mut steps = Vec::new();
    cursor.skip_whitespace();
    loop {
        let descendant = if cursor.eat_str("//") {
            true
        } else if cursor.eat('/') {
            false
        } else {
            // Every step, including the first, needs a leading `/` or `//`
            return None;
        };
        let test = parse_node_test(&mut cursor)?;
        let mut predicates = Vec::new();
        cursor.skip_whitespace();
        while cursor.eat('[') {
            predicates.push(parse_predicate(&mut cursor)?);
            cursor.skip_whitespace();
        }
        // `@attr` and `text()` select leaves; nothing can follow them
        let is_leaf = !matches!(test, NodeTest::Element { .. });
        steps.push(Step {
            descendant,
            test,
            predicates,
        });
        cursor.skip_whitespace();
        if cursor.is_done() {
            return Some(steps);
        }
        if is_leaf || cursor.peek() != Some('/') {
            return None;
        }
    }
}

fn parse_node_test<'x>(cursor: &mut Cursor<'x>) -> Option<NodeTest<'x>> {
    cursor.skip_whitespace();
    if cursor.eat('@') {
        let (prefix, name) = cursor.qname()?;
        return Some(NodeTest::Attribute { prefix, name });
    }
    let (prefix, name) = cursor.qname()?;
    if prefix.is_none() && name == "text" {
        let saved = cursor.position;
        cursor.skip_whitespace();
        if cursor.eat('(') {
            cursor.skip_whitespace();
            if !cursor.eat(')') {
                return None;
            }
            return Some(NodeTest::Text);
        }
        // An element actually named "text"
        cursor.position = saved;
    }
    Some(NodeTest::Element { prefix, name })
}

/// Parses a predicate after the opening `[`, consuming the closing `]`.
fn parse_predicate<'x>(cursor: &mut Cursor<'x>) -> Option<Predicate<'x>> {
    cursor.skip_whitespace();
    let predicate = if cursor.peek().is_some_and(|c| c.is_ascii_digit()) {
        let start = cursor.position;
        while cursor.peek().is_some_and(|c| c.is_ascii_digit()) {
            cursor.position += 1;
        }
        let position: usize = cursor.input[start..cursor.position].parse().ok()?;
        if position == 0 {
            return None;
        }
        Predicate::Position(position)
    } else if cursor.eat_str("last") {
        cursor.skip_whitespace();
        if !cursor.eat('(') {
            return None;
        }
        cursor.skip_whitespace();
        if !cursor.eat(')') {
            return None;
        }
        Predicate::Last
    } else {
        Predicate::Condition(parse_bool_expr(cursor)?)
    };
    cursor.skip_whitespace();
    if !cursor.eat(']') {
        return None;
    }
    Some(predicate)
}

fn parse_bool_expr<'x>(cursor: &mut Cursor<'x>) -> Option<BoolExpr<'x>> {
    cursor.skip_whitespace();
    let name = cursor.ncname()?;
    cursor.skip_whitespace();
    if !cursor.eat('(') {
        return None;
    }
    let expr = match name {
        "contains" | "starts-with" => {
            let first = parse_str_expr(cursor)?;
            cursor.skip_whitespace();
            if !cursor.eat(',') {
                return None;
            }
            let second = parse_str_expr(cursor)?;
            if name == "contains" {
                BoolExpr::Contains(first, second)
            } else {
                BoolExpr::StartsWith(first, second)
            }
        }
        "not" => BoolExpr::Not(Box::new(parse_bool_expr(cursor)?)),
        _ => return None,
    };
    cursor.skip_whitespace();
    if !cursor.eat(')') {
        return None;
    }
    Some(expr)
}

fn parse_str_expr<'x>(cursor: &mut Cursor<'x>) -> Option<StrExpr<'x>> {
    cursor.skip_whitespace();
    match cursor.peek()? {
        '\'' | '"' => Some(StrExpr::Literal(cursor.literal()?)),
        '@' => {
            cursor.position += 1;
            let (prefix, name) = cursor.qname()?;
            Some(StrExpr::Attribute { prefix, name })
        }
        _ => {
            if !cursor.eat_str("text") {
                return None;
            }
            cursor.skip_whitespace();
            if !cursor.eat('(') {
                return None;
            }
            cursor.skip_whitespace();
            if !cursor.eat(')') {
                return None;
            }
            Some(StrExpr::Text)
        }
    }
}

// ── XPath evaluation ──────────────────────────────────────────────────────────

const INVALID_PREFIX: &str = "Invalid namespace prefix";

/// Collects every `xmlns:prefix` declaration in the document so xpath
/// prefixes can be resolved to namespace URIs.
fn collect_namespaces(document: &Document) -> HashMap<String, String> {
    let mut namespaces = HashMap::new();
    for node in document.descendants().filter(|n| n.is_element()) {
        for declaration in node.namespaces() {
            if let Some(prefix) = declaration.name() {
                namespaces
                    .entry(prefix.to_string())
                    .or_insert_with(|| declaration.uri().to_string());
            }
        }
    }
    namespaces
}

fn resolve_prefix<'a>(
    prefix: Option<&str>,
    namespaces: &'a HashMap<String, String>,
) -> Result<Option<&'a str>, &'static str> {
    match prefix {
        Some(p) => match namespaces.get(p) {
            Some(uri) => Ok(Some(uri)),
            None => Err(INVALID_PREFIX),
        },
        None => Ok(None),
    }
}

/// The XPath string-value of an element: all descendant text, concatenated.
fn string_value(node: &XmlNode) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .map(|n| n.text().unwrap_or(""))
        .collect()
}

/// The string-value of `text()` in a predicate argument: XPath converts a
/// node-set to a string by taking its first node, i.e. the element's first
/// direct text child.
fn first_text<'a>(node: &XmlNode<'a, '_>) -> &'a str {
    node.children()
        .find(|n| n.is_text())
        .and_then(|n| n.text())
        .unwrap_or("")
}

fn attribute_value<'a>(
    node: &XmlNode<'a, '_>,
    prefix: Option<&str>,
    name: &str,
    namespaces: &HashMap<String, String>,
) -> Result<Option<&'a str>, &'static str> {
    let uri = resolve_prefix(prefix, namespaces)?;
    Ok(node
        .attributes()
        .find(|a| a.name() == name && a.namespace() == uri)
        .map(|a| a.value()))
}

fn eval_str_expr<'a>(
    node: &XmlNode<'a, '_>,
    expr: &StrExpr<'a>,
    namespaces: &HashMap<String, String>,
) -> Result<&'a str, &'static str> {
    match expr {
        StrExpr::Literal(s) => Ok(s),
        StrExpr::Text => Ok(first_text(node)),
        StrExpr::Attribute { prefix, name } => {
            Ok(attribute_value(node, *prefix, name, namespaces)?.unwrap_or(""))
        }
    }
}

fn eval_bool_expr(
    node: &XmlNode,
    expr: &BoolExpr,
    namespaces: &HashMap<String, String>,
) -> Result<bool, &'static str> {
    match expr {
        BoolExpr::Contains(haystack, needle) => {
            let haystack = eval_str_expr(node, haystack, namespaces)?;
            let needle = eval_str_expr(node, needle, namespaces)?;
            Ok(haystack.contains(needle))
        }
        BoolExpr::StartsWith(haystack, needle) => {
            let haystack = eval_str_expr(node, haystack, namespaces)?;
            let needle = eval_str_expr(node, needle, namespaces)?;
            Ok(haystack.starts_with(needle))
        }
        BoolExpr::Not(inner) => Ok(!eval_bool_expr(node, inner, namespaces)?),
    }
}

/// Applies the predicates of one step to the candidates produced from a
/// single context node, so positions are relative to that context (XPath
/// semantics: `/a/b[2]` is the second `b` of *each* `a`).
fn apply_node_predicates<'a, 'input>(
    mut candidates: Vec<XmlNode<'a, 'input>>,
    predicates: &[Predicate],
    namespaces: &HashMap<String, String>,
) -> Result<Vec<XmlNode<'a, 'input>>, &'static str> {
    for predicate in predicates {
        match predicate {
            Predicate::Position(n) => {
                candidates = match candidates.get(*n - 1) {
                    Some(node) => vec![*node],
                    None => vec![],
                };
            }
            Predicate::Last => {
                candidates = match candidates.last() {
                    Some(node) => vec![*node],
                    None => vec![],
                };
            }
            Predicate::Condition(expr) => {
                let mut kept = Vec::new();
                for node in candidates {
                    if eval_bool_expr(&node, expr, namespaces)? {
                        kept.push(node);
                    }
                }
                candidates = kept;
            }
        }
    }
    Ok(candidates)
}

/// Applies predicates to the string candidates of a final `@attr` or
/// `text()` step. Boolean predicates need a context element, so only the
/// positional forms are meaningful here.
fn apply_string_predicates(
    mut candidates: Vec<String>,
    predicates: &[Predicate],
) -> Result<Vec<String>, &'static str> {
    for predicate in predicates {
        match predicate {
            Predicate::Position(n) => {
                candidates = if *n <= candidates.len() {
                    vec![candidates.swap_remove(*n - 1)]
                } else {
                    vec![]
                };
            }
            Predicate::Last => {
                candidates = match candidates.pop() {
                    Some(value) => vec![value],
                    None => vec![],
                };
            }
            Predicate::Condition(_) => return Err("Invalid XPath"),
        }
    }
    Ok(candidates)
}

/// The nodes a step is applied to. A plain `/` step keeps the current
/// contexts; a `//` step expands each to its descendant-or-self set (only
/// nodes that can have children matter). Sorting by node id restores
/// document order and lets us deduplicate overlapping expansions.
fn base_nodes<'a, 'input>(
    contexts: &[XmlNode<'a, 'input>],
    descendant: bool,
) -> Vec<XmlNode<'a, 'input>> {
    let mut bases: Vec<XmlNode> = if descendant {
        contexts
            .iter()
            .flat_map(|c| c.descendants())
            .filter(|n| n.is_element() || n.is_root())
            .collect()
    } else {
        contexts.to_vec()
    };
    bases.sort_by_key(|n| n.id().get_usize());
    bases.dedup_by_key(|n| n.id().get_usize());
    bases
}

/// Evaluates the parsed steps over the document. Returns the string value
/// of every match, in document order.
fn eval_steps(document: &Document, steps: &[Step]) -> Result<Vec<String>, &'static str> {
    let namespaces = collect_namespaces(document);
    // The initial context is the document node itself, so the first `/step`
    // matches against the root element.
    let mut contexts: Vec<XmlNode> = vec![document.root()];
    for step in steps {
        let bases = base_nodes(&contexts, step.descendant);
        match &step.test {
            NodeTest::Element { prefix, name } => {
                let uri = resolve_prefix(*prefix, &namespaces)?;
                let mut next = Vec::new();
                for base in bases {
                    let candidates: Vec<XmlNode> = base
                        .children()
                        .filter(|c| {
                            c.is_element()
                                && c.tag_name().name() == *name
                                && c.tag_name().namespace() == uri
                        })
                        .collect();
                    next.extend(apply_node_predicates(
                        candidates,
                        &step.predicates,
                        &namespaces,
                    )?);
                }
                // Child sets of distinct bases are disjoint, so `next` is
                // already duplicate-free and in document order.
                contexts = next;
            }
            // The parser guarantees the attribute and text tests are final
            NodeTest::Attribute { prefix, name } => {
                let mut matches = Vec::new();
                for base in bases {
                    let candidates: Vec<String> =
                        attribute_value(&base, *prefix, name, &namespaces)?
                            .map(str::to_string)
                            .into_iter()
                            .collect();
                    matches.extend(apply_string_predicates(candidates, &step.predicates)?);
                }
                return Ok(matches);
            }
            NodeTest::Text => {
                let mut matches = Vec::new();
                for base in bases {
                    let candidates: Vec<String> = base
                        .children()
                        .filter(|c| c.is_text())
                        .map(|c| c.text().unwrap_or("").to_string())
                        .collect();
                    matches.extend(apply_string_predicates(candidates, &step.predicates)?);
                }
                return Ok(matches);
            }
        }
    }
    Ok(contexts.iter().map(string_value).collect())
}

/// Excel returns matches that look like numbers as numbers.
fn parse_number(value: &str) -> Option<f64> {
    match value.trim().parse::<f64>() {
        Ok(number) if number.is_finite() => Some(number),
        _ => None,
    }
}

impl<'a> Model<'a> {
    // FILTERXML(xml, xpath)
    // Pure local evaluation of an XPath 1.0 subset over an XML string
    // (MSXML lineage). Multiple matches spill vertically; invalid xml,
    // invalid namespace prefixes, bad xpaths and empty results are all
    // #VALUE!, matching Excel.
    pub(crate) fn fn_filterxml(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() != 2 {
            return CalcResult::new_args_number_error(cell);
        }
        let xml = match self.get_string(&args[0], cell) {
            Ok(s) => s,
            Err(error) => return error,
        };
        let xpath = match self.get_string(&args[1], cell) {
            Ok(s) => s,
            Err(error) => return error,
        };
        let document = match Document::parse(&xml) {
            Ok(d) => d,
            Err(_) => return CalcResult::new_error(Error::VALUE, cell, "Invalid XML".to_string()),
        };
        let steps = match parse_xpath(&xpath) {
            Some(s) => s,
            None => return CalcResult::new_error(Error::VALUE, cell, "Invalid XPath".to_string()),
        };
        let mut matches = match eval_steps(&document, &steps) {
            Ok(m) => m,
            Err(message) => return CalcResult::new_error(Error::VALUE, cell, message.to_string()),
        };
        match matches.len() {
            0 => CalcResult::new_error(Error::VALUE, cell, "No match".to_string()),
            1 => {
                let value = matches.remove(0);
                match parse_number(&value) {
                    Some(number) => CalcResult::Number(number),
                    None => CalcResult::String(value),
                }
            }
            _ => CalcResult::Array(
                matches
                    .into_iter()
                    .map(|value| match parse_number(&value) {
                        Some(number) => vec![ArrayNode::Number(number)],
                        None => vec![ArrayNode::String(value)],
                    })
                    .collect(),
            ),
        }
    }
}
