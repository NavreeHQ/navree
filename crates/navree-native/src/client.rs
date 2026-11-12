//! [`Client`]: what a native client says about itself on every request, and the
//! parser of the header that carries it.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The system a client runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Platform {
    /// iOS, on a phone.
    Ios,
    /// iPadOS.
    Ipados,
    /// macOS.
    Macos,
    /// watchOS.
    Watchos,
    /// tvOS.
    Tvos,
    /// visionOS.
    Visionos,
    /// Android, on a phone, a tablet or a foldable.
    Android,
    /// Wear OS.
    Wearos,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

impl Platform {
    /// Every platform, in the order the protocol lists them.
    pub const ALL: [Self; 10] = [
        Self::Ios,
        Self::Ipados,
        Self::Macos,
        Self::Watchos,
        Self::Tvos,
        Self::Visionos,
        Self::Android,
        Self::Wearos,
        Self::Windows,
        Self::Linux,
    ];

    /// The platform as the header names it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Ipados => "ipados",
            Self::Macos => "macos",
            Self::Watchos => "watchos",
            Self::Tvos => "tvos",
            Self::Visionos => "visionos",
            Self::Android => "android",
            Self::Wearos => "wearos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }

    /// The platform named `name`, or nothing.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|platform| platform.as_str() == name)
    }

    /// Whether this is one of Apple's systems.
    pub const fn is_apple(self) -> bool {
        matches!(
            self,
            Self::Ios | Self::Ipados | Self::Macos | Self::Watchos | Self::Tvos | Self::Visionos
        )
    }
}

/// The kind of device a client draws on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum FormFactor {
    /// A phone: one narrow column.
    Phone,
    /// A tablet or an unfolded foldable.
    Tablet,
    /// A window on a desktop.
    Desktop,
    /// A watch.
    Watch,
    /// A television.
    Tv,
    /// A spatial computer.
    Spatial,
    /// A car's display.
    Car,
}

impl FormFactor {
    /// Every form factor, in the order the protocol lists them.
    pub const ALL: [Self; 7] = [
        Self::Phone,
        Self::Tablet,
        Self::Desktop,
        Self::Watch,
        Self::Tv,
        Self::Spatial,
        Self::Car,
    ];

    /// The form factor as the header names it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Tablet => "tablet",
            Self::Desktop => "desktop",
            Self::Watch => "watch",
            Self::Tv => "tv",
            Self::Spatial => "spatial",
            Self::Car => "car",
        }
    }

    /// The form factor named `name`, or nothing.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|form| form.as_str() == name)
    }

    /// Whether a screen of this kind has room for one column only.
    pub const fn is_compact(self) -> bool {
        matches!(self, Self::Phone | Self::Watch)
    }
}

/// Light or dark, as the user's system is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    /// Light.
    #[default]
    Light,
    /// Dark.
    Dark,
}

/// How much contrast the user asked their system for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Contrast {
    /// The system's usual contrast.
    #[default]
    Normal,
    /// More contrast.
    More,
}

/// Why a client descriptor was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientError {
    /// The header is longer than [`Client::HEADER_MAX`] bytes.
    TooLong,
    /// A pair is not `key=value`, or holds a character a token can't.
    Malformed(String),
    /// `platform` is missing.
    NoPlatform,
    /// `form` is missing.
    NoForm,
    /// `platform` names a system this build of the protocol does not know.
    UnknownPlatform(String),
    /// `form` names a form factor this build of the protocol does not know.
    UnknownForm(String),
    /// A value that must be a number is not one.
    NotANumber(&'static str),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(f, "the client descriptor is too long"),
            Self::Malformed(pair) => write!(f, "`{pair}` is not `key=value` of tokens"),
            Self::NoPlatform => write!(f, "the client descriptor names no platform"),
            Self::NoForm => write!(f, "the client descriptor names no form factor"),
            Self::UnknownPlatform(name) => write!(f, "`{name}` is not a platform"),
            Self::UnknownForm(name) => write!(f, "`{name}` is not a form factor"),
            Self::NotANumber(key) => write!(f, "`{key}` is not a number"),
        }
    }
}

impl std::error::Error for ClientError {}

/// What a native client says about itself, on every request, in the
/// [`CLIENT_HEADER`](crate::CLIENT_HEADER) header.
///
/// The header is `key=value` pairs separated by semicolons. Values are tokens:
/// ASCII letters and digits and `. - _ / : +`. A key the parser does not know is
/// passed over, so a newer client can say more to an older server.
///
/// ```
/// use navree_native::{Client, FormFactor, Platform, Scheme};
///
/// let client = Client::parse(
///     "platform=android; os=16; form=phone; app=1.4.0; vocab=1; scheme=dark; \
///      text=1.3; locale=en-IN; tz=Asia/Kolkata; caps=biometrics+push",
/// )
/// .unwrap();
/// assert_eq!(client.platform, Platform::Android);
/// assert_eq!(client.form, FormFactor::Phone);
/// assert_eq!(client.scheme, Scheme::Dark);
/// assert_eq!(client.text_percent, 130);
/// assert!(client.supports("push"));
/// assert!(!client.supports("camera"));
/// assert_eq!(Client::parse(&client.to_header()).unwrap(), client);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Client {
    /// The system.
    pub platform: Platform,
    /// The system's version, such as `26.1`. Empty when the client did not say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os: String,
    /// The kind of device.
    pub form: FormFactor,
    /// The app's version, such as `1.4.0`. Empty when the client did not say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app: String,
    /// The newest vocabulary the client draws.
    pub vocab: u32,
    /// Light or dark.
    #[serde(default)]
    pub scheme: Scheme,
    /// The contrast the user asked for.
    #[serde(default)]
    pub contrast: Contrast,
    /// The user's text size, in percent of the system's default: `100` is the default,
    /// `130` is the header's `text=1.3`.
    pub text_percent: u16,
    /// The user's locale, such as `en-IN`. Empty when the client did not say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub locale: String,
    /// The user's time zone, such as `Asia/Kolkata`. Empty when the client did not say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tz: String,
    /// The device features this build of the app may use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<String>,
}

impl Client {
    /// The most bytes a descriptor may hold.
    pub const HEADER_MAX: usize = 1024;

    /// A client on `platform` and `form` that draws the current vocabulary, with
    /// everything else at its default.
    pub fn new(platform: Platform, form: FormFactor) -> Self {
        Self {
            platform,
            os: String::new(),
            form,
            app: String::new(),
            vocab: crate::VOCAB,
            scheme: Scheme::default(),
            contrast: Contrast::default(),
            text_percent: 100,
            locale: String::new(),
            tz: String::new(),
            caps: Vec::new(),
        }
    }

    /// Reads the descriptor from the header's value.
    pub fn parse(header: &str) -> Result<Self, ClientError> {
        if header.len() > Self::HEADER_MAX {
            return Err(ClientError::TooLong);
        }
        let mut platform = None;
        let mut form = None;
        let mut client = Self::new(Platform::Linux, FormFactor::Desktop);
        // A client that names no vocabulary draws the first.
        client.vocab = 1;
        for pair in header.split(';') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let Some((key, value)) = pair.split_once('=') else {
                return Err(ClientError::Malformed(pair.to_owned()));
            };
            let (key, value) = (key.trim(), value.trim());
            if key.is_empty() || !is_token(key) || !is_token(value) {
                return Err(ClientError::Malformed(pair.to_owned()));
            }
            match key {
                "platform" => {
                    platform = Some(
                        Platform::parse(value)
                            .ok_or_else(|| ClientError::UnknownPlatform(value.to_owned()))?,
                    );
                }
                "form" => {
                    form = Some(
                        FormFactor::parse(value)
                            .ok_or_else(|| ClientError::UnknownForm(value.to_owned()))?,
                    );
                }
                "os" => client.os = value.to_owned(),
                "app" => client.app = value.to_owned(),
                "vocab" => {
                    client.vocab = value.parse().map_err(|_| ClientError::NotANumber("vocab"))?;
                }
                // A scheme or contrast this build does not know is the default.
                "scheme" => {
                    client.scheme = if value == "dark" {
                        Scheme::Dark
                    } else {
                        Scheme::Light
                    };
                }
                "contrast" => {
                    client.contrast = if value == "more" {
                        Contrast::More
                    } else {
                        Contrast::Normal
                    };
                }
                "text" => client.text_percent = percent(value)?,
                "locale" => client.locale = value.to_owned(),
                "tz" => client.tz = value.to_owned(),
                "caps" => {
                    client.caps = value
                        .split('+')
                        .filter(|cap| !cap.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
                _ => {}
            }
        }
        client.platform = platform.ok_or(ClientError::NoPlatform)?;
        client.form = form.ok_or(ClientError::NoForm)?;
        Ok(client)
    }

    /// The descriptor as the header carries it.
    pub fn to_header(&self) -> String {
        let mut out = format!("platform={}", self.platform.as_str());
        let mut pair = |key: &str, value: &str| {
            if !value.is_empty() {
                out.push_str("; ");
                out.push_str(key);
                out.push('=');
                out.push_str(value);
            }
        };
        pair("os", &self.os);
        pair("form", self.form.as_str());
        pair("app", &self.app);
        pair("vocab", &self.vocab.to_string());
        pair(
            "scheme",
            match self.scheme {
                Scheme::Light => "light",
                Scheme::Dark => "dark",
            },
        );
        pair(
            "contrast",
            match self.contrast {
                Contrast::Normal => "normal",
                Contrast::More => "more",
            },
        );
        pair(
            "text",
            &format!("{}.{:02}", self.text_percent / 100, self.text_percent % 100),
        );
        pair("locale", &self.locale);
        pair("tz", &self.tz);
        pair("caps", &self.caps.join("+"));
        out
    }

    /// Whether this build of the app may use the device feature `cap`.
    pub fn supports(&self, cap: &str) -> bool {
        self.caps.iter().any(|have| have == cap)
    }

    /// Whether the client draws one narrow column.
    pub fn is_compact(&self) -> bool {
        self.form.is_compact()
    }
}

/// Whether `text` holds only what a token may: ASCII letters and digits and
/// `. - _ / : +`. An empty value is a token too (`caps=`).
fn is_token(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b".-_/:+".contains(&byte))
}

/// `1.3` as `130`. The text size is kept between half and four times the default.
fn percent(value: &str) -> Result<u16, ClientError> {
    let bad = || ClientError::NotANumber("text");
    let (whole, part) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty() || part.len() > 4 || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(bad());
    }
    let whole: u32 = whole.parse().map_err(|_| bad())?;
    let mut hundredths = 0;
    for (at, digit) in part.bytes().take(2).enumerate() {
        hundredths += u32::from(digit - b'0') * if at == 0 { 10 } else { 1 };
    }
    let percent = whole.saturating_mul(100).saturating_add(hundredths);
    Ok(u16::try_from(percent.clamp(50, 400)).unwrap_or(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_descriptor_of_the_protocol_reads_whole() {
        let client = Client::parse(
            "platform=ios; os=26.1; form=phone; app=1.4.0; vocab=1; scheme=dark; contrast=normal; \
             text=1.0; locale=en-IN; tz=Asia/Kolkata; caps=biometrics+push+camera",
        )
        .unwrap();
        assert_eq!(client.platform, Platform::Ios);
        assert!(client.platform.is_apple());
        assert_eq!(client.os, "26.1");
        assert_eq!(client.form, FormFactor::Phone);
        assert_eq!(client.app, "1.4.0");
        assert_eq!(client.vocab, 1);
        assert_eq!(client.scheme, Scheme::Dark);
        assert_eq!(client.contrast, Contrast::Normal);
        assert_eq!(client.text_percent, 100);
        assert_eq!(client.locale, "en-IN");
        assert_eq!(client.tz, "Asia/Kolkata");
        assert_eq!(client.caps, ["biometrics", "push", "camera"]);
        assert!(client.is_compact());
    }

    #[test]
    fn every_platform_and_form_reads_back_from_its_name() {
        for platform in Platform::ALL {
            assert_eq!(Platform::parse(platform.as_str()), Some(platform));
        }
        for form in FormFactor::ALL {
            assert_eq!(FormFactor::parse(form.as_str()), Some(form));
        }
    }

    #[test]
    fn only_the_platform_and_the_form_are_needed() {
        let client = Client::parse("platform=linux;form=desktop").unwrap();
        assert_eq!(client.vocab, 1);
        assert_eq!(client.text_percent, 100);
        assert_eq!(client.scheme, Scheme::Light);
        assert!(client.caps.is_empty());
        assert_eq!(
            Client::parse("form=phone"),
            Err(ClientError::NoPlatform)
        );
        assert_eq!(Client::parse("platform=ios"), Err(ClientError::NoForm));
    }

    #[test]
    fn a_key_from_a_newer_client_is_passed_over() {
        let client = Client::parse("platform=android; form=tablet; fold=open; vocab=7").unwrap();
        assert_eq!(client.vocab, 7);
        assert_eq!(client.form, FormFactor::Tablet);
    }

    #[test]
    fn what_is_not_a_token_is_refused() {
        for bad in [
            "platform",
            "platform=ios; form=phone; locale=en IN",
            "platform=ios; form=phone; app=\"1\"",
            "platform=ios; form=phone; tz=<x>",
            "=ios",
        ] {
            assert!(
                matches!(Client::parse(bad), Err(ClientError::Malformed(_))),
                "{bad}"
            );
        }
        assert_eq!(
            Client::parse("platform=beos; form=phone"),
            Err(ClientError::UnknownPlatform("beos".to_owned()))
        );
        assert_eq!(
            Client::parse("platform=ios; form=fridge"),
            Err(ClientError::UnknownForm("fridge".to_owned()))
        );
        assert_eq!(
            Client::parse("platform=ios; form=phone; vocab=one"),
            Err(ClientError::NotANumber("vocab"))
        );
        assert_eq!(
            Client::parse(&format!("platform=ios; form=phone; app={}", "1".repeat(2000))),
            Err(ClientError::TooLong)
        );
    }

    #[test]
    fn the_text_size_is_a_percentage_kept_in_range() {
        let text = |value: &str| {
            Client::parse(&format!("platform=ios; form=phone; text={value}"))
                .map(|client| client.text_percent)
        };
        assert_eq!(text("1"), Ok(100));
        assert_eq!(text("1.0"), Ok(100));
        assert_eq!(text("1.35"), Ok(135));
        assert_eq!(text("0.8"), Ok(80));
        assert_eq!(text("3.1"), Ok(310));
        assert_eq!(text("0.1"), Ok(50));
        assert_eq!(text("99"), Ok(400));
        assert_eq!(text("big"), Err(ClientError::NotANumber("text")));
        assert_eq!(text(".5"), Err(ClientError::NotANumber("text")));
    }

    #[test]
    fn a_descriptor_writes_what_it_read() {
        let mut client = Client::new(Platform::Ipados, FormFactor::Tablet);
        client.os = "26.1".to_owned();
        client.app = "2.0.0".to_owned();
        client.scheme = Scheme::Dark;
        client.contrast = Contrast::More;
        client.text_percent = 135;
        client.locale = "pa-IN".to_owned();
        client.tz = "Asia/Kolkata".to_owned();
        client.caps = vec!["push".to_owned(), "camera".to_owned()];
        let header = client.to_header();
        assert_eq!(
            header,
            "platform=ipados; os=26.1; form=tablet; app=2.0.0; vocab=1; scheme=dark; \
             contrast=more; text=1.35; locale=pa-IN; tz=Asia/Kolkata; caps=push+camera"
        );
        assert_eq!(Client::parse(&header).unwrap(), client);
    }
}
