//! CSV for a table's export: RFC 4180 quoting, and no cell a spreadsheet reads as
//! a formula.

/// Writes one field. A value starting with `=`, `+`, `-`, `@`, a tab or a carriage
/// return gets a leading apostrophe, so a spreadsheet shows it as text rather than
/// running it; a plain negative number stays a number.
pub fn write_csv_field(out: &mut String, value: &str) {
    let dangerous = value.starts_with(['=', '+', '@', '\t', '\r']) || (value.starts_with('-') && value.parse::<f64>().is_err());
    let quote = dangerous || value.contains([',', '"', '\n', '\r']);
    if quote {
        out.push('"');
    }
    if dangerous {
        out.push('\'');
    }
    if quote {
        for part in value.split_inclusive('"') {
            out.push_str(part);
            if part.ends_with('"') {
                out.push('"');
            }
        }
        out.push('"');
    } else {
        out.push_str(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(value: &str) -> String {
        let mut out = String::new();
        write_csv_field(&mut out, value);
        out
    }

    #[test]
    fn plain_values_stay_plain() {
        assert_eq!(field("Acme"), "Acme");
        assert_eq!(field("-12.5"), "-12.5");
        assert_eq!(field(""), "");
    }

    #[test]
    fn separators_and_quotes_are_quoted() {
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("two\nlines"), "\"two\nlines\"");
    }

    #[test]
    fn a_formula_is_never_a_formula() {
        for bad in ["=1+1", "+1+1", "@SUM(A1)", "-2+3", "=HYPERLINK(\"http://x\")", "\tx", "\rx"] {
            let out = field(bad);
            assert!(out.starts_with("\"'"), "{bad}: {out}");
        }
    }
}
