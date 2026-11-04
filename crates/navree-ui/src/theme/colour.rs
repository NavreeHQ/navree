//! Colours: one strict parser, one serialiser, the WCAG contrast ratio, and
//! mixing in OKLab for the colours a theme derives.
//!
//! A theme never carries colour text from outside: every value is parsed into
//! an [`Rgb`] here and written back out by [`Rgb::to_hex`], so the only CSS a
//! theme can produce is `#` and six hex digits.
//!
//! The OKLab conversion is Björn Ottosson's (public domain).

use std::fmt;

/// An sRGB colour, eight bits a channel. There is no alpha: contrast is
/// undefined for a colour you can see through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

/// Why a colour was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColourError {
    /// Nothing was given.
    Empty,
    /// It is longer than any colour this parser reads.
    TooLong,
    /// It is not `#rgb`, `#rrggbb` or `rgb(r, g, b)`.
    Format,
    /// A channel of `rgb()` is past 255.
    Range,
}

impl fmt::Display for ColourError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "Enter a colour.",
            Self::TooLong => "That is too long for a colour.",
            Self::Format => "Use a hex colour such as #1a73e8, or rgb(26, 115, 232).",
            Self::Range => "Each channel of rgb() runs from 0 to 255.",
        })
    }
}

impl std::error::Error for ColourError {}

/// The longest text [`Rgb::parse`] looks at: `rgb(255, 255, 255)` with room
/// for spaces.
const LONGEST: usize = 32;

impl Rgb {
    /// Black.
    pub const BLACK: Self = Self::hex(0x000000);
    /// White.
    pub const WHITE: Self = Self::hex(0xffffff);

    /// The colour `0xrrggbb`.
    pub const fn hex(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: (value & 0xff) as u8,
        }
    }

    /// The colour as `0xrrggbb`.
    pub const fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    /// Reads `#rgb`, `#rrggbb` (any case) or `rgb(r, g, b)` with whole numbers
    /// from 0 to 255, commas or spaces between them. Nothing else: no names, no
    /// alpha, no other functions, no escapes. Spaces around the colour are
    /// ignored.
    ///
    /// ```
    /// use navree_ui::theme::Rgb;
    ///
    /// assert_eq!(Rgb::parse("#1A73E8").unwrap().to_hex(), "#1a73e8");
    /// assert_eq!(Rgb::parse("#fa0").unwrap().to_hex(), "#ffaa00");
    /// assert_eq!(Rgb::parse("rgb(26 115 232)").unwrap().to_hex(), "#1a73e8");
    /// assert!(Rgb::parse("red").is_err());
    /// assert!(Rgb::parse("#fff;}body{display:none").is_err());
    /// ```
    pub fn parse(text: &str) -> Result<Self, ColourError> {
        if text.len() > LONGEST {
            return Err(ColourError::TooLong);
        }
        let text = text.trim_matches(|c: char| c.is_ascii_whitespace());
        if text.is_empty() {
            return Err(ColourError::Empty);
        }
        if let Some(hex) = text.strip_prefix('#') {
            return parse_hex(hex);
        }
        let lower = text.to_ascii_lowercase();
        let Some(inner) = lower
            .strip_prefix("rgb(")
            .and_then(|rest| rest.strip_suffix(')'))
        else {
            return Err(ColourError::Format);
        };
        let parts: Vec<&str> = if inner.contains(',') {
            inner.split(',').map(str::trim).collect()
        } else {
            inner.split_ascii_whitespace().collect()
        };
        let [r, g, b] = parts.as_slice() else {
            return Err(ColourError::Format);
        };
        let channel = |part: &str| -> Result<u8, ColourError> {
            if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ColourError::Format);
            }
            part.parse::<u16>()
                .ok()
                .and_then(|value| u8::try_from(value).ok())
                .ok_or(ColourError::Range)
        };
        Ok(Self {
            r: channel(r)?,
            g: channel(g)?,
            b: channel(b)?,
        })
    }

    /// `#rrggbb`, lower case: the only way a colour is written into CSS.
    pub fn to_hex(self) -> String {
        let mut out = String::with_capacity(7);
        self.write_hex(&mut out);
        out
    }

    /// Writes [`to_hex`](Self::to_hex) into `out`.
    pub fn write_hex(self, out: &mut String) {
        out.push('#');
        self.write_digits(out);
    }

    /// The six hex digits, lower case, with no `#`.
    pub(crate) fn write_digits(self, out: &mut String) {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        for byte in [self.r, self.g, self.b] {
            out.push(char::from(DIGITS[usize::from(byte >> 4)]));
            out.push(char::from(DIGITS[usize::from(byte & 15)]));
        }
    }

    /// The relative luminance, from 0 (black) to 1 (white), as WCAG 2 defines it.
    pub fn luminance(self) -> f64 {
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// The WCAG 2 contrast ratio with `other`, from 1 to 21.
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (light, dark) = if a > b { (a, b) } else { (b, a) };
        (light + 0.05) / (dark + 0.05)
    }

    /// The colour in OKLab: lightness from 0 to 1, then the two axes.
    pub fn to_oklab(self) -> [f64; 3] {
        let (r, g, b) = (linear(self.r), linear(self.g), linear(self.b));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        [
            0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
            1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
            0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
        ]
    }

    /// The colour at an OKLab point, or `None` when the point is outside sRGB.
    pub fn from_oklab([lightness, a, b]: [f64; 3]) -> Option<Self> {
        let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
        let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
        let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
        let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
        let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;
        const SLACK: f64 = 1e-4;
        if [r, g, b]
            .iter()
            .any(|c| !c.is_finite() || *c < -SLACK || *c > 1.0 + SLACK)
        {
            return None;
        }
        Some(Self {
            r: encode(r),
            g: encode(g),
            b: encode(b),
        })
    }

    /// The colour at lightness `l`, chroma `c` and hue `h` (degrees) in OKLCH,
    /// with the chroma lowered until it fits in sRGB.
    pub fn from_oklch(l: f64, c: f64, h: f64) -> Self {
        let l = l.clamp(0.0, 1.0);
        let (sin, cos) = h.to_radians().sin_cos();
        let mut low = 0.0_f64;
        let mut high = c.max(0.0);
        if let Some(colour) = Self::from_oklab([l, high * cos, high * sin]) {
            return colour;
        }
        for _ in 0..24 {
            let mid = (low + high) / 2.0;
            if Self::from_oklab([l, mid * cos, mid * sin]).is_some() {
                low = mid;
            } else {
                high = mid;
            }
        }
        Self::from_oklab([l, low * cos, low * sin]).unwrap_or_else(|| grey(l))
    }

    /// The colour in OKLCH: lightness, chroma and hue in degrees.
    pub fn to_oklch(self) -> [f64; 3] {
        let [l, a, b] = self.to_oklab();
        let hue = b.atan2(a).to_degrees();
        [l, a.hypot(b), if hue < 0.0 { hue + 360.0 } else { hue }]
    }

    /// `self` mixed with `other` in OKLab: `0.0` is `self`, `1.0` is `other`.
    pub fn mix(self, other: Self, amount: f64) -> Self {
        let amount = if amount.is_nan() {
            0.0
        } else {
            amount.clamp(0.0, 1.0)
        };
        let (a, b) = (self.to_oklab(), other.to_oklab());
        let point = [
            a[0] + (b[0] - a[0]) * amount,
            a[1] + (b[1] - a[1]) * amount,
            a[2] + (b[2] - a[2]) * amount,
        ];
        // A mix of two sRGB colours stays inside sRGB but for rounding.
        Self::from_oklab(point).unwrap_or_else(|| grey(point[0]))
    }

    /// `self` moved toward `toward` in OKLab, in steps of 2 %, until it reaches
    /// `ratio` against every colour of `on`. `toward` itself when no step does.
    pub fn until_contrast(self, on: &[Self], ratio: f64, toward: Self) -> Self {
        let passes = |colour: Self| on.iter().all(|back| colour.contrast(*back) >= ratio);
        (0..=50)
            .map(|step| self.mix(toward, f64::from(step) / 50.0))
            .find(|colour| passes(*colour))
            .unwrap_or(toward)
    }

    /// The text to put on a fill of this colour: white wherever white reaches
    /// 4.5:1, otherwise a deep shade of the fill's own hue, which never reads as
    /// plain black on a bright colour.
    pub fn ink(self) -> Self {
        if Self::WHITE.contrast(self) >= 4.5 {
            return Self::WHITE;
        }
        let [_, chroma, hue] = self.to_oklch();
        let deep = Self::from_oklch(0.2, chroma.min(0.06), hue);
        if deep.contrast(self) >= 4.5 {
            deep
        } else {
            Self::BLACK
        }
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// `rgb`, `rrggbb`, any case.
fn parse_hex(hex: &str) -> Result<Rgb, ColourError> {
    let digit = |byte: u8| -> Result<u8, ColourError> {
        match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            b'A'..=b'F' => Ok(byte - b'A' + 10),
            _ => Err(ColourError::Format),
        }
    };
    match hex.as_bytes() {
        [r, g, b] => {
            let (r, g, b) = (digit(*r)?, digit(*g)?, digit(*b)?);
            Ok(Rgb {
                r: r * 17,
                g: g * 17,
                b: b * 17,
            })
        }
        [r1, r2, g1, g2, b1, b2] => Ok(Rgb {
            r: digit(*r1)? * 16 + digit(*r2)?,
            g: digit(*g1)? * 16 + digit(*g2)?,
            b: digit(*b1)? * 16 + digit(*b2)?,
        }),
        _ => Err(ColourError::Format),
    }
}

/// A channel from sRGB to linear light.
fn linear(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// A channel from linear light to sRGB.
fn encode(linear: f64) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let value = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    // In 0..=255 after the clamp, so the cast keeps every value.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (value * 255.0).round().clamp(0.0, 255.0) as u8;
    byte
}

/// The grey at OKLab lightness `l`.
fn grey(l: f64) -> Rgb {
    let v = encode(l.clamp(0.0, 1.0).powi(3));
    Rgb { r: v, g: v, b: v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_in_both_lengths_and_any_case() {
        assert_eq!(Rgb::parse("#1a73e8"), Ok(Rgb::hex(0x1a73e8)));
        assert_eq!(Rgb::parse("#1A73E8"), Ok(Rgb::hex(0x1a73e8)));
        assert_eq!(Rgb::parse("  #fa0 "), Ok(Rgb::hex(0xffaa00)));
        assert_eq!(Rgb::parse("#000"), Ok(Rgb::BLACK));
    }

    #[test]
    fn rgb_with_commas_or_spaces() {
        assert_eq!(Rgb::parse("rgb(26, 115, 232)"), Ok(Rgb::hex(0x1a73e8)));
        assert_eq!(Rgb::parse("RGB(26 115 232)"), Ok(Rgb::hex(0x1a73e8)));
        assert_eq!(Rgb::parse("rgb(0,0,0)"), Ok(Rgb::BLACK));
        assert_eq!(Rgb::parse("rgb(256, 0, 0)"), Err(ColourError::Range));
        assert_eq!(Rgb::parse("rgb(1, 2)"), Err(ColourError::Format));
        assert_eq!(Rgb::parse("rgb(1, 2, 3, 4)"), Err(ColourError::Format));
        assert_eq!(Rgb::parse("rgb(-1, 2, 3)"), Err(ColourError::Format));
        assert_eq!(Rgb::parse("rgb(1.5, 2, 3)"), Err(ColourError::Format));
        assert_eq!(Rgb::parse("rgb(1 2 3 / .5)"), Err(ColourError::Format));
    }

    #[test]
    fn nothing_else_is_a_colour() {
        for bad in [
            "",
            "  ",
            "red",
            "#",
            "#ff",
            "#ffff",
            "#fffff",
            "#fffffff",
            "#ffffffff",
            "#gggggg",
            "#ff00zz",
            "fff",
            "#fff;}",
            "#fff;}body{display:none}",
            "#fff\0",
            "#ﬀﬀﬀ",
            "#１２３",
            "rgb(1,2,3);x:y",
            "rgba(1,2,3,1)",
            "hsl(1 2% 3%)",
            "oklch(0.5 0.1 120)",
            "var(--accent)",
            "url(javascript:alert(1))",
            "expression(alert(1))",
            "\\23 fff",
            "#f\\66 f",
            "</style><script>alert(1)</script>",
            "rgb(1,2,3)\n}",
        ] {
            assert!(Rgb::parse(bad).is_err(), "{bad:?}");
        }
        assert_eq!(Rgb::parse(&"#".repeat(40)), Err(ColourError::TooLong));
    }

    #[test]
    fn written_back_as_six_lower_case_digits() {
        assert_eq!(Rgb::hex(0x0a0b0c).to_hex(), "#0a0b0c");
        assert_eq!(Rgb::WHITE.to_hex(), "#ffffff");
        for value in [0, 1, 0xabcdef, 0xffffff, 0x123456] {
            let colour = Rgb::hex(value);
            assert_eq!(Rgb::parse(&colour.to_hex()), Ok(colour));
            assert_eq!(colour.to_u32(), value);
        }
    }

    #[test]
    fn contrast_is_the_wcag_formula() {
        assert!((Rgb::BLACK.contrast(Rgb::WHITE) - 21.0).abs() < 1e-9);
        assert!((Rgb::WHITE.contrast(Rgb::WHITE) - 1.0).abs() < 1e-9);
        // #767676 on white is the classic 4.54:1.
        let ratio = Rgb::hex(0x767676).contrast(Rgb::WHITE);
        assert!((ratio - 4.54).abs() < 0.01, "{ratio}");
    }

    #[test]
    fn oklab_round_trips_every_grey_and_some_colours() {
        for v in 0..=255u8 {
            let grey = Rgb { r: v, g: v, b: v };
            assert_eq!(Rgb::from_oklab(grey.to_oklab()), Some(grey), "{grey}");
        }
        for value in [0x3dff8f, 0x0b7a3e, 0x1a73e8, 0xff6b6b, 0x8a5a00, 0x6b46c1] {
            let colour = Rgb::hex(value);
            assert_eq!(Rgb::from_oklab(colour.to_oklab()), Some(colour), "{colour}");
            let [l, c, h] = colour.to_oklch();
            assert_eq!(Rgb::from_oklch(l, c, h), colour, "{colour}");
        }
        // Far outside sRGB: the chroma comes down until it fits.
        let vivid = Rgb::from_oklch(0.7, 0.5, 150.0);
        assert!(vivid.to_oklch()[1] < 0.5);
    }

    #[test]
    fn mixing_runs_from_one_colour_to_the_other() {
        let (a, b) = (Rgb::hex(0x3dff8f), Rgb::BLACK);
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
        assert!(a.mix(b, 0.5).luminance() < a.luminance());
        assert_eq!(a.mix(b, -3.0), a);
        assert_eq!(a.mix(b, 9.0), b);
        assert_eq!(a.mix(b, f64::NAN), a);
    }

    #[test]
    fn until_contrast_stops_at_the_first_step_that_reaches_it() {
        let white = [Rgb::WHITE];
        // Already there: unchanged.
        assert_eq!(
            Rgb::hex(0x0b7a3e).until_contrast(&white, 4.5, Rgb::BLACK),
            Rgb::hex(0x0b7a3e)
        );
        let fixed = Rgb::hex(0xffd84d).until_contrast(&white, 4.5, Rgb::BLACK);
        assert!(fixed.contrast(Rgb::WHITE) >= 4.5, "{fixed}");
        assert!(
            fixed.contrast(Rgb::WHITE) < 5.5,
            "no darker than it needs: {fixed}"
        );
    }

    #[test]
    fn ink_is_white_where_white_reads_and_a_deep_shade_elsewhere() {
        assert_eq!(Rgb::hex(0x0b7a3e).ink(), Rgb::WHITE);
        let ink = Rgb::hex(0x3dff8f).ink();
        assert_ne!(ink, Rgb::BLACK);
        assert!(ink.contrast(Rgb::hex(0x3dff8f)) >= 4.5, "{ink}");
        // Every colour gets an ink that reads.
        for value in (0..0x0100_0000).step_by(0x3011) {
            let fill = Rgb::hex(value);
            assert!(fill.ink().contrast(fill) >= 4.5, "{fill}");
        }
    }
}
