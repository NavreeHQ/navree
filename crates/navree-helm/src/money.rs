//! Amounts in minor units: formatting (as navree-view's `money` filter
//! does) and parsing decimal input.

use std::fmt::Write as _;

/// `cents` of currency `code` as `$12,500.00`, `¥150,000` or `CHF 19.99`.
pub(crate) fn format(cents: i64, code: &str) -> String {
    let code = code.trim().to_ascii_uppercase();
    let decimals = minor_units(&code);
    let divisor = 10_u64.pow(decimals);
    let amount = cents.unsigned_abs();
    let mut out = String::new();
    if cents < 0 {
        out.push('-');
    }
    match symbol(&code) {
        Some(symbol) => out.push_str(symbol),
        None if code.is_empty() => {}
        None => {
            out.push_str(&code);
            out.push(' ');
        }
    }
    out.push_str(&group_digits(amount / divisor, code == "INR"));
    if decimals > 0 {
        let width = decimals as usize;
        let _ = write!(out, ".{:0width$}", amount % divisor);
    }
    out
}

/// A decimal amount in major units (`12500`, `12,500.50`) as minor units
/// of `code`; `None` when it is not a number or has more decimals than the
/// currency.
pub(crate) fn parse_major(text: &str, code: &str) -> Option<i64> {
    let decimals = minor_units(&code.trim().to_ascii_uppercase());
    let text = text.trim().replace(',', "");
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.to_owned()),
        None => (false, text),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((&digits, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if fraction.len() > decimals as usize {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut fraction = fraction.to_owned();
    while fraction.len() < decimals as usize {
        fraction.push('0');
    }
    let fraction: i64 = if fraction.is_empty() {
        0
    } else {
        fraction.parse().ok()?
    };
    let minor = whole
        .checked_mul(10_i64.pow(decimals))?
        .checked_add(fraction)?;
    Some(if negative { -minor } else { minor })
}

/// Minor units as a plain decimal in major units (`1250000` USD is
/// `12500.00`), for form inputs.
pub(crate) fn to_major(cents: i64, code: &str) -> String {
    let decimals = minor_units(&code.trim().to_ascii_uppercase());
    let divisor = 10_u64.pow(decimals);
    let amount = cents.unsigned_abs();
    let sign = if cents < 0 { "-" } else { "" };
    if decimals == 0 {
        return format!("{sign}{amount}");
    }
    let width = decimals as usize;
    format!("{sign}{}.{:0width$}", amount / divisor, amount % divisor)
}

/// The symbol written before the amount, for the most common currencies.
fn symbol(code: &str) -> Option<&'static str> {
    Some(match code {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "INR" => "₹",
        _ => return None,
    })
}

/// The number of decimals (ISO 4217 minor units) of `code`; 2 unless listed.
pub(crate) fn minor_units(code: &str) -> u32 {
    match code {
        "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF" | "UGX"
        | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        _ => 2,
    }
}

/// `1234567` as `1,234,567`, or as `12,34,567` (three digits, then pairs: lakhs and
/// crores) when `indian`.
fn group_digits(n: u64, indian: bool) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 2);
    for (i, digit) in digits.chars().enumerate() {
        let left = digits.len() - i;
        let comma = if indian {
            left == 3 || (left > 3 && (left - 3).is_multiple_of(2))
        } else {
            left.is_multiple_of(3)
        };
        if i > 0 && comma {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A number for display: integers without decimals, others with up to
/// two, thousands grouped.
pub(crate) fn format_number(n: f64) -> String {
    if !n.is_finite() {
        return String::new();
    }
    let negative = n < 0.0;
    let rounded = (n.abs() * 100.0).round() / 100.0;
    let whole = rounded.trunc() as u64;
    let fraction = ((rounded - rounded.trunc()) * 100.0).round() as u64;
    let mut out = String::new();
    if negative && (whole > 0 || fraction > 0) {
        out.push('-');
    }
    out.push_str(&group_digits(whole, false));
    if fraction > 0 {
        if fraction.is_multiple_of(10) {
            let _ = write!(out, ".{}", fraction / 10);
        } else {
            let _ = write!(out, ".{fraction:02}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_format_like_the_view_filter() {
        let views =
            navree_view::Views::from_templates([("money.txt", "{{ cents|money(currency) }}")])
                .unwrap();
        for (cents, code) in [
            (1_250_000, "USD"),
            (150_000, "JPY"),
            (1999, "CHF"),
            (-5, "EUR"),
            (123_456_789, "KWD"),
            (123_456_789, "INR"),
        ] {
            let expected = views
                .render(
                    "money",
                    serde_json::json!({ "cents": cents, "currency": code }),
                )
                .unwrap();
            assert_eq!(format(cents, code), expected, "{cents} {code}");
        }
        assert_eq!(format(1_250_000, "usd"), "$12,500.00");
        assert_eq!(format(123_456_789, "inr"), "₹12,34,567.89");
        assert_eq!(format(1_000_000_000, "INR"), "₹1,00,00,000.00");
    }

    #[test]
    fn decimal_input_becomes_minor_units() {
        assert_eq!(parse_major("12500", "USD"), Some(1_250_000));
        assert_eq!(parse_major("12,500.5", "USD"), Some(1_250_050));
        assert_eq!(parse_major("0.05", "USD"), Some(5));
        assert_eq!(parse_major("-1.25", "EUR"), Some(-125));
        assert_eq!(parse_major("150000", "JPY"), Some(150_000));
        assert_eq!(parse_major("1.5", "JPY"), None);
        assert_eq!(parse_major("1.234", "USD"), None);
        assert_eq!(parse_major("abc", "USD"), None);
        assert_eq!(parse_major("", "USD"), None);
        assert_eq!(to_major(1_250_050, "USD"), "12500.50");
        assert_eq!(to_major(150_000, "JPY"), "150000");
        assert_eq!(to_major(-125, "EUR"), "-1.25");
    }

    #[test]
    fn numbers_format_for_display() {
        assert_eq!(format_number(1234.0), "1,234");
        assert_eq!(format_number(1234.5), "1,234.5");
        assert_eq!(format_number(0.125), "0.13");
        assert_eq!(format_number(-3.0), "-3");
    }
}
