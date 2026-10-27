//! [`Faker`]: fake data for factories, from a seeded generator.
//!
//! Every value comes from one pseudo-random stream (xoshiro256\*\*, seeded
//! through SplitMix64), so a factory given a seed
//! ([`Factory::seed`](crate::factory::Factory::seed)) builds the same records
//! on every run and every platform. Unseeded factories draw a fresh seed
//! from the operating system for each call.

use std::collections::HashSet;
use std::hash::{BuildHasher, Hash};
use std::ops::RangeInclusive;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use uuid::Uuid;

use crate::ulid::Ulid;
use crate::value::Decimal;

/// How many draws [`Faker::unique`] makes before giving up.
const UNIQUE_ATTEMPTS: usize = 10_000;

/// Fake data: people, companies, addresses, text, numbers, dates, ids and
/// choices, from a seeded generator.
///
/// A factory hands one to its definition through
/// [`FactoryContext::fake`](crate::factory::FactoryContext::fake); build one
/// directly with [`Faker::seeded`] (reproducible) or [`Faker::new`].
///
/// ```
/// use navree_orm::factory::Faker;
///
/// let mut a = Faker::seeded(7);
/// let mut b = Faker::seeded(7);
/// assert_eq!(a.name(), b.name(), "the same seed gives the same values");
///
/// let email = a.safe_email();
/// assert!(email.ends_with("@example.com") || email.ends_with("@example.org") || email.ends_with("@example.net"));
/// let employees: i32 = a.number(1..=500);
/// assert!((1..=500).contains(&employees));
/// let words = a.words(3);
/// assert_eq!(words.split(' ').count(), 3);
/// ```
#[derive(Clone, Debug)]
pub struct Faker {
    seed: u64,
    state: [u64; 4],
    seen: HashSet<u64>,
}

impl Default for Faker {
    fn default() -> Self {
        Faker::new()
    }
}

/// SplitMix64: expands a 64-bit seed into the generator's state.
fn split_mix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Faker {
    /// A faker with a fresh, random seed (see [`seed`](Faker::seed) to read
    /// it back and reproduce a run).
    pub fn new() -> Self {
        let random = Uuid::new_v4().as_u128();
        Faker::seeded((random >> 64) as u64 ^ random as u64)
    }

    /// A faker whose values are fully determined by `seed`.
    pub fn seeded(seed: u64) -> Self {
        let mut expand = seed;
        let state = [
            split_mix(&mut expand),
            split_mix(&mut expand),
            split_mix(&mut expand),
            split_mix(&mut expand),
        ];
        Faker {
            seed,
            state,
            seen: HashSet::new(),
        }
    }

    /// The seed this faker started from.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    // ---------------------------------------------------------------------
    // The generator
    // ---------------------------------------------------------------------

    /// The next 64 random bits (xoshiro256\*\*).
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// A number below `bound` (`bound > 0`), without modulo bias.
    fn below(&mut self, bound: u64) -> u64 {
        debug_assert!(bound > 0);
        // Lemire's multiply-shift: reject the few draws that would favour
        // the low values.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let product = u128::from(self.next_u64()) * u128::from(bound);
            if product as u64 >= threshold {
                return (product >> 64) as u64;
            }
        }
    }

    /// A float in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// An index into a list of `len` items (`len > 0`).
    fn index(&mut self, len: usize) -> usize {
        self.below(len as u64) as usize
    }

    // ---------------------------------------------------------------------
    // Numbers and choices
    // ---------------------------------------------------------------------

    /// A number in `range` (both ends included), of any integer or float
    /// type.
    ///
    /// ```
    /// # let mut fake = navree_orm::factory::Faker::seeded(1);
    /// let cents: i64 = fake.number(100..=99_999);
    /// let ratio: f64 = fake.number(0.0..=1.0);
    /// assert!((100..=99_999).contains(&cents) && (0.0..=1.0).contains(&ratio));
    /// ```
    pub fn number<T: FakeNumber>(&mut self, range: RangeInclusive<T>) -> T {
        T::fake(self, range)
    }

    /// A decimal between `low` and `high` with `scale` digits after the
    /// point (`fake.decimal(10, 1_000, 2)` gives values like `347.25`).
    pub fn decimal(&mut self, low: i64, high: i64, scale: u32) -> Decimal {
        let factor = 10i64.saturating_pow(scale);
        let units = self.number(low.saturating_mul(factor)..=high.saturating_mul(factor));
        Decimal::new(units, scale)
    }

    /// `true` or `false`, evenly.
    pub fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// `true` with probability `percent` / 100.
    pub fn chance(&mut self, percent: u8) -> bool {
        self.below(100) < u64::from(percent.min(100))
    }

    /// `Some(value)` with probability `percent` / 100, else `None`.
    pub fn optional<T>(&mut self, percent: u8, value: impl FnOnce(&mut Faker) -> T) -> Option<T> {
        if self.chance(percent) {
            Some(value(self))
        } else {
            None
        }
    }

    /// One item of `items`, cloned.
    ///
    /// # Panics
    ///
    /// When `items` is empty.
    pub fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        assert!(!items.is_empty(), "Faker::pick needs at least one item");
        items[self.index(items.len())].clone()
    }

    /// `count` distinct items of `items` in random order (all of them when
    /// `count` is larger).
    pub fn pick_many<T: Clone>(&mut self, items: &[T], count: usize) -> Vec<T> {
        let mut indexes: Vec<usize> = (0..items.len()).collect();
        self.shuffle(&mut indexes);
        indexes
            .into_iter()
            .take(count)
            .map(|index| items[index].clone())
            .collect()
    }

    /// Shuffles `items` in place (Fisher-Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.index(i + 1);
            items.swap(i, j);
        }
    }

    /// A value from `generate` that this faker has not produced through
    /// `unique` before (compared by hash): unique emails, slugs, names.
    ///
    /// # Panics
    ///
    /// When 10 000 draws in a row were all seen before: the generator has
    /// too few values for the number of records.
    ///
    /// ```
    /// # let mut fake = navree_orm::factory::Faker::seeded(3);
    /// let first = fake.unique(|f| f.number(1..=3_u8));
    /// let second = fake.unique(|f| f.number(1..=3_u8));
    /// assert_ne!(first, second);
    /// ```
    pub fn unique<T: Hash>(&mut self, mut generate: impl FnMut(&mut Faker) -> T) -> T {
        let hasher = std::hash::BuildHasherDefault::<std::hash::DefaultHasher>::default();
        for _ in 0..UNIQUE_ATTEMPTS {
            let value = generate(self);
            if self.seen.insert(hasher.hash_one(&value)) {
                return value;
            }
        }
        panic!(
            "Faker::unique gave up after {UNIQUE_ATTEMPTS} draws: the generator has run out of new values"
        );
    }

    // ---------------------------------------------------------------------
    // People
    // ---------------------------------------------------------------------

    /// A first name.
    pub fn first_name(&mut self) -> String {
        self.pick(FIRST_NAMES).to_owned()
    }

    /// A last name.
    pub fn last_name(&mut self) -> String {
        self.pick(LAST_NAMES).to_owned()
    }

    /// A full name.
    pub fn name(&mut self) -> String {
        format!("{} {}", self.first_name(), self.last_name())
    }

    /// A username: lower-case letters, digits and a dot or underscore.
    pub fn username(&mut self) -> String {
        let first = self.first_name().to_lowercase();
        let last = self.last_name().to_lowercase();
        match self.below(3) {
            0 => format!("{first}.{last}"),
            1 => format!("{first}_{}", self.number(1..=999_u16)),
            _ => format!("{}{last}", &first[..1]),
        }
    }

    /// An email address at a reserved example domain (`example.com`,
    /// `example.org`, `example.net`): never a real mailbox.
    pub fn safe_email(&mut self) -> String {
        let user = self.username();
        let tag = self.number(10..=9_999_u16);
        format!("{user}{tag}@{}", self.pick(SAFE_DOMAINS))
    }

    /// An email address at a fake company domain (`.test`, never resolvable).
    pub fn email(&mut self) -> String {
        let user = self.username();
        let tag = self.number(10..=9_999_u16);
        format!("{user}{tag}@{}", self.domain())
    }

    /// A phone number in E.164 form, from the ranges reserved for fiction
    /// (North American `555-01xx`, UK `07700 900xxx`).
    pub fn phone(&mut self) -> String {
        if self.boolean() {
            format!(
                "+1{}55501{:02}",
                self.number(201..=989_u16),
                self.number(0..=99_u8)
            )
        } else {
            format!("+447700900{:03}", self.number(0..=999_u16))
        }
    }

    /// A job title.
    pub fn job_title(&mut self) -> String {
        format!("{} {}", self.pick(JOB_LEVELS), self.pick(JOB_ROLES))
    }

    // ---------------------------------------------------------------------
    // Companies and the web
    // ---------------------------------------------------------------------

    /// A company name.
    pub fn company(&mut self) -> String {
        match self.below(3) {
            0 => format!("{} {}", self.last_name(), self.pick(COMPANY_SUFFIXES)),
            1 => format!("{} & {}", self.last_name(), self.last_name()),
            _ => format!(
                "{} {}",
                capitalize(self.pick(WORDS)),
                self.pick(COMPANY_SUFFIXES)
            ),
        }
    }

    /// A short marketing line.
    pub fn catch_phrase(&mut self) -> String {
        format!(
            "{} {} {}",
            self.pick(BUZZ_ADJECTIVES),
            self.pick(BUZZ_ADJECTIVES),
            self.pick(BUZZ_NOUNS)
        )
    }

    /// A domain name under `.test` (reserved, never resolvable).
    pub fn domain(&mut self) -> String {
        let word = self.pick(WORDS);
        let other = self.pick(WORDS);
        format!("{word}{other}.test")
    }

    /// An `https://` URL under a [`domain`](Faker::domain).
    pub fn url(&mut self) -> String {
        format!("https://www.{}/{}", self.domain(), self.slug())
    }

    /// A URL slug of two to four words (`brave-orange-harbor`).
    pub fn slug(&mut self) -> String {
        let count = self.number(2..=4_usize);
        (0..count)
            .map(|_| self.pick(WORDS))
            .collect::<Vec<_>>()
            .join("-")
    }

    /// An IPv4 address in a documentation range (`192.0.2.x`,
    /// `198.51.100.x`, `203.0.113.x`).
    pub fn ipv4(&mut self) -> String {
        let prefix = self.pick(&["192.0.2", "198.51.100", "203.0.113"]);
        format!("{prefix}.{}", self.number(1..=254_u8))
    }

    // ---------------------------------------------------------------------
    // Places
    // ---------------------------------------------------------------------

    /// A street address (`42 Harbor Lane`).
    pub fn street_address(&mut self) -> String {
        format!(
            "{} {} {}",
            self.number(1..=999_u16),
            self.last_name(),
            self.pick(STREET_KINDS)
        )
    }

    /// A city.
    pub fn city(&mut self) -> String {
        self.pick(CITIES).to_owned()
    }

    /// A postal code (five digits).
    pub fn postcode(&mut self) -> String {
        format!("{:05}", self.number(1_000..=99_999_u32))
    }

    /// A country name.
    pub fn country(&mut self) -> String {
        self.pick(COUNTRIES).1.to_owned()
    }

    /// An ISO 3166-1 alpha-2 country code (`DE`).
    pub fn country_code(&mut self) -> String {
        self.pick(COUNTRIES).0.to_owned()
    }

    // ---------------------------------------------------------------------
    // Text
    // ---------------------------------------------------------------------

    /// A lower-case word.
    pub fn word(&mut self) -> String {
        self.pick(WORDS).to_owned()
    }

    /// `count` words separated by spaces.
    pub fn words(&mut self, count: usize) -> String {
        (0..count)
            .map(|_| self.pick(WORDS))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A sentence of four to twelve words, capitalized, ending with a period.
    pub fn sentence(&mut self) -> String {
        let count = self.number(4..=12_usize);
        format!("{}.", capitalize(&self.words(count)))
    }

    /// `count` sentences.
    pub fn sentences(&mut self, count: usize) -> String {
        (0..count)
            .map(|_| self.sentence())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A paragraph of three to six sentences.
    pub fn paragraph(&mut self) -> String {
        let count = self.number(3..=6_usize);
        self.sentences(count)
    }

    /// A title: two to five capitalized words.
    pub fn title(&mut self) -> String {
        let count = self.number(2..=5_usize);
        (0..count)
            .map(|_| capitalize(self.pick(WORDS)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Text of at most `max_chars` characters, cut at a word.
    pub fn text(&mut self, max_chars: usize) -> String {
        let mut text = String::new();
        loop {
            let sentence = self.sentence();
            let separator = usize::from(!text.is_empty());
            if text.len() + separator + sentence.len() > max_chars {
                break;
            }
            if separator == 1 {
                text.push(' ');
            }
            text.push_str(&sentence);
        }
        if text.is_empty() {
            let word = self.word();
            text = word.chars().take(max_chars).collect();
        }
        text
    }

    /// `length` letters and digits.
    pub fn alphanumeric(&mut self, length: usize) -> String {
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        (0..length)
            .map(|_| CHARS[self.index(CHARS.len())] as char)
            .collect()
    }

    /// `length` lower-case hexadecimal digits.
    pub fn hex(&mut self, length: usize) -> String {
        const CHARS: &[u8] = b"0123456789abcdef";
        (0..length)
            .map(|_| CHARS[self.index(CHARS.len())] as char)
            .collect()
    }

    /// A password of sixteen letters, digits and symbols.
    pub fn password(&mut self) -> String {
        const CHARS: &[u8] =
            b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789!@#$%^&*-_=+";
        (0..16)
            .map(|_| CHARS[self.index(CHARS.len())] as char)
            .collect()
    }

    // ---------------------------------------------------------------------
    // Colors and money
    // ---------------------------------------------------------------------

    /// A color name.
    pub fn color_name(&mut self) -> String {
        self.pick(COLORS).to_owned()
    }

    /// A CSS hex color (`#3a7bd5`).
    pub fn hex_color(&mut self) -> String {
        format!("#{}", self.hex(6))
    }

    /// An ISO 4217 currency code.
    pub fn currency_code(&mut self) -> String {
        self.pick(CURRENCIES).to_owned()
    }

    // ---------------------------------------------------------------------
    // Dates and times
    // ---------------------------------------------------------------------

    /// A moment between `start` and `end` (whole microseconds, the
    /// precision databases store).
    pub fn date_time_between(&mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> DateTime<Utc> {
        let (low, high) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let span = (high - low).num_microseconds().unwrap_or(i64::MAX).max(0);
        let offset = self.number(0..=span);
        low + Duration::microseconds(offset)
    }

    /// A moment within `within` before now.
    pub fn past(&mut self, within: Duration) -> DateTime<Utc> {
        let now = now_micros();
        self.date_time_between(now - within, now)
    }

    /// A moment within `within` after now.
    pub fn future(&mut self, within: Duration) -> DateTime<Utc> {
        let now = now_micros();
        self.date_time_between(now, now + within)
    }

    /// A moment in the last year.
    pub fn recent(&mut self) -> DateTime<Utc> {
        self.past(Duration::days(365))
    }

    /// A date between `start` and `end` (both included).
    pub fn date_between(&mut self, start: NaiveDate, end: NaiveDate) -> NaiveDate {
        let (low, high) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let days = (high - low).num_days();
        low + Duration::days(self.number(0..=days))
    }

    // ---------------------------------------------------------------------
    // Identifiers
    // ---------------------------------------------------------------------

    /// A random (version 4) UUID drawn from this faker's stream.
    pub fn uuid(&mut self) -> Uuid {
        let bits = (u128::from(self.next_u64()) << 64) | u128::from(self.next_u64());
        uuid::Builder::from_random_bytes(bits.to_be_bytes()).into_uuid()
    }

    /// A ULID for a moment in the last year, drawn from this faker's
    /// stream.
    pub fn ulid(&mut self) -> Ulid {
        let millis = self.recent().timestamp_millis().max(0) as u64;
        let random = (u128::from(self.next_u64()) << 64) | u128::from(self.next_u64());
        Ulid::from_parts(millis, random)
    }
}

/// The current time, whole microseconds.
fn now_micros() -> DateTime<Utc> {
    crate::exec::truncate_to_micros(Utc::now())
}

/// `word` with its first letter in upper case.
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Numbers [`Faker::number`] draws: every integer type, `f32` and `f64`.
pub trait FakeNumber: Copy + PartialOrd {
    /// A value in `range`, both ends included.
    fn fake(faker: &mut Faker, range: RangeInclusive<Self>) -> Self;
}

macro_rules! fake_integers {
    ($($ty:ty),*) => {$(
        impl FakeNumber for $ty {
            fn fake(faker: &mut Faker, range: RangeInclusive<Self>) -> Self {
                let (low, high) = (*range.start(), *range.end());
                let (low, high) = if low <= high { (low, high) } else { (high, low) };
                // The span as unsigned, in i128 so every width fits.
                let span = (high as i128 - low as i128) as u128;
                if span >= u128::from(u64::MAX) {
                    return (low as i128 + i128::from(faker.next_u64())) as $ty;
                }
                let offset = faker.below(span as u64 + 1);
                (low as i128 + i128::from(offset)) as $ty
            }
        }
    )*};
}

fake_integers!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

macro_rules! fake_floats {
    ($($ty:ty),*) => {$(
        impl FakeNumber for $ty {
            fn fake(faker: &mut Faker, range: RangeInclusive<Self>) -> Self {
                let (low, high) = (*range.start(), *range.end());
                let (low, high) = if low <= high { (low, high) } else { (high, low) };
                let value = low as f64 + faker.unit() * (high as f64 - low as f64);
                (value as $ty).clamp(low, high)
            }
        }
    )*};
}

fake_floats!(f32, f64);

const FIRST_NAMES: &[&str] = &[
    "Ada", "Alan", "Amara", "Anika", "Aria", "Arjun", "Beatriz", "Bilal", "Camille", "Carlos",
    "Chen", "Chiara", "Dalia", "Daniel", "Diego", "Elena", "Elif", "Emeka", "Farah", "Felix",
    "Freya", "Grace", "Hana", "Hugo", "Ines", "Isaac", "Ivan", "Jamal", "Jana", "Jonas", "Kai",
    "Kavya", "Kenji", "Lara", "Leila", "Liam", "Lucia", "Malik", "Maya", "Mei", "Mila", "Nadia",
    "Navraj", "Navreet", "Nia", "Noah", "Olga", "Omar", "Priya", "Rafael", "Rosa", "Sana", "Sofia",
    "Tariq", "Theo", "Uma", "Vera", "Wen", "Yara", "Yusuf", "Zara",
];

const LAST_NAMES: &[&str] = &[
    "Abe", "Adeyemi", "Alvarez", "Andersen", "Bauer", "Bianchi", "Brown", "Carter", "Chen",
    "Costa", "Dubois", "Eriksen", "Fischer", "Garcia", "Gill", "Haddad", "Hansen", "Ito", "Jensen",
    "Kaur", "Khan", "Kim", "Kowalski", "Larsen", "Lopez", "Martin", "Meyer", "Moreau", "Nakamura",
    "Nguyen", "Novak", "Okafor", "Olsen", "Park", "Patel", "Perez", "Petrov", "Quinn", "Rossi",
    "Sato", "Schmidt", "Silva", "Singh", "Smith", "Sousa", "Suzuki", "Tanaka", "Torres", "Varga",
    "Wagner", "Walker", "Weber", "Wong", "Yilmaz", "Young", "Zhang",
];

const WORDS: &[&str] = &[
    "amber", "anchor", "apex", "arbor", "atlas", "aurora", "beacon", "birch", "bold", "breeze",
    "bright", "cascade", "cedar", "cinder", "clear", "cobalt", "comet", "coral", "crest",
    "crystal", "delta", "drift", "dune", "echo", "ember", "falcon", "fern", "field", "flint",
    "forge", "frost", "garnet", "glade", "glow", "granite", "grove", "harbor", "hazel", "horizon",
    "iris", "island", "ivory", "jade", "juniper", "kestrel", "lagoon", "lantern", "linen", "lumen",
    "maple", "meadow", "mesa", "mint", "nimbus", "north", "nova", "oak", "ocean", "onyx", "orbit",
    "orchid", "pebble", "pine", "pioneer", "prism", "quartz", "quill", "raven", "reef", "ridge",
    "river", "rowan", "sable", "sage", "signal", "silver", "slate", "solar", "spark", "spruce",
    "stone", "summit", "swift", "thistle", "tide", "timber", "topaz", "trail", "vale", "velvet",
    "vertex", "violet", "willow", "wind", "zephyr",
];

const COMPANY_SUFFIXES: &[&str] = &[
    "Labs",
    "Group",
    "Partners",
    "Systems",
    "Works",
    "Studio",
    "Holdings",
    "Collective",
    "Co.",
    "Industries",
    "Ventures",
    "Analytics",
];

const BUZZ_ADJECTIVES: &[&str] = &[
    "adaptive",
    "seamless",
    "scalable",
    "intuitive",
    "secure",
    "proactive",
    "robust",
    "unified",
    "realtime",
    "streamlined",
    "resilient",
    "open",
];

const BUZZ_NOUNS: &[&str] = &[
    "platform",
    "workflow",
    "pipeline",
    "dashboard",
    "framework",
    "network",
    "toolkit",
    "service",
    "experience",
    "ecosystem",
];

const JOB_LEVELS: &[&str] = &[
    "Junior",
    "Senior",
    "Lead",
    "Principal",
    "Head of",
    "Staff",
    "Chief",
];

const JOB_ROLES: &[&str] = &[
    "Engineer",
    "Designer",
    "Account Executive",
    "Product Manager",
    "Analyst",
    "Consultant",
    "Architect",
    "Researcher",
    "Support Specialist",
    "Operations Manager",
];

const SAFE_DOMAINS: &[&str] = &["example.com", "example.org", "example.net"];

const STREET_KINDS: &[&str] = &[
    "Street",
    "Avenue",
    "Lane",
    "Road",
    "Way",
    "Boulevard",
    "Court",
    "Place",
];

const CITIES: &[&str] = &[
    "Amsterdam",
    "Auckland",
    "Bangalore",
    "Barcelona",
    "Berlin",
    "Bogota",
    "Boston",
    "Brisbane",
    "Buenos Aires",
    "Cairo",
    "Cape Town",
    "Chicago",
    "Copenhagen",
    "Dublin",
    "Edinburgh",
    "Helsinki",
    "Istanbul",
    "Jakarta",
    "Kyoto",
    "Lagos",
    "Lisbon",
    "London",
    "Lyon",
    "Madrid",
    "Melbourne",
    "Mexico City",
    "Montreal",
    "Mumbai",
    "Nairobi",
    "Oslo",
    "Paris",
    "Prague",
    "Seoul",
    "Singapore",
    "Stockholm",
    "Taipei",
    "Tokyo",
    "Toronto",
    "Vienna",
    "Zurich",
];

const COUNTRIES: &[(&str, &str)] = &[
    ("AR", "Argentina"),
    ("AU", "Australia"),
    ("AT", "Austria"),
    ("BE", "Belgium"),
    ("BR", "Brazil"),
    ("CA", "Canada"),
    ("CL", "Chile"),
    ("CN", "China"),
    ("CO", "Colombia"),
    ("DK", "Denmark"),
    ("EG", "Egypt"),
    ("FI", "Finland"),
    ("FR", "France"),
    ("DE", "Germany"),
    ("GR", "Greece"),
    ("IN", "India"),
    ("ID", "Indonesia"),
    ("IE", "Ireland"),
    ("IT", "Italy"),
    ("JP", "Japan"),
    ("KE", "Kenya"),
    ("MX", "Mexico"),
    ("NL", "Netherlands"),
    ("NZ", "New Zealand"),
    ("NG", "Nigeria"),
    ("NO", "Norway"),
    ("PL", "Poland"),
    ("PT", "Portugal"),
    ("SG", "Singapore"),
    ("ZA", "South Africa"),
    ("KR", "South Korea"),
    ("ES", "Spain"),
    ("SE", "Sweden"),
    ("CH", "Switzerland"),
    ("TW", "Taiwan"),
    ("TR", "Turkey"),
    ("GB", "United Kingdom"),
    ("US", "United States"),
];

const COLORS: &[&str] = &[
    "amber", "azure", "beige", "black", "blue", "bronze", "coral", "crimson", "cyan", "emerald",
    "gold", "gray", "green", "indigo", "lavender", "lime", "magenta", "navy", "olive", "orange",
    "pink", "purple", "red", "silver", "teal", "violet", "white", "yellow",
];

const CURRENCIES: &[&str] = &[
    "AUD", "BRL", "CAD", "CHF", "CNY", "DKK", "EUR", "GBP", "INR", "JPY", "KRW", "MXN", "NOK",
    "NZD", "SEK", "SGD", "USD", "ZAR",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_is_fixed_by_the_seed() {
        let mut a = Faker::seeded(42);
        let mut b = Faker::seeded(42);
        let first: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let second: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        assert_eq!(first, second);
        assert_eq!(a.seed(), 42);
        let mut other = Faker::seeded(43);
        assert_ne!(first[0], other.next_u64());
    }

    #[test]
    fn numbers_stay_in_their_ranges_and_cover_them() {
        let mut fake = Faker::seeded(9);
        let mut seen = [false; 6];
        for _ in 0..500 {
            let n: u8 = fake.number(0..=5);
            seen[usize::from(n)] = true;
            let negative: i64 = fake.number(-10..=-5);
            assert!((-10..=-5).contains(&negative));
            let wide: u64 = fake.number(0..=u64::MAX);
            let _ = wide;
            let float: f64 = fake.number(1.5..=2.5);
            assert!((1.5..=2.5).contains(&float));
        }
        assert!(
            seen.iter().all(|s| *s),
            "every value of a small range comes up"
        );
        // A reversed range on purpose: it is read low to high.
        #[allow(clippy::reversed_empty_ranges)]
        let reversed: i32 = fake.number(10..=1);
        assert!(
            (1..=10).contains(&reversed),
            "a reversed range is read low to high"
        );
        let single: i32 = fake.number(7..=7);
        assert_eq!(single, 7);
    }

    #[test]
    fn decimals_have_their_scale() {
        let mut fake = Faker::seeded(5);
        for _ in 0..100 {
            let value = fake.decimal(10, 20, 2);
            assert_eq!(value.scale(), 2);
            assert!(value >= Decimal::new(1_000, 2) && value <= Decimal::new(2_000, 2));
        }
    }

    #[test]
    fn unique_values_never_repeat_and_give_up_loudly() {
        let mut fake = Faker::seeded(11);
        let mut values: Vec<u8> = (0..4).map(|_| fake.unique(|f| f.number(1..=4))).collect();
        values.sort_unstable();
        assert_eq!(values, [1, 2, 3, 4]);
        let exhausted = std::panic::catch_unwind(move || fake.unique(|f| f.number(1..=4_u8)));
        assert!(exhausted.is_err());
    }

    #[test]
    fn text_generators_are_well_formed() {
        let mut fake = Faker::seeded(1);
        let sentence = fake.sentence();
        assert!(sentence.ends_with('.'));
        assert!(sentence.chars().next().unwrap().is_uppercase());
        assert!(fake.text(40).len() <= 40);
        assert_eq!(fake.text(3).len(), 3);
        let slug = fake.slug();
        assert!(slug.split('-').count() >= 2);
        assert!(slug.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
        assert_eq!(fake.alphanumeric(12).len(), 12);
        assert!(fake.hex_color().starts_with('#'));
        assert_eq!(fake.country_code().len(), 2);
        assert!(fake.domain().ends_with(".test"));
        assert!(fake.email().contains(".test"));
        assert!(fake.phone().starts_with('+'));
        assert!(fake.ipv4().split('.').count() == 4);
    }

    #[test]
    fn dates_fall_inside_their_window() {
        let mut fake = Faker::seeded(2);
        let start = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let end = start + Duration::days(30);
        for _ in 0..100 {
            let at = fake.date_time_between(start, end);
            assert!(at >= start && at <= end);
            assert_eq!(
                at.timestamp_subsec_nanos() % 1_000,
                0,
                "microsecond precision"
            );
        }
        let day = fake.date_between(
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 1, 3).unwrap(),
        );
        assert!(day.to_string().starts_with("2026-01-0"));
    }

    #[test]
    fn ids_come_from_the_stream() {
        let a = Faker::seeded(8).uuid();
        let b = Faker::seeded(8).uuid();
        assert_eq!(a, b);
        assert_eq!(a.get_version_num(), 4);
        assert!(!Faker::seeded(8).ulid().is_nil());
    }

    #[test]
    fn choices_and_shuffles() {
        let mut fake = Faker::seeded(4);
        let picked = fake.pick_many(&[1, 2, 3, 4, 5], 3);
        assert_eq!(picked.len(), 3);
        let mut sorted = picked.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "distinct items");
        assert_eq!(fake.pick_many(&[1, 2], 5).len(), 2);
        let mut items = [1, 2, 3, 4, 5, 6, 7, 8];
        fake.shuffle(&mut items);
        let mut back = items;
        back.sort_unstable();
        assert_eq!(back, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(fake.optional(100, |f| f.word()).is_some());
        assert!(fake.optional(0, |f| f.word()).is_none());
    }
}
