//! Printing values: the modes, HTML escaping and JSON.

use std::fmt::{self, Write as _};

use crate::environment::AutoEscape;
use crate::error::{Error, ErrorKind};
use crate::output::Output;
use crate::syntax::ast::EscapeTag;
use crate::value::{Repr, Safety, Value};

/// How a value prints at some point of a render. Modes combine by asking for
/// both, so the mode at a point can only be as strict as what surrounds it or
/// stricter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// As it is.
    None,
    /// With HTML's special characters as entities.
    Html,
    /// As JSON.
    Json,
    /// As JSON that is also fit for HTML: `<`, `>`, `&` and `'` in strings are
    /// `\u` escapes, and quotes suit where the text lands.
    Both,
}

impl From<AutoEscape> for Mode {
    fn from(mode: AutoEscape) -> Mode {
        match mode {
            AutoEscape::None => Mode::None,
            AutoEscape::Html => Mode::Html,
            AutoEscape::Json => Mode::Json,
        }
    }
}

impl Mode {
    /// The mode that is at least as strict as both.
    pub(crate) fn and(self, other: Mode) -> Mode {
        match (self, other) {
            (Mode::None, mode) | (mode, Mode::None) => mode,
            (Mode::Html, Mode::Html) => Mode::Html,
            (Mode::Json, Mode::Json) => Mode::Json,
            _ => Mode::Both,
        }
    }

    pub(crate) fn has_html(self) -> bool {
        matches!(self, Mode::Html | Mode::Both)
    }

    /// The mode of text inside an `{% autoescape %}` tag in a template whose
    /// own mode is `self`. In an HTML template the JSON stays fit for HTML.
    pub(crate) fn under(self, tag: EscapeTag) -> Mode {
        match tag {
            EscapeTag::Inherit => self,
            EscapeTag::On if self == Mode::None => Mode::Html,
            EscapeTag::On => self,
            EscapeTag::Off => Mode::None,
            EscapeTag::Html => Mode::Html,
            EscapeTag::Json if self.has_html() => Mode::Both,
            EscapeTag::Json => Mode::Json,
        }
    }

    /// What text captured in this mode is safe for.
    pub(crate) fn safety(self) -> Safety {
        match self {
            Mode::None => Safety::Plain,
            Mode::Html => Safety::Html,
            Mode::Json => Safety::Json,
            Mode::Both => Safety::Both,
        }
    }
}

impl Safety {
    /// Whether a string with this mark needs no more escaping in `mode`. Only
    /// the mode it was made in counts: JSON printed into HTML escapes again.
    pub(crate) fn fits(self, mode: Mode) -> bool {
        match self {
            Safety::Plain => mode == Mode::None,
            Safety::Trusted | Safety::Tojson => true,
            Safety::Html => matches!(mode, Mode::None | Mode::Html),
            Safety::Json => matches!(mode, Mode::None | Mode::Json),
            Safety::Both => matches!(mode, Mode::None | Mode::Both),
        }
    }
}

/// Writes `value` the way `mode` prints it, for where `out` is now.
pub(crate) fn write_escaped(out: &mut Output, mode: Mode, value: &Value) -> Result<(), Error> {
    if let Repr::Str(text, safety) = &value.0 {
        if *safety == Safety::Tojson && mode.has_html() {
            return write_tojson(out, mode, text);
        }
        if safety.fits(mode) {
            return out.write_str(text);
        }
    }
    match mode {
        Mode::None => write_display(out, value),
        Mode::Html => match &value.0 {
            Repr::Str(text, _) => write_html(out, text),
            Repr::Undefined(_) | Repr::None | Repr::Bool(_) | Repr::Int(_) | Repr::Float(_) => {
                write_display(out, value)
            }
            _ => {
                let mut escaping = HtmlWriter { out };
                write!(escaping, "{value}").map_err(|_| escaping.out.take_failure())
            }
        },
        Mode::Json => write_json(out, value, JsonStyle::Compact, false, Quote::Raw),
        Mode::Both => {
            let quote = quote_for(out, mode);
            write_json(out, value, JsonStyle::Compact, true, quote)
        }
    }
}

/// A JSON string's quotes, for where `out` is now: a `<script>`'s content
/// takes them as they are and entities mean nothing there; anywhere else in
/// HTML, attributes included, an entity reads back as a quote and a quote may
/// end the attribute.
fn quote_for(out: &Output, mode: Mode) -> Quote {
    if out.in_script() || (mode == Mode::Both && !out.is_html()) {
        Quote::Raw
    } else {
        Quote::Entity
    }
}

/// Writes `|tojson` output into HTML.
fn write_tojson(out: &mut Output, mode: Mode, text: &str) -> Result<(), Error> {
    let quote = quote_for(out, mode).as_str();
    let mut parts = text.split('"');
    if let Some(first) = parts.next() {
        out.write_str(first)?;
    }
    for part in parts {
        out.write_str(quote)?;
        out.write_str(part)?;
    }
    Ok(())
}

/// Writes a value as it prints, for a string or anything else.
pub(crate) fn write_display(out: &mut Output, value: &Value) -> Result<(), Error> {
    if let Repr::Str(text, _) = &value.0 {
        return out.write_str(text);
    }
    write!(out, "{value}").map_err(|_| out.take_failure())
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
        write_html(self.out, text).map_err(|error| {
            self.out.set_failure(error);
            fmt::Error
        })
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

/// How JSON's quotes are written.
#[derive(Clone, Copy)]
pub(crate) enum Quote {
    /// `"`.
    Raw,
    /// `&quot;`, for HTML outside a script.
    Entity,
}

impl Quote {
    fn as_str(self) -> &'static str {
        match self {
            Quote::Raw => "\"",
            Quote::Entity => "&quot;",
        }
    }
}

/// Writes `value` as JSON to `out`, within what `out` may hold. With
/// `html_safe`, `<`, `>`, `&` and `'` in strings are `\u` escapes.
pub(crate) fn write_json(
    out: &mut Output,
    value: &Value,
    style: JsonStyle,
    html_safe: bool,
    quote: Quote,
) -> Result<(), Error> {
    JsonWriter {
        out,
        style,
        html_safe,
        quote: quote.as_str(),
        depth: 0,
    }
    .write_value(value)
}

struct JsonWriter<'a> {
    out: &'a mut Output,
    style: JsonStyle,
    html_safe: bool,
    quote: &'static str,
    depth: usize,
}

impl JsonWriter<'_> {
    fn write_value(&mut self, value: &Value) -> Result<(), Error> {
        match &value.0 {
            Repr::None | Repr::Undefined(_) => self.out.write_str("null")?,
            Repr::Bool(flag) => self.out.write_str(if *flag { "true" } else { "false" })?,
            Repr::Int(integer) => self.out.write_str(&integer.to_string())?,
            Repr::Float(float) => self.write_float(*float)?,
            Repr::Str(text, _) => self.write_str(text)?,
            Repr::Seq(items, ..) => {
                self.begin("[")?;
                for (index, item) in items.iter().enumerate() {
                    self.item(index == 0)?;
                    self.write_value(item)?;
                }
                self.end("]", items.is_empty())?;
            }
            Repr::Map(..) | Repr::Kwargs(..) | Repr::Module(_) => {
                let Some(map) = value.as_map() else {
                    return Ok(());
                };
                self.begin("{")?;
                for (index, (key, item)) in map.iter().enumerate() {
                    self.item(index == 0)?;
                    self.write_key(key)?;
                    self.out.write_str(match self.style {
                        JsonStyle::Compact => ":",
                        JsonStyle::Spaced | JsonStyle::Pretty(_) => ": ",
                    })?;
                    self.write_value(item)?;
                }
                self.end("}", map.is_empty())?;
            }
            Repr::Function(_) | Repr::Macro(_) | Repr::Loop(_) => {
                let text = value.to_string_capped(self.out.limit())?;
                self.write_str(&text)?;
            }
        }
        Ok(())
    }

    fn write_float(&mut self, float: f64) -> Result<(), Error> {
        if !float.is_finite() {
            return self.out.write_str("null");
        }
        let mut text = String::new();
        write_finite_f64(&mut text, float);
        self.out.write_str(&text)
    }

    /// Keys must be strings: numbers and booleans become strings.
    fn write_key(&mut self, key: &Value) -> Result<(), Error> {
        match &key.0 {
            Repr::Str(text, _) => self.write_str(text),
            Repr::Function(_) | Repr::Macro(_) | Repr::Loop(_) => {
                let text = key.to_string_capped(self.out.limit())?;
                self.write_str(&text)
            }
            Repr::Bool(_) | Repr::Int(_) => {
                self.out.write_str(self.quote)?;
                self.write_value(key)?;
                self.out.write_str(self.quote)
            }
            Repr::Float(float) if float.is_finite() => {
                self.out.write_str(self.quote)?;
                self.write_float(*float)?;
                self.out.write_str(self.quote)
            }
            _ => Err(Error::new(
                ErrorKind::BadSerialization,
                format!("cannot use value of type {} as JSON key", key.kind()),
            )),
        }
    }

    fn begin(&mut self, open: &str) -> Result<(), Error> {
        self.depth += 1;
        self.out.write_str(open)
    }

    fn item(&mut self, first: bool) -> Result<(), Error> {
        match self.style {
            JsonStyle::Compact => {
                if !first {
                    self.out.write_str(",")?;
                }
            }
            JsonStyle::Spaced => {
                if !first {
                    self.out.write_str(", ")?;
                }
            }
            JsonStyle::Pretty(indent) => {
                self.out.write_str(if first { "\n" } else { ",\n" })?;
                self.indent(indent)?;
            }
        }
        Ok(())
    }

    fn end(&mut self, close: &str, empty: bool) -> Result<(), Error> {
        self.depth -= 1;
        if let JsonStyle::Pretty(indent) = self.style
            && !empty
        {
            self.out.write_str("\n")?;
            self.indent(indent)?;
        }
        self.out.write_str(close)
    }

    fn indent(&mut self, indent: usize) -> Result<(), Error> {
        const SPACES: &str = "                                                                ";
        let mut left = indent.saturating_mul(self.depth);
        while left > 0 {
            let width = left.min(SPACES.len());
            self.out.write_str(SPACES.get(..width).unwrap_or(""))?;
            left -= width;
        }
        Ok(())
    }

    fn write_str(&mut self, text: &str) -> Result<(), Error> {
        self.out.write_str(self.quote)?;
        let mut start = 0;
        for (index, byte) in text.bytes().enumerate() {
            let needs_escape = byte < 0x20
                || byte == b'"'
                || byte == b'\\'
                || (self.html_safe && matches!(byte, b'<' | b'>' | b'&' | b'\''));
            if !needs_escape {
                continue;
            }
            self.out.write_str(text.get(start..index).unwrap_or(""))?;
            match byte {
                b'"' => {
                    self.out.write_str("\\")?;
                    self.out.write_str(self.quote)?;
                }
                b'\\' => self.out.write_str("\\\\")?,
                0x08 => self.out.write_str("\\b")?,
                0x0c => self.out.write_str("\\f")?,
                b'\n' => self.out.write_str("\\n")?,
                b'\r' => self.out.write_str("\\r")?,
                b'\t' => self.out.write_str("\\t")?,
                _ => self.out.write_str(&format!("\\u{byte:04x}"))?,
            }
            start = index + 1;
        }
        self.out.write_str(text.get(start..).unwrap_or(""))?;
        self.out.write_str(self.quote)
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
        let mut out = Output::for_tests();
        write_json(&mut out, &value, JsonStyle::Compact, html_safe, Quote::Raw).expect("json");
        out.into_string()
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
