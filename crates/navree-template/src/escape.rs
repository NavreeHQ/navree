//! Printing values: HTML escaping and JSON.

use std::fmt::{self, Write as _};

use crate::environment::AutoEscape;
use crate::error::{Error, ErrorKind};
use crate::value::{Repr, Value};
use crate::vm::Output;

/// Writes `value` the way `mode` prints it. A safe string prints as it is
/// in every mode.
pub(crate) fn write_escaped(
    out: &mut Output,
    mode: AutoEscape,
    value: &Value,
) -> Result<(), Error> {
    if let Repr::Str(text, true) = &value.0 {
        return out.write_str(text);
    }
    match mode {
        AutoEscape::None => write_display(out, value),
        AutoEscape::Html => match &value.0 {
            Repr::Str(text, _) => write_html(out, text),
            Repr::Undefined(_) | Repr::None | Repr::Bool(_) | Repr::Int(_) | Repr::Float(_) => {
                write_display(out, value)
            }
            _ => {
                let mut escaping = HtmlWriter { out };
                write!(escaping, "{value}").map_err(|_| too_large())
            }
        },
        AutoEscape::Json => {
            let text = to_json(value, JsonStyle::Compact, false, out_limit(out))?;
            out.write_str(&text)
        }
    }
}

fn out_limit(out: &Output) -> usize {
    out.limit()
}

fn too_large() -> Error {
    Error::limit("the output is too large")
}

fn write_display(out: &mut Output, value: &Value) -> Result<(), Error> {
    if let Repr::Str(text, _) = &value.0 {
        return out.write_str(text);
    }
    write!(out, "{value}").map_err(|_| too_large())
}

/// Writes text with `&`, `<`, `>`, `"`, `'` and `/` as HTML entities.
fn write_html(out: &mut Output, text: &str) -> Result<(), Error> {
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        let entity = match byte {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&#x27;",
            b'/' => "&#x2f;",
            _ => continue,
        };
        out.write_str(text.get(start..index).unwrap_or(""))?;
        out.write_str(entity)?;
        start = index + 1;
    }
    out.write_str(text.get(start..).unwrap_or(""))
}

/// Escapes what is printed through it.
struct HtmlWriter<'a> {
    out: &'a mut Output,
}

impl fmt::Write for HtmlWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        write_html(self.out, text).map_err(|_| fmt::Error)
    }
}

/// How JSON is laid out.
#[derive(Clone, Copy)]
pub(crate) enum JsonStyle {
    /// No whitespace: `[1,2]`.
    Compact,
    /// A space after separators: `[1, 2]`.
    Spaced,
    /// One item per line, indented by this many spaces.
    Pretty(usize),
}

/// `value` as JSON, at most `max_bytes` long. With `html_safe`, `<`, `>`,
/// `&` and `'` in strings are escaped too, so the output is safe inside
/// HTML (script tags and single-quoted attributes included).
pub(crate) fn to_json(
    value: &Value,
    style: JsonStyle,
    html_safe: bool,
    max_bytes: usize,
) -> Result<String, Error> {
    let mut writer = JsonWriter {
        out: String::new(),
        style,
        html_safe,
        depth: 0,
        max_bytes,
    };
    writer.write_value(value)?;
    Ok(writer.out)
}

struct JsonWriter {
    out: String,
    style: JsonStyle,
    html_safe: bool,
    depth: usize,
    max_bytes: usize,
}

impl JsonWriter {
    fn check_size(&self) -> Result<(), Error> {
        if self.out.len() > self.max_bytes {
            return Err(Error::limit("a string is too large"));
        }
        Ok(())
    }

    fn write_value(&mut self, value: &Value) -> Result<(), Error> {
        self.check_size()?;
        match &value.0 {
            Repr::None | Repr::Undefined(_) => self.out.push_str("null"),
            Repr::Bool(flag) => self.out.push_str(if *flag { "true" } else { "false" }),
            Repr::Int(integer) => {
                let _ = write!(self.out, "{integer}");
            }
            Repr::Float(float) => {
                if float.is_finite() {
                    write_finite_f64(&mut self.out, *float);
                } else {
                    self.out.push_str("null");
                }
            }
            Repr::Str(text, _) => self.write_str(text),
            Repr::Seq(items, ..) => {
                self.begin('[');
                for (index, item) in items.iter().enumerate() {
                    self.item(index == 0);
                    self.write_value(item)?;
                }
                self.end(']', items.is_empty());
            }
            Repr::Map(..) | Repr::Kwargs(_) | Repr::Module(_) => {
                let pairs = value.map_pairs().unwrap_or_default();
                self.begin('{');
                for (index, (key, item)) in pairs.iter().enumerate() {
                    self.item(index == 0);
                    self.write_key(key)?;
                    self.out.push_str(match self.style {
                        JsonStyle::Compact => ":",
                        JsonStyle::Spaced | JsonStyle::Pretty(_) => ": ",
                    });
                    self.write_value(item)?;
                }
                self.end('}', pairs.is_empty());
            }
            Repr::Function(_) | Repr::Macro(_) | Repr::Loop(_) => {
                let text = value.to_string_capped(self.max_bytes)?;
                self.write_str(&text);
            }
        }
        Ok(())
    }

    /// Keys must be strings: numbers and booleans become strings.
    fn write_key(&mut self, key: &Value) -> Result<(), Error> {
        match &key.0 {
            Repr::Str(text, _) => self.write_str(text),
            Repr::Function(_) | Repr::Macro(_) | Repr::Loop(_) => {
                let text = key.to_string_capped(self.max_bytes)?;
                self.write_str(&text);
            }
            Repr::Bool(_) | Repr::Int(_) => {
                self.out.push('"');
                self.write_value(key)?;
                self.out.push('"');
            }
            Repr::Float(float) if float.is_finite() => {
                self.out.push('"');
                write_finite_f64(&mut self.out, *float);
                self.out.push('"');
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::BadSerialization,
                    format!("cannot use value of type {} as JSON key", key.kind()),
                ));
            }
        }
        Ok(())
    }

    fn begin(&mut self, open: char) {
        self.depth += 1;
        self.out.push(open);
    }

    fn item(&mut self, first: bool) {
        match self.style {
            JsonStyle::Compact => {
                if !first {
                    self.out.push(',');
                }
            }
            JsonStyle::Spaced => {
                if !first {
                    self.out.push_str(", ");
                }
            }
            JsonStyle::Pretty(indent) => {
                self.out.push_str(if first { "\n" } else { ",\n" });
                self.indent(indent);
            }
        }
    }

    fn end(&mut self, close: char, empty: bool) {
        self.depth -= 1;
        if let JsonStyle::Pretty(indent) = self.style
            && !empty
        {
            self.out.push('\n');
            self.indent(indent);
        }
        self.out.push(close);
    }

    fn indent(&mut self, indent: usize) {
        for _ in 0..indent.saturating_mul(self.depth).min(self.max_bytes) {
            self.out.push(' ');
        }
    }

    fn write_str(&mut self, text: &str) {
        self.out.reserve(text.len().min(self.max_bytes) + 2);
        self.out.push('"');
        let mut start = 0;
        for (index, byte) in text.bytes().enumerate() {
            let needs_escape = byte < 0x20
                || byte == b'"'
                || byte == b'\\'
                || (self.html_safe && matches!(byte, b'<' | b'>' | b'&' | b'\''));
            if !needs_escape {
                continue;
            }
            self.out.push_str(text.get(start..index).unwrap_or(""));
            match byte {
                b'"' => self.out.push_str("\\\""),
                b'\\' => self.out.push_str("\\\\"),
                0x08 => self.out.push_str("\\b"),
                0x0c => self.out.push_str("\\f"),
                b'\n' => self.out.push_str("\\n"),
                b'\r' => self.out.push_str("\\r"),
                b'\t' => self.out.push_str("\\t"),
                _ => {
                    let _ = write!(self.out, "\\u{byte:04x}");
                }
            }
            start = index + 1;
        }
        self.out.push_str(text.get(start..).unwrap_or(""));
        self.out.push('"');
    }
}

/// Writes a finite float with the shortest digits that read back as the
/// value: `1.0`, `0.001`, `1e+16`, `1.5e-7`.
fn write_finite_f64(out: &mut String, value: f64) {
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mantissa.bytes().filter(|&b| b != b'.').collect();
    let len = digits.len();
    // A value exactly between two shortest candidates takes the even digit,
    // when that one reads back as the value too.
    if let Some(last) = digits.last().copied()
        && (last - b'0') % 2 == 1
        && is_tie(value.abs(), &digits, exponent)
    {
        let mut even = digits.clone();
        if let Some(digit) = even.last_mut() {
            *digit = last - 1;
        }
        let text = format!(
            "{}e{}",
            String::from_utf8_lossy(&even),
            exponent - (len as i32 - 1)
        );
        if text.parse::<f64>().ok() == Some(value.abs()) {
            digits = even;
        }
    }
    let digits = String::from_utf8_lossy(&digits).into_owned();
    let len = len as i32;
    // The value is digits * 10^k, and 10^(kk - 1) <= value < 10^kk.
    let kk = exponent + 1;
    let k = kk - len;
    if value.is_sign_negative() {
        out.push('-');
    }
    let split = |at: i32| usize::try_from(at).unwrap_or(0);
    if 0 <= k && kk <= 16 {
        out.push_str(&digits);
        for _ in 0..k {
            out.push('0');
        }
        out.push_str(".0");
    } else if 0 < kk && kk <= 16 {
        let (whole, fraction) = digits.split_at(split(kk).min(digits.len()));
        out.push_str(whole);
        out.push('.');
        out.push_str(fraction);
    } else if -5 < kk && kk <= 0 {
        out.push_str("0.");
        for _ in kk..0 {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1.min(digits.len()));
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push('e');
        if kk > 1 {
            out.push('+');
        }
        let _ = write!(out, "{}", kk - 1);
    }
}

/// Whether `value` lies exactly between the digits and the digits with the
/// last one decremented. With `d` for the `n` digits and `q = exp - n`, the
/// middle is `(2d - 1) * 5^(q + 1) * 2^q`, and the value is `m * 2^e` with
/// an odd `m`: they are equal if the powers of two and the odd parts are.
fn is_tie(value: f64, digits: &[u8], exponent: i32) -> bool {
    let bits = value.to_bits();
    let biased = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let fraction = bits & ((1 << 52) - 1);
    let (mut m, mut e) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), biased - 1075)
    };
    if m == 0 {
        return false;
    }
    let zeros = m.trailing_zeros();
    m >>= zeros;
    e += i32::try_from(zeros).unwrap_or(0);
    let q = exponent - i32::try_from(digits.len()).unwrap_or(0);
    if e != q {
        return false;
    }
    let Some(number) = std::str::from_utf8(digits)
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
    else {
        return false;
    };
    let odd = u128::from(number) * 2 - 1;
    let m = u128::from(m);
    let p = q + 1;
    let (small, large, power) = if p >= 0 {
        (m, odd, p.unsigned_abs())
    } else {
        (odd, m, p.unsigned_abs())
    };
    5_u128
        .checked_pow(power)
        .and_then(|factor| factor.checked_mul(large))
        == Some(small)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(value: Value, html_safe: bool) -> String {
        to_json(&value, JsonStyle::Compact, html_safe, 1 << 20).expect("json")
    }

    #[test]
    fn floats_print_shortest() {
        for (value, expected) in [
            (1.0, "1.0"),
            (0.001, "0.001"),
            (1e16, "1e+16"),
            (1.5e-7, "1.5e-7"),
            (-2.5, "-2.5"),
            (0.0, "0.0"),
            (123_456.789, "123456.789"),
            (1e15, "1000000000000000.0"),
        ] {
            assert_eq!(json(Value::from(value), false), expected);
        }
        assert_eq!(json(Value::from(f64::NAN), false), "null");
    }

    #[test]
    fn strings_escape() {
        assert_eq!(
            json(Value::from("a\"b\\c\n\t\u{1}/<>&'"), false),
            "\"a\\\"b\\\\c\\n\\t\\u0001/<>&'\""
        );
        assert_eq!(
            json(Value::from("<>&'"), true),
            "\"\\u003c\\u003e\\u0026\\u0027\""
        );
    }

    #[test]
    fn html_entities() {
        let mut out = Output::for_tests();
        write_html(&mut out, "<a href=\"x\">'&'/</a>").expect("write");
        assert_eq!(
            out.buf,
            "&lt;a href=&quot;x&quot;&gt;&#x27;&amp;&#x27;&#x2f;&lt;&#x2f;a&gt;"
        );
    }
}
