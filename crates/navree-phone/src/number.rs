//! [`PhoneNumber`], [`Region`] and [`NumberType`].

use std::fmt;
use std::str::FromStr;

use crate::format::{self, PhoneFormat};
use crate::meta::{self, Kind, Territory};
use crate::parse::{self, Length, Parsed};

/// The most digits in E.164: a calling code and the national number
/// together (ITU-T E.164).
pub const MAX_E164_DIGITS: usize = 15;

/// The most digits kept of an extension.
const MAX_EXTENSION: usize = 20;

/// Why text is not a phone number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseError {
    /// Nothing in it reads as a phone number.
    NotANumber,
    /// It has no country calling code, and no region was given to read it in.
    NoRegion,
    /// Its country calling code does not exist.
    InvalidCountryCode,
    /// Too few digits follow the international prefix.
    TooShortAfterIdd,
    /// Too few digits for any number.
    TooShort,
    /// Too long: more than 250 characters, more than 17 national digits, or
    /// more than E.164's 15 digits in all.
    TooLong,
    /// The digits do not fit the numbering plan of their country: a number
    /// that cannot exist there, or a length its numbers never have.
    Invalid,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotANumber => "not a phone number",
            Self::NoRegion => "no country calling code, and no region to read the number in",
            Self::InvalidCountryCode => "not a country calling code",
            Self::TooShortAfterIdd => "too few digits after the international prefix",
            Self::TooShort => "too few digits for a phone number",
            Self::TooLong => "too long for a phone number",
            Self::Invalid => "not a number in its country's numbering plan",
        })
    }
}

impl std::error::Error for ParseError {}

/// What kind of line a number reaches, as its country's numbering plan
/// tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum NumberType {
    /// A landline.
    FixedLine,
    /// A mobile phone.
    Mobile,
    /// A landline or a mobile: some plans (the United States, Canada,
    /// China, ...) don't tell them apart.
    FixedLineOrMobile,
    /// Free for the caller (`0800`, `1-800`).
    TollFree,
    /// Charged above normal rates.
    PremiumRate,
    /// The cost is split between the caller and the receiver.
    SharedCost,
    /// Voice over IP.
    Voip,
    /// A personal number, routed to wherever its owner is.
    PersonalNumber,
    /// A pager.
    Pager,
    /// A company's universal access number, routed to one of its offices.
    Uan,
    /// A voicemail access number.
    Voicemail,
}

impl NumberType {
    /// Every type, in declaration order.
    pub const ALL: [NumberType; 11] = [
        Self::FixedLine,
        Self::Mobile,
        Self::FixedLineOrMobile,
        Self::TollFree,
        Self::PremiumRate,
        Self::SharedCost,
        Self::Voip,
        Self::PersonalNumber,
        Self::Pager,
        Self::Uan,
        Self::Voicemail,
    ];

    /// The type's name in `snake_case`: `fixed_line`, `mobile`, ...
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FixedLine => "fixed_line",
            Self::Mobile => "mobile",
            Self::FixedLineOrMobile => "fixed_line_or_mobile",
            Self::TollFree => "toll_free",
            Self::PremiumRate => "premium_rate",
            Self::SharedCost => "shared_cost",
            Self::Voip => "voip",
            Self::PersonalNumber => "personal_number",
            Self::Pager => "pager",
            Self::Uan => "uan",
            Self::Voicemail => "voicemail",
        }
    }

    /// Whether a number of this type can be a mobile phone (`Mobile`, or
    /// `FixedLineOrMobile` where the plan doesn't say).
    pub fn may_be_mobile(self) -> bool {
        matches!(self, Self::Mobile | Self::FixedLineOrMobile)
    }

    /// The description that holds this type's numbers and example.
    fn kind(self) -> Kind {
        match self {
            Self::FixedLine | Self::FixedLineOrMobile => Kind::FixedLine,
            Self::Mobile => Kind::Mobile,
            Self::TollFree => Kind::TollFree,
            Self::PremiumRate => Kind::PremiumRate,
            Self::SharedCost => Kind::SharedCost,
            Self::Voip => Kind::Voip,
            Self::PersonalNumber => Kind::PersonalNumber,
            Self::Pager => Kind::Pager,
            Self::Uan => Kind::Uan,
            Self::Voicemail => Kind::Voicemail,
        }
    }
}

impl fmt::Display for NumberType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The type of national number `nsn` in `territory`, or `None` when it
/// fits none of the territory's types.
pub(crate) fn kind_of(territory: &Territory, nsn: &[u8]) -> Option<NumberType> {
    if !territory.general.pattern.matches(nsn) {
        return None;
    }
    for (kind, ty) in [
        (Kind::PremiumRate, NumberType::PremiumRate),
        (Kind::TollFree, NumberType::TollFree),
        (Kind::SharedCost, NumberType::SharedCost),
        (Kind::Voip, NumberType::Voip),
        (Kind::PersonalNumber, NumberType::PersonalNumber),
        (Kind::Pager, NumberType::Pager),
        (Kind::Uan, NumberType::Uan),
        (Kind::Voicemail, NumberType::Voicemail),
    ] {
        if territory.is(kind, nsn) {
            return Some(ty);
        }
    }
    if territory.is(Kind::FixedLine, nsn) {
        return Some(
            if territory.same_mobile_and_fixed || territory.is(Kind::Mobile, nsn) {
                NumberType::FixedLineOrMobile
            } else {
                NumberType::FixedLine
            },
        );
    }
    if !territory.same_mobile_and_fixed && territory.is(Kind::Mobile, nsn) {
        return Some(NumberType::Mobile);
    }
    None
}

/// A country or area with a numbering plan of its own, by its ISO 3166-1
/// alpha-2 code (`GB`), plus the few areas libphonenumber gives codes of
/// their own: Ascension Island (`AC`), Tristan da Cunha (`TA`) and Kosovo
/// (`XK`).
///
/// ```
/// use navree_phone::Region;
///
/// let gb = Region::parse("gb").unwrap();
/// assert_eq!(gb.as_str(), "GB");
/// assert_eq!(gb.name(), "United Kingdom");
/// assert_eq!(gb.calling_code(), 44);
/// assert_eq!(Region::all().count(), 245);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Region(u8);

impl Region {
    /// The region with this code, in either case.
    pub fn parse(code: &str) -> Option<Region> {
        let bytes = code.as_bytes();
        let [a, b] = bytes else {
            return None;
        };
        let id = [a.to_ascii_uppercase(), b.to_ascii_uppercase()];
        let territories = meta::geographic();
        let at = territories
            .binary_search_by(|territory| territory.id.cmp(&id))
            .ok()?;
        u8::try_from(at).ok().map(Region)
    }

    /// Every region, by code.
    pub fn all() -> impl Iterator<Item = Region> {
        (0..meta::geographic().len()).filter_map(|at| u8::try_from(at).ok().map(Region))
    }

    pub(crate) fn territory(self) -> &'static Territory {
        // Built only from indexes of the table.
        let territories = meta::geographic();
        territories
            .get(usize::from(self.0))
            .or_else(|| territories.first())
            .unwrap_or(&FALLBACK)
    }

    pub(crate) fn of(territory: &'static Territory) -> Option<Region> {
        if !territory.is_geographic() {
            return None;
        }
        Region::parse(territory.id_str())
    }

    /// The code: `GB`.
    pub fn as_str(self) -> &'static str {
        self.territory().id_str()
    }

    /// The name in English: `United Kingdom`.
    pub fn name(self) -> &'static str {
        self.territory().name.get()
    }

    /// The country calling code: `44`.
    pub fn calling_code(self) -> u16 {
        self.territory().code
    }

    /// The prefix dialled before a national number (`0` in the United
    /// Kingdom), if the plan has one.
    pub fn national_prefix(self) -> Option<&'static str> {
        Some(self.territory().national_prefix.get()).filter(|prefix| !prefix.is_empty())
    }

    /// An example number of `kind`, when the region has numbers of that
    /// type. For `FixedLineOrMobile`, the fixed line example.
    pub fn example(self, kind: NumberType) -> Option<PhoneNumber> {
        let desc = self.territory().desc(kind.kind())?;
        PhoneNumber::parse(desc.example.get(), Some(self)).ok()
    }

    /// The best example for a form's placeholder: a mobile number, else a
    /// fixed line.
    pub fn placeholder_example(self) -> Option<PhoneNumber> {
        self.example(NumberType::Mobile)
            .or_else(|| self.example(NumberType::FixedLine))
    }
}

/// Never used: every `Region` indexes the table.
static FALLBACK: Territory = Territory {
    id: *b"00",
    code: 0,
    name: meta::EMPTY,
    idd: meta::NONE,
    national_prefix: meta::EMPTY,
    npp: meta::NONE,
    npt: meta::EMPTY,
    extension_prefix: meta::EMPTY,
    leading: meta::NONE,
    general: meta::d(meta::NONE, 0, 0, meta::EMPTY),
    types: [0; 11],
    formats: (0, 0),
    intl_formats: (0, 0),
    same_mobile_and_fixed: false,
};

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Region({})", self.as_str())
    }
}

/// Text that is not a region code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownRegion;

impl fmt::Display for UnknownRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a region with a numbering plan")
    }
}

impl std::error::Error for UnknownRegion {}

impl FromStr for Region {
    type Err = UnknownRegion;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Region::parse(code).ok_or(UnknownRegion)
    }
}

/// A phone number that fits its country's numbering plan, kept in E.164
/// form (`+447400123456`) with its extension, if any.
///
/// It reads what people type: spaces, dashes, dots and brackets, `tel:`
/// URIs, an extension (`ext. 12`, `x12`, `#12`), letters on a keypad
/// (`1-800-FLOWERS`) and digits of any script (`+٩٧١ ٥٠ ١٢٣ ٤٥٦٧`). A
/// number written with its country calling code (`+44 ...`, or `00 44 ...`
/// from a region that dials `00`) stands alone; one written without it
/// (`07400 123456`) is read in the region given.
///
/// "Fits the plan" means its country calling code exists, and its national
/// number has a length and leading digits that the plan assigns to some
/// kind of line. Nothing checks that the line is in service.
///
/// Work is bounded on any input: text longer than 250 characters is
/// refused, every step is linear or bounded by a constant, and nothing
/// panics.
///
/// ```
/// use navree_phone::{NumberType, PhoneFormat, PhoneNumber, Region};
///
/// let gb = Region::parse("GB");
/// let number = PhoneNumber::parse("07400 123456", gb).unwrap();
/// assert_eq!(number.as_str(), "+447400123456");
/// assert_eq!(number.international(), "+44 7400 123456");
/// assert_eq!(number.national(), "07400 123456");
/// assert_eq!(number.format(PhoneFormat::Rfc3966), "tel:+44-7400-123456");
/// assert_eq!(number.number_type(), Some(NumberType::Mobile));
/// assert_eq!(number.region(), gb);
///
/// // With a country calling code, no region is needed.
/// assert_eq!("+1 (201) 555-0123".parse::<PhoneNumber>().unwrap().as_str(), "+12015550123");
/// assert!(PhoneNumber::parse("07400 123456", None).is_err());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PhoneNumber {
    /// `+` and the digits, then zeros.
    e164: [u8; MAX_E164_DIGITS + 1],
    len: u8,
    code_len: u8,
    extension: [u8; MAX_EXTENSION],
    extension_len: u8,
}

impl PhoneNumber {
    /// Parses `text`, reading a number without a country calling code in
    /// `region`, and checks that it fits its country's numbering plan.
    pub fn parse(text: &str, region: Option<Region>) -> Result<PhoneNumber, ParseError> {
        let parsed = parse::parse(text, region.map(Region::territory))?;
        let number = PhoneNumber::from_parsed(&parsed)?;
        if number.number_type().is_none() {
            return Err(ParseError::Invalid);
        }
        Ok(number)
    }

    /// Parses `text` like [`parse`](Self::parse), but only checks that the
    /// number has a length its country's numbers have, not its leading
    /// digits. It accepts numbers in ranges a plan opened after this
    /// release of the plans, and more mistakes: prefer `parse`.
    pub fn parse_possible(text: &str, region: Option<Region>) -> Result<PhoneNumber, ParseError> {
        let parsed = parse::parse(text, region.map(Region::territory))?;
        let number = PhoneNumber::from_parsed(&parsed)?;
        let main = meta::main_for(parsed.code).ok_or(ParseError::InvalidCountryCode)?;
        match parse::test_length(parsed.nsn.as_bytes(), main) {
            // A number that fits only by its length can read as another
            // from its own E.164 (a leading zero taken as a national prefix
            // again): such text has no one meaning, and is refused.
            Length::Possible => match parse::parse(number.as_str(), None) {
                Ok(again) if again.code == parsed.code && again.nsn == parsed.nsn => Ok(number),
                _ => Err(ParseError::Invalid),
            },
            Length::TooShort => Err(ParseError::TooShort),
            Length::TooLong => Err(ParseError::TooLong),
            Length::LocalOnly | Length::Invalid => Err(ParseError::Invalid),
        }
    }

    /// Reads a number stored in E.164 (`+447400123456`) exactly as it was
    /// written, without checking it against the plans compiled in, so a
    /// number saved under older plans still reads after an update. Only `+`,
    /// a calling code that exists and at most 15 digits in all are accepted;
    /// read what people type with [`parse`](Self::parse).
    ///
    /// ```
    /// use navree_phone::PhoneNumber;
    ///
    /// let stored = PhoneNumber::from_e164("+447400123456").unwrap();
    /// assert_eq!(stored.international(), "+44 7400 123456");
    /// assert!(PhoneNumber::from_e164("+44 7400 123456").is_err());
    /// ```
    pub fn from_e164(text: &str) -> Result<PhoneNumber, ParseError> {
        let digits = text.strip_prefix('+').ok_or(ParseError::NotANumber)?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ParseError::NotANumber);
        }
        if digits.len() > MAX_E164_DIGITS {
            return Err(ParseError::TooLong);
        }
        let (code, nsn) =
            parse::split_country_code(digits).ok_or(ParseError::InvalidCountryCode)?;
        if nsn.len() < 2 {
            return Err(ParseError::TooShort);
        }
        PhoneNumber::from_parsed(&Parsed {
            code,
            nsn: nsn.to_owned(),
            extension: String::new(),
        })
    }

    fn from_parsed(parsed: &Parsed) -> Result<PhoneNumber, ParseError> {
        let code = parsed.code.to_string();
        let digits = code.len() + parsed.nsn.len();
        if digits > MAX_E164_DIGITS {
            return Err(ParseError::TooLong);
        }
        let mut e164 = [0_u8; MAX_E164_DIGITS + 1];
        e164[0] = b'+';
        for (slot, byte) in e164
            .iter_mut()
            .skip(1)
            .zip(code.bytes().chain(parsed.nsn.bytes()))
        {
            *slot = byte;
        }
        let mut extension = [0_u8; MAX_EXTENSION];
        let kept = parsed.extension.len().min(MAX_EXTENSION);
        for (slot, byte) in extension.iter_mut().zip(parsed.extension.bytes()) {
            *slot = byte;
        }
        Ok(PhoneNumber {
            e164,
            len: u8::try_from(digits + 1).unwrap_or(0),
            code_len: u8::try_from(code.len()).unwrap_or(0),
            extension,
            extension_len: u8::try_from(kept).unwrap_or(0),
        })
    }

    /// The E.164 form: `+447400123456`.
    pub fn as_str(&self) -> &str {
        self.e164
            .get(..usize::from(self.len))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }

    /// The country calling code: `44`.
    pub fn country_code(&self) -> u16 {
        self.as_str()
            .get(1..1 + usize::from(self.code_len))
            .and_then(|code| code.parse().ok())
            .unwrap_or(0)
    }

    /// The national significant number, the digits after the calling code:
    /// `7400123456`. It keeps leading zeros where a plan has them (Italy's
    /// `06...`).
    pub fn national_number(&self) -> &str {
        self.as_str()
            .get(1 + usize::from(self.code_len)..)
            .unwrap_or("")
    }

    /// The extension, which E.164 has no room for: `12` from `ext. 12`.
    pub fn extension(&self) -> Option<&str> {
        self.extension
            .get(..usize::from(self.extension_len))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .filter(|extension| !extension.is_empty())
    }

    /// The same number without its extension.
    pub fn without_extension(mut self) -> PhoneNumber {
        self.extension = [0; MAX_EXTENSION];
        self.extension_len = 0;
        self
    }

    /// The region the number belongs to: of the regions sharing its calling
    /// code (`+1` is the United States, Canada and 23 more), the one whose
    /// plan it fits. `None` for a global service number (`+800`).
    pub fn region(&self) -> Option<Region> {
        self.territory().and_then(Region::of)
    }

    fn territory(&self) -> Option<&'static Territory> {
        parse::territory_of(self.country_code(), self.national_number().as_bytes())
    }

    /// The kind of line, when the number fits its region's plan; always
    /// `Some` for a number from [`parse`](Self::parse).
    pub fn number_type(&self) -> Option<NumberType> {
        kind_of(self.territory()?, self.national_number().as_bytes())
    }

    /// Whether the number fits the plan of `region` itself: a Canadian
    /// number fits `CA` but not `US`, though both use `+1`.
    pub fn is_valid_for(&self, region: Region) -> bool {
        let territory = region.territory();
        territory.code == self.country_code()
            && kind_of(territory, self.national_number().as_bytes()).is_some()
    }

    /// Whether the number can be dialled from abroad. Some numbers (many
    /// toll-free ones) only work from inside their country.
    pub fn is_internationally_diallable(&self) -> bool {
        self.territory().is_none_or(|territory| {
            !territory.is(
                Kind::NoInternationalDialling,
                self.national_number().as_bytes(),
            )
        })
    }

    /// The number written in `how`.
    pub fn format(&self, how: PhoneFormat) -> String {
        format::format(
            self.country_code(),
            self.national_number(),
            self.extension().unwrap_or(""),
            how,
        )
    }

    /// `+44 7400 123456`, with the extension if any (`+44 7400 123456 x12`).
    pub fn international(&self) -> String {
        self.format(PhoneFormat::International)
    }

    /// `07400 123456`, with the extension if any.
    pub fn national(&self) -> String {
        self.format(PhoneFormat::National)
    }

    /// `tel:+44-7400-123456`, with the extension if any (`;ext=12`).
    pub fn rfc3966(&self) -> String {
        self.format(PhoneFormat::Rfc3966)
    }
}

impl fmt::Display for PhoneNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for PhoneNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.extension() {
            Some(extension) => write!(f, "PhoneNumber({} ext. {extension})", self.as_str()),
            None => write!(f, "PhoneNumber({})", self.as_str()),
        }
    }
}

impl FromStr for PhoneNumber {
    type Err = ParseError;

    /// A number written with its country calling code (`+44 ...`); see
    /// [`PhoneNumber::parse`] for national numbers.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        PhoneNumber::parse(text, None)
    }
}

impl AsRef<str> for PhoneNumber {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<PhoneNumber> for String {
    fn from(number: PhoneNumber) -> Self {
        number.as_str().to_owned()
    }
}
