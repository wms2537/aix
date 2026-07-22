use crate::{
    formatter::{self, format::Formatted},
    locale::get_locale,
    types::NumFmt,
};

const DEFAULT_NUM_FMTS: &[&str] = &[
    "general",
    "0",
    "0.00",
    "#,##0",
    "#,##0.00",
    "$#,##0; \\ - $#,##0",
    "$#,##0; [Red] \\ - $#,##0",
    "$#,##0.00; \\ - $#,##0.00",
    "$#,##0.00; [Red] \\ - $#,##0.00",
    "0%",
    "0.00%",
    "0.00E + 00",
    "#?/?",
    "#?? / ??",
    "mm-dd-yy",
    "d-mmm-yy",
    "d-mmm",
    "mmm-yy",
    "h:mm AM / PM",
    "h:mm:ss AM / PM",
    "h:mm",
    "h:mm:ss",
    "m / d / yy h:mm",
    "#,##0;()#,##0)",
    "#,##0; [Red]()#,##0)",
    "#,##0.00;()#,##0.00)",
    "#,##0.00; [Red]()#,##0.00)",
    "_()$”*#,##0.00 _); _()$”* \\()#,##0.00\\); _()$”* - ?? _); _()@_)",
    "mm:ss",
    "[h]:mm:ss",
    "mmss .0",
    "##0.0E + 0",
    "@",
    "[$ -404] e / m / d ",
    "m / d / yy",
    "[$ -404] e / m / d",
    "[$ -404] e / / d",
    "[$ -404] e / m / d",
    "t0",
    "t0.00",
    "t#,##0",
    "t#,##0.00",
    "t0%",
    "t0.00 %",
    "t#?/?",
];

pub fn get_default_num_fmt_id(num_fmt: &str) -> Option<i32> {
    for (index, default_num_fmt) in DEFAULT_NUM_FMTS.iter().enumerate() {
        if default_num_fmt == &num_fmt {
            return Some(index as i32);
        };
    }
    None
}

pub fn get_num_fmt(num_fmt_id: i32, num_fmts: &[NumFmt]) -> String {
    // Check if it defined
    for num_fmt in num_fmts {
        if num_fmt.num_fmt_id == num_fmt_id {
            return num_fmt.format_code.clone();
        }
    }
    // Return one of the default ones
    if num_fmt_id < DEFAULT_NUM_FMTS.len() as i32 {
        return DEFAULT_NUM_FMTS[num_fmt_id as usize].to_string();
    }
    // Return general
    DEFAULT_NUM_FMTS[0].to_string()
}

pub fn get_new_num_fmt_index(num_fmts: &[NumFmt]) -> i32 {
    let mut index = DEFAULT_NUM_FMTS.len() as i32;
    let mut found = true;
    while found {
        found = false;
        for num_fmt in num_fmts {
            if num_fmt.num_fmt_id == index {
                found = true;
                index += 1;
                break;
            }
        }
    }
    index
}

pub fn to_precision(value: f64, precision: usize) -> f64 {
    if value.is_infinite() || value.is_nan() {
        return value;
    }
    to_precision_str(value, precision)
        .parse::<f64>()
        .unwrap_or({
            // TODO: do this in a way that does not require a possible error
            0.0
        })
}

/// It rounds a `f64` with `p` significant figures:
/// ```
///     use ironcalc_base::number_format;
///     assert_eq!(number_format::to_precision(0.1+0.2, 15), 0.3);
///     assert_eq!(number_format::to_excel_precision_str(0.1+0.2), "0.3");
/// ```
/// This intends to be equivalent to the js: `${parseFloat(value.toPrecision(precision)})`
/// See ([ecma](https://tc39.es/ecma262/#sec-number.prototype.toprecision)).
pub fn to_excel_precision_str(value: f64) -> String {
    to_precision_str(value, 15)
}

pub fn to_excel_precision(value: f64, precision: usize) -> f64 {
    if !value.is_finite() {
        return value;
    }

    let s = format!("{:.*e}", precision.saturating_sub(1), value);
    s.parse::<f64>().unwrap_or(value)
}

/// Render a number to text the way Excel's General format does when a number is COERCED to text
/// (the `&` operator, CONCATENATE, EXACT, …). An exact integer within f64 integer precision is
/// shown in FULL — Excel never rounds an integer to 15 significant figures, so `1234567890123456`
/// must stay `1234567890123456`, not `1234567890123460`. Other values use 15 significant figures
/// (`0.1+0.2` -> "0.3", `1/3` -> "0.333333333333333"). Very large/small magnitudes still fall back
/// to a scientific form whose exact exponent formatting differs across Excel/LibreOffice/this
/// engine — an inter-engine disagreement no single render can satisfy, so certify fail-safely
/// refuses such a coerced-text cache rather than vouching one spelling of it.
pub fn number_to_excel_text(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN".to_string()
        } else if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let abs = value.abs();
    // Excel's General coercion shows numbers in FIXED notation across a wide range at 15
    // SIGNIFICANT figures (`0.0000001`, `0.333333333333333`, `12345.678`), switching to scientific
    // only at the extremes. Compute the decimals for 15 sig figs from the base-10 exponent (via
    // `{:e}`, which is exact), then trim trailing zeros. Outside the fixed range, fall back to the
    // shortest 15-sig-fig form — a scientific spelling on which Excel/LibreOffice/this engine
    // disagree, so certify fail-safely refuses such a coerced cache rather than vouching one.
    if (1e-11..1e15).contains(&abs) {
        let exp: i32 = format!("{abs:e}")
            .rsplit('e')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let decimals = (14 - exp).max(0) as usize;
        let mut s = format!("{value:.decimals$}");
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        return s;
    }
    to_precision_str(value, 15)
}

pub fn to_precision_str(value: f64, precision: usize) -> String {
    if !value.is_finite() {
        if value.is_infinite() {
            return "inf".to_string();
        } else {
            return "NaN".to_string();
        }
    }

    let s = format!("{:.*e}", precision.saturating_sub(1), value);
    let parsed = s.parse::<f64>().unwrap_or(value);

    // I would love to use the std library. There is not a speed concern here
    // problem is it doesn't do the right thing
    // Also ryu is my favorite _modern_ algorithm
    let mut buffer = ryu::Buffer::new();
    let text = buffer.format(parsed);
    // The above algorithm converts 2 to 2.0 regrettably
    if let Some(stripped) = text.strip_suffix(".0") {
        return stripped.to_string();
    }
    text.to_string()
}

pub fn format_number(value: f64, format_code: &str, locale: &str) -> Formatted {
    let locale = match get_locale(locale) {
        Ok(l) => l,
        Err(_) => {
            return Formatted {
                text: "#ERROR!".to_owned(),
                color: None,
                error: Some("Invalid locale".to_string()),
            }
        }
    };
    formatter::format::format_number(value, format_code, locale)
}
