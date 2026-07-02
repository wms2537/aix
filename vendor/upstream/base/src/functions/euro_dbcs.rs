//! EUROCONVERT, DBCS/JIS, BAHTTEXT and PHONETIC: locale/currency text
//! functions that are fully computable offline (see
//! docs/specs/full-catalog-semantics.md, "Tier I").

use crate::{
    calc_result::CalcResult,
    expressions::{parser::Node, token::Error, types::CellReferenceIndex},
    model::Model,
    number_format::to_precision,
};

/// The 14 currencies of Excel's EUROCONVERT table (the euro itself plus the
/// 13 legacy currencies whose conversion rates were irrevocably fixed by EU
/// law). Each entry is (ISO code, units per 1 EUR, decimal places used for
/// the currency-specific rounding applied when `full_precision` is FALSE).
/// CYP/MTL/SKK/EEK/LVL/LTL/HRK are NOT in Excel's table.
const EUROCONVERT_RATES: [(&str, f64, i32); 14] = [
    ("ATS", 13.7603, 2),
    ("BEF", 40.3399, 0),
    ("DEM", 1.95583, 2),
    ("ESP", 166.386, 0),
    ("EUR", 1.0, 2),
    ("FIM", 5.94573, 2),
    ("FRF", 6.55957, 2),
    ("GRD", 340.75, 0),
    ("IEP", 0.787564, 2),
    ("ITL", 1936.27, 0),
    ("LUF", 40.3399, 0),
    ("NLG", 2.20371, 2),
    ("PTE", 200.482, 0),
    ("SIT", 239.64, 2),
];

/// The minimum value Excel accepts for `triangulation_precision`.
const EUROCONVERT_MIN_TRIANGULATION_PRECISION: f64 = 3.0;

fn euroconvert_rate(code: &str) -> Option<(f64, i32)> {
    EUROCONVERT_RATES
        .iter()
        .find(|(name, _, _)| *name == code)
        .map(|(_, rate, decimal_places)| (*rate, *decimal_places))
}

/// Rounds half away from zero to a number of decimal places, matching ROUND.
fn round_to_decimal_places(value: f64, decimal_places: i32) -> f64 {
    let scale = 10.0_f64.powi(decimal_places);
    (to_precision(value, 15) * scale).round() / scale
}

/// Full-width katakana equivalents of the half-width forms U+FF61..=U+FF9F,
/// indexed by `code_point - 0xFF61`.
const HALF_TO_FULL_KATAKANA: [char; 63] = [
    '。', '「', '」', '、', '・', 'ヲ', 'ァ', 'ィ', 'ゥ', 'ェ', 'ォ', 'ャ', 'ュ', 'ョ', 'ッ', 'ー',
    'ア', 'イ', 'ウ', 'エ', 'オ', 'カ', 'キ', 'ク', 'ケ', 'コ', 'サ', 'シ', 'ス', 'セ', 'ソ', 'タ',
    'チ', 'ツ', 'テ', 'ト', 'ナ', 'ニ', 'ヌ', 'ネ', 'ノ', 'ハ', 'ヒ', 'フ', 'ヘ', 'ホ', 'マ', 'ミ',
    'ム', 'メ', 'モ', 'ヤ', 'ユ', 'ヨ', 'ラ', 'リ', 'ル', 'レ', 'ロ', 'ワ', 'ン', '゛', '゜',
];

/// The voiced (dakuten) composition of a full-width katakana, if one exists:
/// カ..ト and ハ..ホ are one code point below their voiced forms, plus the
/// three irregular pairs ウ→ヴ, ワ→ヷ and ヲ→ヺ.
fn compose_voiced_katakana(c: char) -> Option<char> {
    match c {
        'カ' | 'キ' | 'ク' | 'ケ' | 'コ' | 'サ' | 'シ' | 'ス' | 'セ' | 'ソ' | 'タ' | 'チ'
        | 'ツ' | 'テ' | 'ト' | 'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ' => {
            char::from_u32(c as u32 + 1)
        }
        'ウ' => Some('ヴ'),
        'ワ' => Some('ヷ'),
        'ヲ' => Some('ヺ'),
        _ => None,
    }
}

/// The semi-voiced (handakuten) composition of a full-width katakana, if one
/// exists: ハ..ホ are two code points below their semi-voiced forms.
fn compose_semi_voiced_katakana(c: char) -> Option<char> {
    match c {
        'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ' => char::from_u32(c as u32 + 2),
        _ => None,
    }
}

const THAI_DIGITS: [&str; 10] = [
    "ศูนย์",
    "หนึ่ง",
    "สอง",
    "สาม",
    "สี่",
    "ห้า",
    "หก",
    "เจ็ด",
    "แปด",
    "เก้า",
];

/// Thai place names for the six digit positions inside a block; the units
/// position has no place name.
const THAI_PLACES: [&str; 6] = ["", "สิบ", "ร้อย", "พัน", "หมื่น", "แสน"];

/// Renders a single six-digit block (1..=999_999) as Thai text.
/// Irregularities: 1 in the tens place is สิบ (not หนึ่งสิบ), 2 in the tens
/// place is ยี่สิบ (not สองสิบ), and 1 in the units place is เอ็ด instead of
/// หนึ่ง only when the tens digit of the same block is non-zero — Excel says
/// หนึ่งร้อยหนึ่ง for 101, not the colloquial หนึ่งร้อยเอ็ด.
fn thai_block_text(block: u64) -> String {
    let mut result = String::new();
    for position in (0..6).rev() {
        let digit = ((block / 10u64.pow(position)) % 10) as usize;
        if digit == 0 {
            continue;
        }
        match position {
            1 => {
                if digit == 2 {
                    result.push_str("ยี่");
                } else if digit != 1 {
                    result.push_str(THAI_DIGITS[digit]);
                }
                result.push_str("สิบ");
            }
            0 => {
                let tens = (block / 10) % 10;
                if digit == 1 && tens > 0 {
                    result.push_str("เอ็ด");
                } else {
                    result.push_str(THAI_DIGITS[digit]);
                }
            }
            _ => {
                result.push_str(THAI_DIGITS[digit]);
                result.push_str(THAI_PLACES[position as usize]);
            }
        }
    }
    result
}

/// Renders a positive integer as Thai text. The number is split into
/// six-digit blocks from the right and every block boundary contributes a
/// ล้าน (million), stacking for higher powers: 10^12 is หนึ่งล้านล้าน.
fn thai_integer_text(n: u64) -> String {
    let mut blocks = Vec::new();
    let mut rest = n;
    while rest > 0 {
        blocks.push(rest % 1_000_000);
        rest /= 1_000_000;
    }
    let mut result = String::new();
    for (index, block) in blocks.iter().enumerate().rev() {
        if *block > 0 {
            result.push_str(&thai_block_text(*block));
        }
        if index > 0 {
            result.push_str("ล้าน");
        }
    }
    result
}

impl<'a> Model<'a> {
    // EUROCONVERT(number, source, target, [full_precision], [triangulation_precision])
    // Offline conversion between the euro and the legacy currencies whose
    // rates were irrevocably fixed by EU law. A legacy→legacy conversion
    // triangulates through the euro; `triangulation_precision` (an integer
    // >= 3) rounds the intermediate euro value to that many DECIMAL PLACES
    // (Excel's documentation says "significant digits" but its own worked
    // example computes decimal places; spec "Tier I", EUROCONVERT row).
    // When `full_precision` is FALSE or omitted the result is rounded to the
    // target currency-specific number of decimal places.
    pub(crate) fn fn_euroconvert(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() < 3 || args.len() > 5 {
            return CalcResult::new_args_number_error(cell);
        }
        let number = match self.get_number(&args[0], cell) {
            Ok(f) => f,
            Err(error) => return error,
        };
        let source = match self.get_string(&args[1], cell) {
            Ok(s) => s.to_ascii_uppercase(),
            Err(error) => return error,
        };
        let target = match self.get_string(&args[2], cell) {
            Ok(s) => s.to_ascii_uppercase(),
            Err(error) => return error,
        };
        let full_precision = if args.len() > 3 {
            match self.get_boolean(&args[3], cell) {
                Ok(b) => b,
                Err(error) => return error,
            }
        } else {
            false
        };
        let triangulation_precision = if args.len() > 4 {
            match self.evaluate_node_in_context(&args[4], cell) {
                CalcResult::EmptyArg => None,
                value => match self.cast_to_number(value, cell) {
                    Ok(f) => {
                        if f < EUROCONVERT_MIN_TRIANGULATION_PRECISION {
                            return CalcResult::new_error(
                                Error::VALUE,
                                cell,
                                "Triangulation precision must be at least 3".to_string(),
                            );
                        }
                        Some(f.trunc() as i32)
                    }
                    Err(error) => return error,
                },
            }
        } else {
            None
        };
        let (source_rate, _) = match euroconvert_rate(&source) {
            Some(entry) => entry,
            None => {
                return CalcResult::new_error(
                    Error::VALUE,
                    cell,
                    "Invalid source currency".to_string(),
                )
            }
        };
        let (target_rate, target_decimal_places) = match euroconvert_rate(&target) {
            Some(entry) => entry,
            None => {
                return CalcResult::new_error(
                    Error::VALUE,
                    cell,
                    "Invalid target currency".to_string(),
                )
            }
        };
        if source == target {
            return CalcResult::Number(number);
        }
        let result = if source == "EUR" {
            number * target_rate
        } else if target == "EUR" {
            number / source_rate
        } else {
            let mut intermediate = number / source_rate;
            if let Some(decimal_places) = triangulation_precision {
                intermediate = round_to_decimal_places(intermediate, decimal_places);
            }
            intermediate * target_rate
        };
        if full_precision {
            CalcResult::Number(result)
        } else {
            CalcResult::Number(round_to_decimal_places(result, target_decimal_places))
        }
    }

    // DBCS(text) / JIS(text)
    // Half-width to full-width conversion: ASCII U+0021..=U+007E maps to
    // U+FF01..=U+FF5E and half-width katakana U+FF61..=U+FF9F maps to
    // full-width, composing a following voiced/semi-voiced sound mark into a
    // single character (ｶ + ﾞ → ガ). Everything else is left unchanged and
    // the function never errors on text. DBCS and JIS are two names for the
    // same built-in (ECMA-376 §18.17.7 stores JIS).
    pub(crate) fn fn_dbcs(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() != 1 {
            return CalcResult::new_args_number_error(cell);
        }
        let text = match self.get_string(&args[0], cell) {
            Ok(s) => s,
            Err(error) => return error,
        };
        let mut result = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\u{0021}'..='\u{007E}' => match char::from_u32(c as u32 + 0xFEE0) {
                    Some(full_width) => result.push(full_width),
                    None => result.push(c),
                },
                '\u{FF61}'..='\u{FF9F}' => {
                    let mut full_width = HALF_TO_FULL_KATAKANA[c as usize - 0xFF61];
                    let composed = match chars.peek() {
                        Some('\u{FF9E}') => compose_voiced_katakana(full_width),
                        Some('\u{FF9F}') => compose_semi_voiced_katakana(full_width),
                        _ => None,
                    };
                    if let Some(composed) = composed {
                        full_width = composed;
                        chars.next();
                    }
                    result.push(full_width);
                }
                _ => result.push(c),
            }
        }
        CalcResult::String(result)
    }

    // BAHTTEXT(number)
    // Converts a number to Thai text money: the absolute value is rounded to
    // two decimal places, the baht part is rendered in six-digit blocks with
    // ล้าน stacking, a whole amount ends in บาทถ้วน, satang follow as
    // ...สตางค์ and an amount under one baht has no บาท at all. Zero is
    // ศูนย์บาทถ้วน and the negative prefix ลบ is applied after the zero
    // check. Spec "Tier I", BAHTTEXT row.
    pub(crate) fn fn_bahttext(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() != 1 {
            return CalcResult::new_args_number_error(cell);
        }
        let number = match self.get_number(&args[0], cell) {
            Ok(f) => f,
            Err(error) => return error,
        };
        let total_satang = (number.abs() * 100.0).round();
        // Guard the u64 conversion below; Excel's behavior for amounts this
        // large is undocumented [U].
        if !total_satang.is_finite() || total_satang > u64::MAX as f64 {
            return CalcResult::new_error(Error::VALUE, cell, "Number too large".to_string());
        }
        let total_satang = total_satang as u64;
        if total_satang == 0 {
            return CalcResult::String("ศูนย์บาทถ้วน".to_string());
        }
        let baht = total_satang / 100;
        let satang = total_satang % 100;
        let mut result = String::new();
        if number < 0.0 {
            result.push_str("ลบ");
        }
        if baht > 0 {
            result.push_str(&thai_integer_text(baht));
            result.push_str("บาท");
        }
        if satang == 0 {
            result.push_str("ถ้วน");
        } else {
            result.push_str(&thai_block_text(satang));
            result.push_str("สตางค์");
        }
        CalcResult::String(result)
    }

    // PHONETIC(reference)
    // Excel concatenates the <rPh> furigana runs stored with the shared
    // string (ISO 29500 §18.4.6) and falls back to the cell's own text,
    // unchanged, when the string has no phonetic runs. The xlsx importer
    // flattens every <si> to the concatenation of its <t> descendants, so
    // structured phonetic runs never survive an import and this
    // implementation always takes the documented no-runs fallback. A
    // multi-cell reference uses its upper-left cell; the #N/A Excel returns
    // for a nonadjacent range cannot arise because the parser has no range
    // union. Spec "Tier I", PHONETIC row.
    pub(crate) fn fn_phonetic(&mut self, args: &[Node], cell: CellReferenceIndex) -> CalcResult {
        if args.len() != 1 {
            return CalcResult::new_args_number_error(cell);
        }
        let value = match self.evaluate_node_in_context(&args[0], cell) {
            CalcResult::Range { left, .. } => self.evaluate_cell(left),
            value => value,
        };
        match self.cast_to_string(value, cell) {
            Ok(text) => CalcResult::String(text),
            Err(error) => error,
        }
    }
}
