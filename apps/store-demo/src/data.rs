//! What the shop stores: its products, its customers, their orders with the lines
//! in each, and everything that happened to an order or a product, with a seed
//! that writes the same shop on every run.
//!
//! Every name, address and company here is invented. Addresses are on
//! `example.com`, `example.org` and `example.net`, and card numbers are four
//! digits picked by the seed.

use chrono::{DateTime, NaiveDate};
use navree::orm::Model;
use navree::ui::{Tone, icons};

/// The shop the app runs.
pub const SHOP: &str = "Fernhill Supply";
/// The shop's own time zone, which every page shows its times in.
pub const ZONE: &str = "Europe/Amsterdam";
/// That zone's offset from UTC in November, in seconds.
pub const OFFSET: i32 = 3600;
/// How far back the seeded orders go: a year.
pub const DAYS: i64 = 365;
/// How many orders the seed writes.
pub const ORDERS: i64 = 200_000;
/// How many customers the seed writes.
pub const CUSTOMERS: i64 = 40_000;
/// How many products the shop sells: every item of [`CATALOGUE`] in colours
/// and editions.
pub const PRODUCTS: usize = 2_000;
/// How many orders the seed writes at a time, so the shop never sits in memory
/// whole.
pub const CHUNK: i64 = 2_000;
/// The order that ships for free from this subtotal, in cents.
pub const FREE_SHIPPING: i64 = 10_000;
/// The person running the shop, who issues refunds and restocks.
pub const STAFF: &str = "Ines Varga";

/// A product, as stored. Prices are in cents.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "products")]
pub struct Product {
    pub id: i64,
    pub sku: String,
    pub name: String,
    pub category: String,
    /// The key of its icon in [`product_icon`].
    pub icon: String,
    pub price: i64,
    pub stock: i64,
    /// Stock at or under this is low.
    pub reorder_at: i64,
    /// One of [`Level`], as its key, kept with the stock.
    pub level: String,
}

/// A customer, as stored.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "customers")]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub email: String,
    /// A two-letter country code.
    pub country: String,
    pub city: String,
    pub since: Option<NaiveDate>,
}

/// An order, as stored. Amounts are in cents.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "orders")]
pub struct Order {
    pub id: i64,
    pub number: String,
    pub customer_id: i64,
    pub customer: String,
    pub email: String,
    /// Where it goes: a two-letter country code and a city.
    pub country: String,
    pub city: String,
    /// One of [`Status`], as its key.
    pub status: String,
    /// One of [`Fulfilment`], as its key.
    pub fulfilment: String,
    /// One of [`Payment`], as its key.
    pub payment: String,
    /// `card` or `transfer`.
    pub method: String,
    pub card_brand: String,
    pub card_last4: String,
    /// How many units, across every line.
    pub items: i64,
    pub subtotal: i64,
    pub shipping: i64,
    pub total: i64,
    pub refunded: i64,
    /// The warehouse it ships from, by its country code in [`WAREHOUSES`].
    pub warehouse: String,
    pub carrier: String,
    pub tracking: String,
    /// What the customer wrote at checkout.
    pub note: String,
    /// Unix seconds.
    pub placed_at: i64,
    pub placed_on: NaiveDate,
    /// When the confirmation last went out, in unix seconds.
    pub confirmed_at: i64,
}

/// One line of an order.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "order_lines")]
pub struct Line {
    pub id: i64,
    pub order_id: i64,
    pub product_id: i64,
    pub sku: String,
    pub name: String,
    pub icon: String,
    pub quantity: i64,
    pub price: i64,
}

/// Something that happened to an order or a product.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "events")]
pub struct Event {
    pub id: i64,
    /// One of [`Kind`], as its key.
    pub kind: String,
    /// Unix seconds.
    pub at: i64,
    /// The order, or 0.
    pub order_id: i64,
    /// The product, or 0.
    pub product_id: i64,
    /// An amount in cents, or a number of units for stock.
    pub amount: i64,
    /// Who did it: a customer, the warehouse, a carrier, the staff.
    pub actor: String,
    /// A line of detail: a reason, a tracking number, a product.
    pub detail: String,
}

/// Where an order stands as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Open,
    Completed,
    Cancelled,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Open, Status::Completed, Status::Cancelled];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Completed => "Completed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Open => Tone::Info,
            Self::Completed => Tone::Success,
            Self::Cancelled => Tone::Neutral,
        }
    }

    pub fn parse(key: &str) -> Status {
        Self::ALL
            .into_iter()
            .find(|status| status.key() == key)
            .unwrap_or(Self::Open)
    }
}

/// How far the goods have got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fulfilment {
    Unfulfilled,
    Packed,
    Shipped,
    Delivered,
}

impl Fulfilment {
    pub const ALL: [Fulfilment; 4] = [
        Fulfilment::Unfulfilled,
        Fulfilment::Packed,
        Fulfilment::Shipped,
        Fulfilment::Delivered,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Unfulfilled => "unfulfilled",
            Self::Packed => "packed",
            Self::Shipped => "shipped",
            Self::Delivered => "delivered",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Unfulfilled => "Unfulfilled",
            Self::Packed => "Packed",
            Self::Shipped => "Shipped",
            Self::Delivered => "Delivered",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Unfulfilled => Tone::Warning,
            Self::Packed => Tone::named("cyan"),
            Self::Shipped => Tone::named("violet"),
            Self::Delivered => Tone::Success,
        }
    }

    pub fn parse(key: &str) -> Fulfilment {
        Self::ALL
            .into_iter()
            .find(|step| step.key() == key)
            .unwrap_or(Self::Unfulfilled)
    }
}

/// Where the money stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payment {
    Pending,
    Paid,
    PartiallyRefunded,
    Refunded,
}

impl Payment {
    pub const ALL: [Payment; 4] = [
        Payment::Pending,
        Payment::Paid,
        Payment::PartiallyRefunded,
        Payment::Refunded,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Paid => "paid",
            Self::PartiallyRefunded => "partially_refunded",
            Self::Refunded => "refunded",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Paid => "Paid",
            Self::PartiallyRefunded => "Partly refunded",
            Self::Refunded => "Refunded",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Pending => Tone::Warning,
            Self::Paid => Tone::Success,
            Self::PartiallyRefunded => Tone::named("rose"),
            Self::Refunded => Tone::Neutral,
        }
    }

    pub fn parse(key: &str) -> Payment {
        Self::ALL
            .into_iter()
            .find(|payment| payment.key() == key)
            .unwrap_or(Self::Pending)
    }
}

/// How much of a product is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    InStock,
    Low,
    Out,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::InStock, Level::Low, Level::Out];

    /// The level of `stock` against a reorder point.
    pub const fn of(stock: i64, reorder_at: i64) -> Level {
        if stock <= 0 {
            Self::Out
        } else if stock <= reorder_at {
            Self::Low
        } else {
            Self::InStock
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::InStock => "in_stock",
            Self::Low => "low",
            Self::Out => "out",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::InStock => "In stock",
            Self::Low => "Low stock",
            Self::Out => "Out of stock",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::InStock => Tone::Success,
            Self::Low => Tone::Warning,
            Self::Out => Tone::Danger,
        }
    }

    pub fn parse(key: &str) -> Level {
        Self::ALL
            .into_iter()
            .find(|level| level.key() == key)
            .unwrap_or(Self::InStock)
    }
}

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Placed,
    Paid,
    Packed,
    Shipped,
    Delivered,
    Refunded,
    Cancelled,
    Resent,
    LowStock,
    Restocked,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Placed,
        Kind::Paid,
        Kind::Packed,
        Kind::Shipped,
        Kind::Delivered,
        Kind::Refunded,
        Kind::Cancelled,
        Kind::Resent,
        Kind::LowStock,
        Kind::Restocked,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Placed => "placed",
            Self::Paid => "paid",
            Self::Packed => "packed",
            Self::Shipped => "shipped",
            Self::Delivered => "delivered",
            Self::Refunded => "refunded",
            Self::Cancelled => "cancelled",
            Self::Resent => "resent",
            Self::LowStock => "low_stock",
            Self::Restocked => "restocked",
        }
    }

    pub fn parse(key: &str) -> Option<Kind> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Placed => Tone::Info,
            Self::Paid | Self::Delivered => Tone::Success,
            Self::Packed => Tone::named("cyan"),
            Self::Shipped => Tone::named("violet"),
            Self::Refunded => Tone::named("rose"),
            Self::Cancelled => Tone::Neutral,
            Self::Resent => Tone::Neutral,
            Self::LowStock => Tone::Warning,
            Self::Restocked => Tone::Success,
        }
    }

    pub fn icon(self) -> icons::Icon {
        match self {
            Self::Placed => icons::SHOPPING_BAG,
            Self::Paid => icons::CREDIT_CARD,
            Self::Packed => icons::PACKAGE,
            Self::Shipped => icons::TRUCK,
            Self::Delivered => icons::PACKAGE_CHECK,
            Self::Refunded => icons::UNDO_2,
            Self::Cancelled => icons::BAN,
            Self::Resent => icons::SEND,
            Self::LowStock => icons::TRIANGLE_ALERT,
            Self::Restocked => icons::PACKAGE_PLUS,
        }
    }
}

/// A place orders go to.
pub struct Place {
    pub country: &'static str,
    pub city: &'static str,
}

const fn place(country: &'static str, city: &'static str) -> Place {
    Place { country, city }
}

/// Where customers live.
pub const PLACES: [Place; 30] = [
    place("NL", "Utrecht"),
    place("NL", "Rotterdam"),
    place("BE", "Ghent"),
    place("DE", "Hamburg"),
    place("DE", "Leipzig"),
    place("FR", "Lyon"),
    place("FR", "Nantes"),
    place("GB", "Bristol"),
    place("GB", "Edinburgh"),
    place("IE", "Galway"),
    place("ES", "Valencia"),
    place("PT", "Porto"),
    place("IT", "Bologna"),
    place("AT", "Graz"),
    place("DK", "Aarhus"),
    place("SE", "Gothenburg"),
    place("NO", "Bergen"),
    place("FI", "Tampere"),
    place("PL", "Wrocław"),
    place("CZ", "Brno"),
    place("CH", "Lausanne"),
    place("US", "Portland"),
    place("US", "Minneapolis"),
    place("CA", "Halifax"),
    place("MX", "Guadalajara"),
    place("AR", "Buenos Aires"),
    place("AU", "Hobart"),
    place("NZ", "Christchurch"),
    place("JP", "Sapporo"),
    place("ZA", "Stellenbosch"),
];

/// A warehouse: where it is, and the carrier that collects from it.
pub struct Warehouse {
    pub country: &'static str,
    pub city: &'static str,
    pub carrier: &'static str,
}

/// The shop's warehouses, each serving its part of the world.
pub const WAREHOUSES: [Warehouse; 3] = [
    Warehouse {
        country: "NL",
        city: "Rotterdam",
        carrier: "Parcelway",
    },
    Warehouse {
        country: "US",
        city: "Columbus",
        carrier: "Northline Freight",
    },
    Warehouse {
        country: "AU",
        city: "Melbourne",
        carrier: "Kestrel Post",
    },
];

/// The warehouse that serves `country`.
pub fn warehouse_for(country: &str) -> &'static Warehouse {
    match country {
        "US" | "CA" | "MX" | "AR" => &WAREHOUSES[1],
        "AU" | "NZ" | "JP" => &WAREHOUSES[2],
        _ => &WAREHOUSES[0],
    }
}

/// The warehouse with this country code.
pub fn warehouse(code: &str) -> &'static Warehouse {
    WAREHOUSES
        .iter()
        .find(|warehouse| warehouse.country == code)
        .unwrap_or(&WAREHOUSES[0])
}

/// Shipping for a subtotal to a country, in cents.
pub fn shipping_for(subtotal: i64, country: &str) -> i64 {
    if subtotal >= FREE_SHIPPING {
        0
    } else if warehouse_for(country).country == "NL" {
        690
    } else {
        1_490
    }
}

/// A product the shop sells: its SKU, name, category, icon, price in cents and
/// the stock it reorders at.
pub struct Stock {
    pub sku: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub icon: &'static str,
    pub price: i64,
    pub reorder_at: i64,
}

const fn stock(
    sku: &'static str,
    name: &'static str,
    category: &'static str,
    icon: &'static str,
    price: i64,
    reorder_at: i64,
) -> Stock {
    Stock {
        sku,
        name,
        category,
        icon,
        price,
        reorder_at,
    }
}

/// What the shop sells, before colours and editions.
pub const CATALOGUE: [Stock; 24] = [
    stock("KIT-101", "Enamel camp mug", "Kitchen", "mug", 1_800, 25),
    stock(
        "KIT-102",
        "Stoneware pour-over set",
        "Kitchen",
        "pour",
        4_600,
        12,
    ),
    stock(
        "HOM-201",
        "Cedar and smoke candle",
        "Home",
        "candle",
        2_400,
        20,
    ),
    stock("HOM-202", "Brass desk lamp", "Home", "lamp", 12_800, 6),
    stock(
        "STA-301",
        "Field notebooks, three",
        "Stationery",
        "notebook",
        2_100,
        30,
    ),
    stock(
        "BAG-401",
        "Waxed canvas backpack",
        "Bags",
        "backpack",
        14_900,
        8,
    ),
    stock(
        "APP-501",
        "Merino crew sweater",
        "Apparel",
        "sweater",
        9_600,
        10,
    ),
    stock(
        "OUT-601",
        "Two-person trail tent",
        "Outdoor",
        "tent",
        38_900,
        4,
    ),
    stock(
        "OUT-602",
        "Brass pocket compass",
        "Outdoor",
        "compass",
        5_400,
        10,
    ),
    stock(
        "GAR-701",
        "Herb garden starter kit",
        "Garden",
        "sprout",
        3_200,
        15,
    ),
    stock(
        "OUT-603",
        "Insulated trail flask",
        "Outdoor",
        "flask",
        3_800,
        18,
    ),
    stock(
        "KIT-103",
        "Linen kitchen apron",
        "Kitchen",
        "apron",
        4_400,
        10,
    ),
    stock(
        "HOM-203",
        "Wool picnic blanket",
        "Home",
        "blanket",
        11_800,
        6,
    ),
    stock(
        "BAG-402",
        "Leather card wallet",
        "Bags",
        "wallet",
        5_800,
        12,
    ),
    stock(
        "AUD-801",
        "Over-ear headphones",
        "Audio",
        "headphones",
        17_900,
        5,
    ),
    stock(
        "APP-502",
        "Field watch, 38 mm",
        "Apparel",
        "watch",
        21_000,
        4,
    ),
    stock(
        "HOM-204",
        "Instant film camera",
        "Home",
        "camera",
        13_900,
        5,
    ),
    stock(
        "GAR-702",
        "Folding garden shears",
        "Garden",
        "shears",
        2_900,
        15,
    ),
    stock(
        "KIT-104",
        "Glass water carafe",
        "Kitchen",
        "carafe",
        3_400,
        12,
    ),
    stock(
        "HOM-205",
        "Pressed leaf prints, four",
        "Home",
        "leaf",
        6_200,
        8,
    ),
    stock(
        "OUT-604",
        "Commuter bike lights",
        "Outdoor",
        "bike",
        4_500,
        14,
    ),
    stock("BAG-403", "Market tote, natural", "Bags", "tote", 2_200, 30),
    stock("OUT-605", "Fly-tying kit", "Outdoor", "fish", 7_400, 6),
    stock(
        "HOM-206",
        "Storage crates, three",
        "Home",
        "crates",
        6_600,
        8,
    ),
];

/// The colours each item comes in.
const COLOURS: [&str; 12] = [
    "sage", "rust", "charcoal", "oat", "indigo", "moss", "clay", "slate", "sand", "ochre", "pine",
    "plum",
];

/// The editions each colour comes in, with what each does to the price, in
/// percent.
const EDITIONS: [(&str, i64); 7] = [
    ("", 100),
    ("small", 85),
    ("large", 120),
    ("twin pack", 180),
    ("gift set", 140),
    ("travel", 90),
    ("studio", 130),
];

/// The categories, for the inventory's filter.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("Apparel", "Apparel"),
    ("Audio", "Audio"),
    ("Bags", "Bags"),
    ("Garden", "Garden"),
    ("Home", "Home"),
    ("Kitchen", "Kitchen"),
    ("Outdoor", "Outdoor"),
    ("Stationery", "Stationery"),
];

/// The icon a product is drawn with, by its key.
pub fn product_icon(key: &str) -> icons::Icon {
    match key {
        "mug" => icons::COFFEE,
        "pour" => icons::GLASS_WATER,
        "candle" => icons::FLAME,
        "lamp" => icons::LAMP_DESK,
        "notebook" => icons::NOTEBOOK,
        "backpack" => icons::BACKPACK,
        "sweater" => icons::SHIRT,
        "tent" => icons::TENT,
        "compass" => icons::COMPASS,
        "sprout" => icons::SPROUT,
        "flask" => icons::CUP_SODA,
        "apron" => icons::UTENSILS_CROSSED,
        "blanket" => icons::SUN,
        "wallet" => icons::WALLET,
        "headphones" => icons::HEADPHONES,
        "watch" => icons::WATCH,
        "camera" => icons::CAMERA,
        "shears" => icons::SCISSORS,
        "carafe" => icons::DROPLET,
        "leaf" => icons::LEAF,
        "bike" => icons::BIKE,
        "tote" => icons::SHOPPING_BAG,
        "fish" => icons::FISH,
        _ => icons::BOX,
    }
}

const FIRST: [&str; 36] = [
    "Amélie", "Bram", "Chiara", "Daan", "Elif", "Finn", "Greta", "Hugo", "Ilse", "Jonah", "Kaia",
    "Lars", "Marisol", "Niko", "Olga", "Pieter", "Quinn", "Rosa", "Sven", "Tamsin", "Udo", "Vera",
    "Wim", "Ximena", "Yara", "Zeno", "Anouk", "Björn", "Carys", "Dario", "Esme", "Fenna", "Gael",
    "Hanne", "Ivo", "Juno",
];

const LAST: [&str; 36] = [
    "Laurent",
    "de Vries",
    "Moretti",
    "Jansen",
    "Aydın",
    "Brennan",
    "Holm",
    "Marchetti",
    "Visser",
    "Okoye",
    "Lindgren",
    "Sato",
    "Ortega",
    "Kowalski",
    "Petrova",
    "Bakker",
    "Fairweather",
    "Castell",
    "Nyström",
    "Pryce",
    "Hartmann",
    "Novák",
    "Smit",
    "Reyes",
    "Haddad",
    "Ferraro",
    "Dekker",
    "Strand",
    "Llewellyn",
    "Conti",
    "Whitlock",
    "Mulder",
    "Rivas",
    "Berg",
    "Vos",
    "Takeda",
];

const DOMAINS: [&str; 3] = ["example.com", "example.org", "example.net"];

/// What customers write at checkout, now and then.
const NOTES: [&str; 8] = [
    "Please leave it with the neighbour at number 12 if nobody is home.",
    "It's a gift: please leave the receipt out of the box.",
    "The bell is quiet, so knock twice.",
    "Deliver to the side door by the garage, please.",
    "Could the candle be wrapped separately? It's for my sister.",
    "No rush, I'm away until Friday.",
    "Please use as little plastic as you can.",
    "Office delivery: reception is open from 9 to 5.",
];

const BRANDS: [&str; 3] = ["Visa", "Mastercard", "Amex"];

/// A small, fast generator: the same numbers from the same seed on every run.
#[derive(Clone, Debug)]
pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    /// The next number (SplitMix64).
    pub fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.draw() % n.max(1)
    }

    /// An index into a list of `len`.
    pub fn pick(&mut self, len: usize) -> usize {
        usize::try_from(self.below(len as u64)).unwrap_or(0)
    }

    /// A number in `low..high`, as seconds or cents.
    pub fn between(&mut self, low: i64, high: i64) -> i64 {
        low + i64::try_from(self.below(u64::try_from((high - low).max(1)).unwrap_or(1)))
            .unwrap_or(0)
    }

    /// True `percent` times in a hundred.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// `Amélie` as `amelie`, for an address.
fn ascii(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'ı' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ý' => 'y',
            'ř' => 'r',
            other => other.to_ascii_lowercase(),
        })
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// The date of a unix time, in the shop's zone.
pub fn day_of(at: i64) -> NaiveDate {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0)
        .map(|time| time.date_naive())
        .unwrap_or_default()
}

/// `FH-20481` for order 1.
pub fn order_number(id: i64) -> String {
    format!("FH-{}", 20_480 + id)
}

/// A tracking number for an order shipped by a carrier.
pub fn tracking(order: &Order, dice: &mut Dice) -> String {
    let prefix = match warehouse(&order.warehouse).carrier {
        "Northline Freight" => "NL",
        "Kestrel Post" => "KP",
        _ => "PW",
    };
    format!(
        "{prefix} {:04} {:04} {}",
        dice.below(10_000),
        dice.below(10_000),
        order.country
    )
}

/// One product of the shop, by its index from 0: the item, colour and edition
/// vary so that the first few hundred products cover every item.
pub fn product(at: usize) -> Product {
    let item = &CATALOGUE[at % CATALOGUE.len()];
    let colour = COLOURS[(at / CATALOGUE.len()) % COLOURS.len()];
    let (edition, percent) = EDITIONS[(at / (CATALOGUE.len() * COLOURS.len())) % EDITIONS.len()];
    let mut dice = Dice::new(u64::try_from(at).unwrap_or(0) * 31 + 7);
    let price = (item.price * percent / 100 / 100) * 100;
    let reorder_at = (item.reorder_at * 100 / percent).max(2);
    // Most are comfortably stocked; about one in thirty is low and one in a
    // hundred and fifty has run out.
    let stock = match dice.below(150) {
        0 => 0,
        1..=4 => dice.between(1, reorder_at + 1),
        _ => reorder_at + dice.between(4, 160),
    };
    let name = if edition.is_empty() {
        format!("{}, {colour}", item.name)
    } else {
        format!("{}, {colour}, {edition}", item.name)
    };
    let sku = format!(
        "{}-{:02}{}",
        item.sku,
        (at / CATALOGUE.len()) % COLOURS.len(),
        at / (CATALOGUE.len() * COLOURS.len())
    );
    Product {
        id: i64::try_from(at).unwrap_or(0) + 1,
        sku,
        name,
        category: item.category.to_owned(),
        icon: item.icon.to_owned(),
        price,
        stock,
        reorder_at,
        level: Level::of(stock, reorder_at).key().to_owned(),
    }
}

/// The products as they stand: the stock each holds now.
pub fn products() -> Vec<Product> {
    (0..PRODUCTS).map(product).collect()
}

/// One customer, by id from 1.
pub fn customer(id: i64, now: i64) -> Customer {
    let n = usize::try_from(id - 1).unwrap_or(0);
    let mut dice = Dice::new(u64::try_from(id).unwrap_or(0) * 104_729);
    let names = FIRST.len() * LAST.len();
    let (first, last) = (
        FIRST[n % FIRST.len()],
        LAST[(n / FIRST.len() + n * 7) % LAST.len()],
    );
    // Namesakes are told apart by a number in their address.
    let round = n / names;
    let place = &PLACES[dice.pick(PLACES.len())];
    Customer {
        id,
        name: format!("{first} {last}"),
        email: if round == 0 {
            format!("{}.{}@{}", ascii(first), ascii(last), DOMAINS[n % 3])
        } else {
            format!("{}.{}{round}@{}", ascii(first), ascii(last), DOMAINS[n % 3])
        },
        country: place.country.to_owned(),
        city: place.city.to_owned(),
        since: Some(day_of(now - dice.between(70, 3 * 365) * 86_400)),
    }
}

/// The customers.
pub fn customers(now: i64) -> Vec<Customer> {
    (1..=CUSTOMERS).map(|id| customer(id, now)).collect()
}

/// When order `id` was placed, for a shop whose orders `now` come in twice as
/// fast as on average: the newest a few minutes before `now`.
pub fn placed_at(id: i64, now: i64) -> i64 {
    let start = now - DAYS * 86_400;
    let span = now - 240 - start;
    let fraction = id as f64 / ORDERS as f64;
    start + (span as f64 * fraction.sqrt()) as i64
}

/// Orders `from` to `to` (inclusive) as they stand at `now`, with their lines
/// and events. The same ids at the same `now` give the same orders, so the shop
/// is written a chunk at a time and a preview can draw only the newest.
pub fn orders(
    from: i64,
    to: i64,
    now: i64,
    products: &[Product],
    customers: &[Customer],
) -> (Vec<Order>, Vec<Line>, Vec<Event>) {
    let mut orders = Vec::new();
    let mut lines = Vec::new();
    let mut events = Vec::new();
    for id in from.max(1)..=to.min(ORDERS) {
        let (order, mut its_lines, mut its_events) =
            one_order(id, placed_at(id, now), now, products, customers);
        orders.push(order);
        lines.append(&mut its_lines);
        events.append(&mut its_events);
    }
    (orders, lines, events)
}

/// The id of the first line of order `id`: every order before it has at most
/// four lines, so ids never collide across chunks.
pub fn first_line(id: i64) -> i64 {
    (id - 1) * 4 + 1
}

/// The events the shelves raised: each low or empty product said so, lately.
pub fn stock_events(products: &[Product], now: i64) -> Vec<Event> {
    products
        .iter()
        .filter(|product| product.level != Level::InStock.key())
        .map(|product| Event {
            kind: Kind::LowStock.key().to_owned(),
            at: now - 600 - (product.id * 977) % (20 * 3600),
            product_id: product.id,
            amount: product.stock,
            actor: product.name.clone(),
            detail: product.sku.clone(),
            ..Event::default()
        })
        .collect()
}

/// The dice an order is rolled with: the first roll picks its customer.
fn order_dice(id: i64) -> Dice {
    Dice::new(u64::try_from(id).unwrap_or(0) * 7_919 + 17)
}

/// The customer of seeded order `id`, by their id, without rolling the rest of
/// the order.
pub fn customer_of(id: i64) -> i64 {
    let mut dice = order_dice(id);
    i64::try_from(dice.pick(usize::try_from(CUSTOMERS).unwrap_or(1))).unwrap_or(0) + 1
}

/// One order placed at `placed_at`, with its lines and how far it got by `now`.
pub fn one_order(
    id: i64,
    placed_at: i64,
    now: i64,
    products: &[Product],
    customers: &[Customer],
) -> (Order, Vec<Line>, Vec<Event>) {
    let mut dice = order_dice(id);
    let customer = &customers[dice.pick(customers.len())];
    let mut its_lines: Vec<Line> = Vec::new();
    let count = match dice.below(10) {
        0..=4 => 1,
        5..=7 => 2,
        8 => 3,
        _ => 4,
    };
    for _ in 0..count {
        // A few hundred products sell most: the lower the index, the likelier.
        let a = dice.pick(products.len());
        let b = dice.pick(products.len());
        let c = dice.pick(products.len());
        let product = &products[a.min(b).min(c)];
        if its_lines.iter().any(|line| line.product_id == product.id) {
            continue;
        }
        its_lines.push(Line {
            id: 0,
            order_id: id,
            product_id: product.id,
            sku: product.sku.clone(),
            name: product.name.clone(),
            icon: product.icon.clone(),
            quantity: if dice.chance(80) {
                1
            } else {
                dice.between(2, 4)
            },
            price: product.price,
        });
    }
    for (at, line) in its_lines.iter_mut().enumerate() {
        line.id = first_line(id) + i64::try_from(at).unwrap_or(0);
    }
    let subtotal: i64 = its_lines
        .iter()
        .map(|line| line.price * line.quantity)
        .sum();
    let shipping = shipping_for(subtotal, &customer.country);
    let card = dice.chance(88);
    let mut order = Order {
        id,
        number: order_number(id),
        customer_id: customer.id,
        customer: customer.name.clone(),
        email: customer.email.clone(),
        country: customer.country.clone(),
        city: customer.city.clone(),
        status: Status::Open.key().to_owned(),
        fulfilment: Fulfilment::Unfulfilled.key().to_owned(),
        payment: Payment::Pending.key().to_owned(),
        method: if card { "card" } else { "transfer" }.to_owned(),
        card_brand: if card {
            BRANDS[dice.pick(3)].to_owned()
        } else {
            String::new()
        },
        card_last4: if card {
            format!("{:04}", dice.below(10_000))
        } else {
            String::new()
        },
        items: its_lines.iter().map(|line| line.quantity).sum(),
        subtotal,
        shipping,
        total: subtotal + shipping,
        refunded: 0,
        warehouse: warehouse_for(&customer.country).country.to_owned(),
        carrier: warehouse_for(&customer.country).carrier.to_owned(),
        tracking: String::new(),
        note: if dice.chance(18) {
            NOTES[dice.pick(NOTES.len())].to_owned()
        } else {
            String::new()
        },
        placed_at,
        placed_on: day_of(placed_at),
        confirmed_at: placed_at + 4,
    };
    let mut its_events = Vec::new();
    let mut push = |kind: Kind, at: i64, amount: i64, actor: &str, detail: &str| {
        if at <= now - 20 {
            its_events.push(Event {
                id: 0,
                kind: kind.key().to_owned(),
                at,
                order_id: id,
                product_id: 0,
                amount,
                actor: actor.to_owned(),
                detail: detail.to_owned(),
            });
        }
    };
    push(Kind::Placed, placed_at, order.total, &order.customer, "");
    // A parcel to the warehouse's own neighbours arrives in a day or two.
    let far = !matches!(order.country.as_str(), "NL" | "BE" | "DE" | "US" | "AU");
    // Paid at once by card; by transfer some hours later.
    let paid_at = if card {
        placed_at + dice.between(3, 40)
    } else {
        placed_at + dice.between(2 * 3600, 30 * 3600)
    };
    let packed_at = paid_at + dice.between(40 * 60, 18 * 3600);
    let shipped_at = packed_at + dice.between(2 * 3600, 20 * 3600);
    let delivered_at = shipped_at
        + if far {
            dice.between(3 * 86_400, 7 * 86_400)
        } else {
            dice.between(20 * 3600, 3 * 86_400)
        };
    let cancelled = dice.chance(3) && now - placed_at > 6 * 3600;
    let house = warehouse(&order.warehouse);
    if paid_at <= now {
        push(Kind::Paid, paid_at, order.total, &order.customer, "");
        order.payment = Payment::Paid.key().to_owned();
    }
    if cancelled {
        let at = placed_at + dice.between(15 * 60, 5 * 3600);
        push(
            Kind::Cancelled,
            at,
            0,
            STAFF,
            "The customer asked to cancel",
        );
        order.status = Status::Cancelled.key().to_owned();
        if paid_at <= at {
            push(Kind::Refunded, at + 30, order.total, STAFF, "Cancelled");
            order.refunded = order.total;
            order.payment = Payment::Refunded.key().to_owned();
        } else {
            order.payment = Payment::Pending.key().to_owned();
        }
        return (order, its_lines, its_events);
    }
    if packed_at <= now && order.payment == Payment::Paid.key() {
        push(Kind::Packed, packed_at, order.items, house.city, "");
        order.fulfilment = Fulfilment::Packed.key().to_owned();
    }
    if shipped_at <= now && order.fulfilment == Fulfilment::Packed.key() {
        order.tracking = tracking(&order, &mut dice);
        push(Kind::Shipped, shipped_at, 0, house.city, &order.tracking);
        order.fulfilment = Fulfilment::Shipped.key().to_owned();
    }
    if delivered_at <= now && order.fulfilment == Fulfilment::Shipped.key() {
        push(Kind::Delivered, delivered_at, 0, house.carrier, "");
        order.fulfilment = Fulfilment::Delivered.key().to_owned();
        order.status = Status::Completed.key().to_owned();
        // Now and then something comes back, or arrives broken.
        if dice.chance(5) {
            let at = delivered_at + dice.between(86_400, 6 * 86_400);
            let full = dice.chance(40);
            let amount = if full {
                order.total
            } else {
                its_lines
                    .first()
                    .map_or(order.total / 2, |line| line.price.min(order.total / 2))
            };
            if at <= now - 20 {
                push(
                    Kind::Refunded,
                    at,
                    amount,
                    STAFF,
                    if full {
                        "Returned in full"
                    } else {
                        "Arrived damaged"
                    },
                );
                order.refunded = amount;
                order.payment = if full {
                    Payment::Refunded
                } else {
                    Payment::PartiallyRefunded
                }
                .key()
                .to_owned();
            }
        }
    }
    (order, its_lines, its_events)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_793_793_600;

    fn newest(n: i64) -> (Vec<Order>, Vec<Line>, Vec<Event>) {
        orders(ORDERS - n + 1, ORDERS, NOW, &products(), &customers(NOW))
    }

    #[test]
    fn the_seed_is_the_same_on_every_run() {
        let (a, _, x) = newest(300);
        let (b, _, y) = newest(300);
        assert_eq!(a.len(), 300);
        assert_eq!(a[17].customer, b[17].customer);
        assert_eq!(x.len(), y.len());
        assert_eq!(x[200].at, y[200].at);
        // A chunk is the same orders as the whole.
        let (c, _, _) = orders(
            ORDERS - 299,
            ORDERS - 200,
            NOW,
            &products(),
            &customers(NOW),
        );
        assert_eq!(c[0].number, a[0].number);
        assert_eq!(c[99].total, a[99].total);
    }

    #[test]
    fn every_person_is_invented_and_reachable() {
        let people = customers(NOW);
        assert_eq!(people.len(), 40_000);
        let mut emails: Vec<&str> = people.iter().map(|c| c.email.as_str()).collect();
        emails.sort_unstable();
        emails.dedup();
        assert_eq!(emails.len(), 40_000, "one address each");
        for customer in &people {
            assert!(
                DOMAINS
                    .iter()
                    .any(|domain| customer.email.ends_with(&format!("@{domain}"))),
                "{}",
                customer.email
            );
        }
    }

    #[test]
    fn an_orders_customer_is_known_without_the_order() {
        let (orders, _, _) = newest(50);
        for order in &orders {
            assert_eq!(customer_of(order.id), order.customer_id);
        }
    }

    #[test]
    fn two_thousand_products_each_with_its_own_sku() {
        let all = products();
        assert_eq!(all.len(), 2_000);
        let mut skus: Vec<&str> = all.iter().map(|p| p.sku.as_str()).collect();
        skus.sort_unstable();
        skus.dedup();
        assert_eq!(skus.len(), 2_000);
        assert!(all.iter().all(|p| p.price > 0 && p.reorder_at >= 2));
        let low = all
            .iter()
            .filter(|p| p.level != Level::InStock.key())
            .count();
        assert!((20..120).contains(&low), "{low} low or out");
    }

    #[test]
    fn every_order_adds_up() {
        let (orders, lines, events) = newest(4_000);
        let mut ids: Vec<i64> = lines.iter().map(|line| line.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), lines.len(), "line ids are unique");
        for order in &orders {
            let mine: Vec<&Line> = lines
                .iter()
                .filter(|line| line.order_id == order.id)
                .collect();
            assert!(!mine.is_empty(), "{} has lines", order.number);
            let subtotal: i64 = mine.iter().map(|line| line.price * line.quantity).sum();
            assert_eq!(order.subtotal, subtotal, "{}", order.number);
            assert_eq!(order.total, order.subtotal + order.shipping);
            assert!(order.refunded >= 0 && order.refunded <= order.total);
            assert!(order.placed_at < NOW);
            match Payment::parse(&order.payment) {
                Payment::Refunded => assert_eq!(order.refunded, order.total),
                Payment::PartiallyRefunded => {
                    assert!(order.refunded > 0 && order.refunded < order.total);
                }
                _ => assert_eq!(order.refunded, 0),
            }
            // Nothing ships before it's paid, and a cancelled order never ships.
            if Fulfilment::parse(&order.fulfilment) >= Fulfilment::Packed {
                assert_ne!(Payment::parse(&order.payment), Payment::Pending);
                assert_ne!(Status::parse(&order.status), Status::Cancelled);
            }
        }
        assert!(events.iter().all(|event| event.at <= NOW));
    }

    #[test]
    fn the_numbers_look_like_a_shop() {
        let (orders, _, _) = newest(4_000);
        let today = day_of(NOW);
        let placed_today = orders.iter().filter(|o| o.placed_on == today).count();
        assert!((300..900).contains(&placed_today), "{placed_today} today");
        assert!(orders.iter().any(|o| o.status == Status::Cancelled.key()));
        assert!(
            orders
                .iter()
                .filter(|o| o.status == Status::Open.key())
                .count()
                > 200
        );
        // Refunds come days after delivery: look a few months back for them.
        let (older, _, _) = super::orders(150_001, 154_000, NOW, &products(), &customers(NOW));
        assert!(
            older
                .iter()
                .any(|o| o.payment == Payment::PartiallyRefunded.key())
        );
        assert!(older.iter().any(|o| o.payment == Payment::Refunded.key()));
        assert!(orders.iter().any(|o| o.city == "Buenos Aires"));
        // A year back, the first orders are a year old.
        assert!(placed_at(1, NOW) < NOW - 360 * 86_400);
    }
}
