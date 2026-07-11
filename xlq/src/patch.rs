//! The patch format for `xlq apply`.
//!
//! CONTRACT (other modules depend on these types + signatures):
//!   #[derive(Deserialize)] pub struct Patch {
//!       pub base_hash: String,          // sha256 the file must currently have
//!       pub actor: Option<String>,
//!       pub ops: Vec<Op>,
//!       pub watch: Vec<String>,         // A1 refs to report before/after in dry-run
//!       pub clock: Option<i64>,         // pinned epoch ms for volatile fns (optional)
//!       pub seed: Option<u64>,
//!   }
//!   #[serde(tag = "type", rename_all = "snake_case")] pub enum Op {
//!       SetCell { sheet: String, cell: String, value: serde_json::Value },
//!       SetFormula { sheet: String, cell: String, formula: String },
//!   }
//!   pub fn load(path: &str) -> anyhow::Result<Patch>
//!   pub fn parse_a1(cell: &str) -> anyhow::Result<(i32 /*row*/, i32 /*col*/)>
//!
//! SetCell.value is JSON number|string|bool|null mapped to Excel per value.rs
//! conventions (number→number, string→text, bool→bool, null→clear); dates go
//! as {"type":"date","iso":"YYYY-MM-DD"} (see full-catalog-semantics spec).
//! parse_a1("B7") -> (7, 2). Errors carry basenames only (no full paths).

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Patch {
    pub base_hash: String,
    #[serde(default)]
    pub actor: Option<String>,
    pub ops: Vec<Op>,
    #[serde(default)]
    pub watch: Vec<String>,
    #[serde(default)]
    pub clock: Option<i64>,
    #[serde(default)]
    pub seed: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Op {
    SetCell {
        sheet: String,
        cell: String,
        value: serde_json::Value,
    },
    SetFormula {
        sheet: String,
        cell: String,
        formula: String,
    },
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn load(path: &str) -> Result<Patch> {
    let name = basename(path);
    let text = std::fs::read_to_string(path).with_context(|| format!("reading patch {name}"))?;
    let patch: Patch =
        serde_json::from_str(&text).with_context(|| format!("parsing patch {name}"))?;
    Ok(patch)
}

/// Parse an A1 reference into (row, col), both 1-based.
/// "B7" -> (7, 2); "AA10" -> (10, 27). Rejects anything that is not a run of
/// ASCII letters followed by a run of ASCII digits.
pub fn parse_a1(cell: &str) -> Result<(i32, i32)> {
    let bytes = cell.as_bytes();
    let mut i = 0;
    let mut col: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        let d = (bytes[i].to_ascii_uppercase() - b'A') as i64 + 1;
        col = col * 26 + d;
        i += 1;
        if col > i32::MAX as i64 {
            return Err(anyhow!("column out of range in cell {cell}"));
        }
    }
    if i == 0 {
        return Err(anyhow!("missing column letters in cell {cell}"));
    }
    let digits = &cell[i..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(anyhow!("invalid row in cell {cell}"));
    }
    let row: i64 = digits
        .parse()
        .map_err(|_| anyhow!("invalid row in cell {cell}"))?;
    if row < 1 || row > i32::MAX as i64 {
        return Err(anyhow!("row out of range in cell {cell}"));
    }
    Ok((row as i32, col as i32))
}

/// Map a patch JSON value to the string IronCalc's `set_user_input` expects:
/// number -> the number's text, string -> the text, bool -> TRUE/FALSE,
/// null -> "" (clear), {"type":"date","iso":".."} -> the ISO date string.
pub fn value_to_input(v: &serde_json::Value) -> Result<String> {
    use serde_json::Value;
    match v {
        Value::Null => Ok(String::new()),
        Value::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s.clone()),
        Value::Object(map) => {
            let ty = map.get("type").and_then(|t| t.as_str());
            match ty {
                Some("date") => map
                    .get("iso")
                    .and_then(|i| i.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| anyhow!("date wrapper missing string 'iso' field")),
                Some(other) => Err(anyhow!("unsupported value wrapper type '{other}'")),
                None => Err(anyhow!("object value must be a typed wrapper (e.g. date)")),
            }
        }
        Value::Array(_) => Err(anyhow!("array is not a valid cell value")),
    }
}

/// JSON Schema (Draft-07) of the `xlq apply` patch format, printed by
/// `xlq apply --schema`. Hand-written and co-located with the `Patch`/`Op` types
/// so it doubles as the authoritative spec without pulling a schema-derivation
/// crate into a deliberately lean, security-audited dependency set. The
/// `schema_matches_the_deserializer` test keeps it in lockstep with the structs.
pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "xlq apply patch",
        "type": "object",
        "required": ["base_hash", "ops"],
        "additionalProperties": false,
        "properties": {
            "base_hash": {
                "type": "string",
                "description": "sha256 the target file must currently hash to; the write refuses on mismatch"
            },
            "actor": {
                "type": "string",
                "description": "actor recorded in the receipt (else --actor, then $XLQ_ACTOR, else \"unknown\")"
            },
            "ops": {
                "type": "array",
                "description": "ordered cell operations to apply",
                "items": {
                    "oneOf": [
                        {
                            "type": "object",
                            "required": ["type", "sheet", "cell", "value"],
                            "additionalProperties": false,
                            "properties": {
                                "type": {"const": "set_cell"},
                                "sheet": {"type": "string"},
                                "cell": {"type": "string", "description": "A1 reference, e.g. \"B7\""},
                                "value": {
                                    "description": "number|string|bool|null (null clears); a date is {\"type\":\"date\",\"iso\":\"YYYY-MM-DD\"}",
                                    "oneOf": [
                                        {"type": "number"},
                                        {"type": "string"},
                                        {"type": "boolean"},
                                        {"type": "null"},
                                        {
                                            "type": "object",
                                            "required": ["type", "iso"],
                                            "additionalProperties": false,
                                            "properties": {
                                                "type": {"const": "date"},
                                                "iso": {"type": "string", "description": "YYYY-MM-DD"}
                                            }
                                        }
                                    ]
                                }
                            }
                        },
                        {
                            "type": "object",
                            "required": ["type", "sheet", "cell", "formula"],
                            "additionalProperties": false,
                            "properties": {
                                "type": {"const": "set_formula"},
                                "sheet": {"type": "string"},
                                "cell": {"type": "string", "description": "A1 reference"},
                                "formula": {"type": "string", "description": "e.g. \"=A1+1\""}
                            }
                        }
                    ]
                }
            },
            "watch": {
                "type": "array",
                "items": {"type": "string"},
                "description": "A1 refs whose before/after values --dry-run reports"
            },
            "clock": {
                "type": "integer",
                "description": "pinned epoch-ms for volatile functions (determinism)"
            },
            "seed": {
                "type": "integer",
                "minimum": 0,
                "description": "pinned RNG seed (determinism)"
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_matches_the_deserializer() {
        let s = schema();
        let required = s["required"].as_array().expect("required is an array");
        assert!(
            required.iter().any(|v| v == "base_hash"),
            "base_hash required"
        );
        assert!(required.iter().any(|v| v == "ops"), "ops required");
        // Drift guard: an example conforming to the schema (both op kinds + a date
        // wrapper) must deserialize through the REAL Patch deserializer, so the
        // hand-written schema cannot silently diverge from the structs.
        let example = r#"{
            "base_hash": "h", "actor": "a",
            "ops": [
                {"type":"set_cell","sheet":"S","cell":"A1","value":1},
                {"type":"set_cell","sheet":"S","cell":"A2","value":{"type":"date","iso":"2026-01-01"}},
                {"type":"set_formula","sheet":"S","cell":"B1","formula":"=A1+1"}
            ],
            "watch": ["A1"], "clock": 0, "seed": 0
        }"#;
        let _p: Patch =
            serde_json::from_str(example).expect("schema example must match the deserializer");
    }

    #[test]
    fn parses_patch_with_both_ops() {
        let text = r#"{
            "base_hash": "abc123",
            "actor": "tester",
            "ops": [
                {"type": "set_cell", "sheet": "Sheet1", "cell": "B7", "value": 42},
                {"type": "set_formula", "sheet": "Sheet1", "cell": "C1", "formula": "=A1+1"}
            ],
            "watch": ["A1", "B7"],
            "clock": 1700000000000,
            "seed": 7
        }"#;
        let p: Patch = serde_json::from_str(text).unwrap();
        assert_eq!(p.base_hash, "abc123");
        assert_eq!(p.actor.as_deref(), Some("tester"));
        assert_eq!(p.watch, vec!["A1", "B7"]);
        assert_eq!(p.clock, Some(1700000000000));
        assert_eq!(p.seed, Some(7));
        assert_eq!(p.ops.len(), 2);
        match &p.ops[0] {
            Op::SetCell { sheet, cell, value } => {
                assert_eq!(sheet, "Sheet1");
                assert_eq!(cell, "B7");
                assert_eq!(value, &json!(42));
            }
            _ => panic!("expected set_cell"),
        }
        match &p.ops[1] {
            Op::SetFormula {
                sheet,
                cell,
                formula,
            } => {
                assert_eq!(sheet, "Sheet1");
                assert_eq!(cell, "C1");
                assert_eq!(formula, "=A1+1");
            }
            _ => panic!("expected set_formula"),
        }
    }

    #[test]
    fn patch_defaults_are_optional() {
        let text = r#"{"base_hash": "h", "ops": []}"#;
        let p: Patch = serde_json::from_str(text).unwrap();
        assert!(p.actor.is_none());
        assert!(p.watch.is_empty());
        assert!(p.clock.is_none());
        assert!(p.seed.is_none());
        assert!(p.ops.is_empty());
    }

    #[test]
    fn parse_a1_valid() {
        assert_eq!(parse_a1("A1").unwrap(), (1, 1));
        assert_eq!(parse_a1("Z1").unwrap(), (1, 26));
        assert_eq!(parse_a1("AA1").unwrap(), (1, 27));
        assert_eq!(parse_a1("AB100").unwrap(), (100, 28));
        assert_eq!(parse_a1("B7").unwrap(), (7, 2));
        assert_eq!(parse_a1("aa10").unwrap(), (10, 27));
    }

    #[test]
    fn parse_a1_rejects_junk() {
        assert!(parse_a1("").is_err());
        assert!(parse_a1("1").is_err());
        assert!(parse_a1("A").is_err());
        assert!(parse_a1("A0").is_err());
        assert!(parse_a1("A1B").is_err());
        assert!(parse_a1("$A$1").is_err());
        assert!(parse_a1("A 1").is_err());
        assert!(parse_a1("7B").is_err());
    }

    #[test]
    fn value_to_input_all_kinds() {
        assert_eq!(value_to_input(&json!(null)).unwrap(), "");
        assert_eq!(value_to_input(&json!(true)).unwrap(), "TRUE");
        assert_eq!(value_to_input(&json!(false)).unwrap(), "FALSE");
        assert_eq!(value_to_input(&json!(42)).unwrap(), "42");
        assert_eq!(value_to_input(&json!(3.5)).unwrap(), "3.5");
        assert_eq!(value_to_input(&json!("hello")).unwrap(), "hello");
        assert_eq!(
            value_to_input(&json!({"type": "date", "iso": "2026-07-03"})).unwrap(),
            "2026-07-03"
        );
    }

    #[test]
    fn value_to_input_rejects_bad_wrappers() {
        assert!(value_to_input(&json!([1, 2, 3])).is_err());
        assert!(value_to_input(&json!({"foo": "bar"})).is_err());
        assert!(value_to_input(&json!({"type": "money", "amount": 1})).is_err());
        assert!(value_to_input(&json!({"type": "date"})).is_err());
    }
}
