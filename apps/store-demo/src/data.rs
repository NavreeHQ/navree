//! What the shop stores: its products, its customers, their orders with the lines
//! in each, and everything that happened to an order or a product, with a seed
//! that writes the same shop on every run.
//!
//! Every name, address, business and number here is invented. Addresses are on
//! `example.com`, `example.org` and `example.net`, phone numbers follow one
//! made-up pattern (`+91 98765 0xxxx`), and card numbers are four digits picked by
//! the seed.

use chrono::{DateTime, NaiveDate};
use navree::orm::Model;
use navree::ui::{Tone, icons};

/// The shop the app runs.
pub const SHOP: &str = "Charkha Supply Co.";
/// The shop's own time zone, which every page shows its times in.
pub const ZONE: &str = "Asia/Kolkata";
/// That zone's offset from UTC, in seconds.
pub const OFFSET: i32 = 19_800;
/// How far back the seeded orders go: a year.
pub const DAYS: i64 = 365;
/// How many orders the seed writes.
pub const ORDERS: i64 = 200_000;
/// How many customers the seed writes.
pub const CUSTOMERS: i64 = 40_000;
/// How many products the shop sells: every item of [`CATALOGUE`] from several
/// makers' towns and in several editions.
pub const PRODUCTS: usize = 2_000;
/// How many orders the seed writes at a time, so the shop never sits in memory
/// whole.
pub const CHUNK: i64 = 2_000;
/// An order ships for free from this subtotal, in paise: ₹999.
pub const FREE_SHIPPING: i64 = 99_900;
/// The person running the shop, who issues refunds and restocks.
pub const STAFF: &str = "Ravneet Kaur";

/// A product, as stored. Prices are in paise.
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
    /// In E.164: `+919876501234`.
    pub phone: String,
    /// A two-letter country code: `IN`, now and then another.
    pub country: String,
    /// A state's code in [`STATES`], or `EX` outside India.
    pub state: String,
    pub city: String,
    /// The PIN code, or a postcode outside India.
    pub pin: String,
    pub since: Option<NaiveDate>,
}

/// An order, as stored. Amounts are in paise.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "orders")]
pub struct Order {
    pub id: i64,
    pub number: String,
    pub customer_id: i64,
    pub customer: String,
    pub email: String,
    /// Where it goes: a country, a state, a city and its PIN code.
    pub country: String,
    pub state: String,
    pub city: String,
    pub pin: String,
    /// One of [`Status`], as its key.
    pub status: String,
    /// One of [`Fulfilment`], as its key.
    pub fulfilment: String,
    /// One of [`Payment`], as its key.
    pub payment: String,
    /// `upi`, `card` or `cod` (cash on delivery, paid when it arrives).
    pub method: String,
    pub card_brand: String,
    pub card_last4: String,
    /// How many units, across every line.
    pub items: i64,
    pub subtotal: i64,
    pub shipping: i64,
    pub total: i64,
    pub refunded: i64,
    /// The warehouse it ships from, by its code in [`WAREHOUSES`].
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
    /// An amount in paise, or a number of units for stock.
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

/// A place orders go to: a country, a state, a city and the first PIN code of its
/// range.
pub struct Place {
    pub country: &'static str,
    pub state: &'static str,
    pub city: &'static str,
    pub pin: u32,
}

const fn place(state: &'static str, city: &'static str, pin: u32) -> Place {
    Place {
        country: "IN",
        state,
        city,
        pin,
    }
}

const fn abroad(country: &'static str, city: &'static str) -> Place {
    Place {
        country,
        state: "EX",
        city,
        pin: 0,
    }
}

/// Where customers in India live: mostly Punjab and the north, some further away.
pub const PLACES: [Place; 32] = [
    place("PB", "Ludhiana", 141_001),
    place("PB", "Ludhiana", 141_001),
    place("PB", "Amritsar", 143_001),
    place("PB", "Amritsar", 143_001),
    place("PB", "Jalandhar", 144_001),
    place("PB", "Jalandhar", 144_001),
    place("PB", "Patiala", 147_001),
    place("PB", "Bathinda", 151_001),
    place("PB", "Mohali", 160_055),
    place("PB", "Moga", 142_001),
    place("PB", "Hoshiarpur", 146_001),
    place("CH", "Chandigarh", 160_017),
    place("CH", "Chandigarh", 160_017),
    place("HR", "Panchkula", 134_109),
    place("HR", "Gurugram", 122_001),
    place("HR", "Karnal", 132_001),
    place("DL", "Delhi", 110_001),
    place("DL", "Delhi", 110_017),
    place("DL", "Delhi", 110_024),
    place("UP", "Noida", 201_301),
    place("UP", "Lucknow", 226_001),
    place("UP", "Varanasi", 221_001),
    place("UP", "Kanpur", 208_001),
    place("RJ", "Jaipur", 302_001),
    place("UK", "Dehradun", 248_001),
    place("HP", "Shimla", 171_001),
    place("HP", "Dharamshala", 176_215),
    place("JK", "Jammu", 180_001),
    place("MH", "Mumbai", 400_050),
    place("MH", "Pune", 411_001),
    place("KA", "Bengaluru", 560_034),
    place("GJ", "Ahmedabad", 380_009),
];

/// Where the few orders that go abroad go: about one in fifty.
pub const ABROAD: [Place; 4] = [
    abroad("AE", "Dubai"),
    abroad("CA", "Brampton"),
    abroad("GB", "Leicester"),
    abroad("AU", "Melbourne"),
];

/// The states the shop ships to, for the orders' filter, and `EX` for abroad.
pub const STATES: &[(&str, &str)] = &[
    ("PB", "Punjab"),
    ("CH", "Chandigarh"),
    ("HR", "Haryana"),
    ("DL", "Delhi"),
    ("UP", "Uttar Pradesh"),
    ("RJ", "Rajasthan"),
    ("UK", "Uttarakhand"),
    ("HP", "Himachal Pradesh"),
    ("JK", "Jammu and Kashmir"),
    ("MH", "Maharashtra"),
    ("KA", "Karnataka"),
    ("GJ", "Gujarat"),
    ("EX", "Outside India"),
];

/// `Punjab` for `PB`.
pub fn state_name(code: &str) -> &'static str {
    STATES
        .iter()
        .find(|(key, _)| *key == code)
        .map_or("", |(_, name)| *name)
}

/// A warehouse: its code, where it is, and the courier that collects from it.
pub struct Warehouse {
    pub code: &'static str,
    pub state: &'static str,
    pub city: &'static str,
    pub carrier: &'static str,
}

/// The shop's warehouses, each serving its part of the country.
pub const WAREHOUSES: [Warehouse; 3] = [
    Warehouse {
        code: "LDH",
        state: "PB",
        city: "Ludhiana",
        carrier: "Doaba Express",
    },
    Warehouse {
        code: "DEL",
        state: "DL",
        city: "Delhi",
        carrier: "Yamuna Couriers",
    },
    Warehouse {
        code: "BLR",
        state: "KA",
        city: "Bengaluru",
        carrier: "Kaveri Logistics",
    },
];

/// The warehouse that serves a state. Orders abroad leave from Delhi.
pub fn warehouse_for(state: &str) -> &'static Warehouse {
    match state {
        "PB" | "CH" | "HP" | "JK" => &WAREHOUSES[0],
        "MH" | "KA" | "GJ" => &WAREHOUSES[2],
        _ => &WAREHOUSES[1],
    }
}

/// The warehouse with this code.
pub fn warehouse(code: &str) -> &'static Warehouse {
    WAREHOUSES
        .iter()
        .find(|warehouse| warehouse.code == code)
        .unwrap_or(&WAREHOUSES[0])
}

/// Shipping for a subtotal to a state, in paise: free from ₹999 in India, ₹79
/// within a warehouse's own state, ₹129 across India and ₹1,499 abroad.
pub fn shipping_for(subtotal: i64, state: &str) -> i64 {
    if state == "EX" {
        149_900
    } else if subtotal >= FREE_SHIPPING {
        0
    } else if warehouse_for(state).state == state {
        7_900
    } else {
        12_900
    }
}

/// A product the shop sells: its SKU, name, category, icon, price in paise and
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

/// What the shop sells, before makers' towns and editions.
pub const CATALOGUE: [Stock; 24] = [
    stock(
        "CLO-101",
        "Phulkari dupatta",
        "Clothing",
        "dupatta",
        149_900,
        12,
    ),
    stock("FTW-201", "Punjabi jutti", "Footwear", "jutti", 129_900, 10),
    stock("KIT-301", "Brass lota", "Kitchen", "lota", 89_900, 15),
    stock(
        "KIT-302",
        "Steel tiffin, three tiers",
        "Kitchen",
        "tiffin",
        74_900,
        20,
    ),
    stock("KIT-303", "Iron kadhai", "Kitchen", "kadhai", 119_900, 10),
    stock("PAN-401", "Desi ghee, 1 kg", "Pantry", "ghee", 69_900, 30),
    stock(
        "PAN-402",
        "Basmati rice, 5 kg",
        "Pantry",
        "rice",
        64_900,
        25,
    ),
    stock("CLO-102", "Pashmina shawl", "Clothing", "shawl", 699_900, 4),
    stock("CLO-103", "Khadi kurta", "Clothing", "kurta", 159_900, 10),
    stock("KIT-304", "Masala dabba", "Kitchen", "dabba", 99_900, 12),
    stock(
        "KIT-305",
        "Copper bottle, 1 litre",
        "Kitchen",
        "bottle",
        84_900,
        18,
    ),
    stock(
        "CLO-104",
        "Patiala salwar",
        "Clothing",
        "salwar",
        109_900,
        10,
    ),
    stock("HOM-501", "Cotton dhurrie", "Home", "dhurrie", 189_900, 6),
    stock(
        "FES-601",
        "Terracotta diyas, four",
        "Festive",
        "diya",
        34_900,
        40,
    ),
    stock(
        "KIT-306",
        "Kulhad cups, six",
        "Kitchen",
        "kulhad",
        29_900,
        30,
    ),
    stock(
        "HOM-502",
        "Handloom bedsheet",
        "Home",
        "bedsheet",
        149_900,
        8,
    ),
    stock(
        "FES-602",
        "Brass diya stand",
        "Festive",
        "stand",
        129_900,
        6,
    ),
    stock("KIT-307", "Chakla belan", "Kitchen", "belan", 54_900, 14),
    stock(
        "PAN-403",
        "Mustard oil, 1 litre",
        "Pantry",
        "oil",
        24_900,
        40,
    ),
    stock("PAN-404", "Jaggery, 1 kg", "Pantry", "jaggery", 14_900, 40),
    stock("PAN-405", "Pinni box, 500 g", "Pantry", "pinni", 44_900, 20),
    stock(
        "PAN-406",
        "Amritsari papad, 400 g",
        "Pantry",
        "papad",
        19_900,
        30,
    ),
    stock(
        "HOM-503",
        "Painted Kashmiri bowl",
        "Home",
        "bowl",
        109_900,
        6,
    ),
    stock("KIT-308", "Steel thali set", "Kitchen", "thali", 249_900, 8),
];

/// The categories, for the inventory's filter.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("Clothing", "Clothing"),
    ("Festive", "Festive"),
    ("Footwear", "Footwear"),
    ("Home", "Home"),
    ("Kitchen", "Kitchen"),
    ("Pantry", "Pantry"),
];

/// The makers' towns each item comes from.
const ORIGINS: [&str; 12] = [
    "Amritsar",
    "Ludhiana",
    "Patiala",
    "Jalandhar",
    "Jaipur",
    "Lucknow",
    "Varanasi",
    "Moradabad",
    "Panipat",
    "Saharanpur",
    "Kanpur",
    "Dehradun",
];

/// The editions each comes in, with what each does to the price, in percent.
const EDITIONS: [(&str, i64); 7] = [
    ("", 100),
    ("small", 85),
    ("large", 120),
    ("twin pack", 180),
    ("gift box", 140),
    ("festive edition", 130),
    ("travel", 90),
];

/// The icon a product is drawn with, by its key.
pub fn product_icon(key: &str) -> icons::Icon {
    match key {
        "dupatta" => icons::RIBBON,
        "jutti" => icons::FOOTPRINTS,
        "lota" => icons::MILK,
        "tiffin" => icons::SOUP,
        "kadhai" => icons::COOKING_POT,
        "ghee" => icons::DROPLET,
        "rice" => icons::WHEAT,
        "shawl" => icons::LAYERS,
        "kurta" | "salwar" => icons::SHIRT,
        "dabba" => icons::SALAD,
        "bottle" => icons::FLASK_ROUND,
        "dhurrie" => icons::LAYOUT_GRID,
        "diya" => icons::FLAME,
        "kulhad" => icons::COFFEE,
        "bedsheet" => icons::BED,
        "stand" => icons::LAMP,
        "belan" => icons::UTENSILS_CROSSED,
        "oil" => icons::DROPLETS,
        "jaggery" => icons::CANDY,
        "pinni" => icons::GIFT,
        "papad" => icons::COOKIE,
        "bowl" => icons::PALETTE,
        "thali" => icons::UTENSILS,
        _ => icons::PACKAGE,
    }
}

/// First names, mostly Punjabi, each with whether it's a woman's.
const PUNJABI: [(&str, bool); 32] = [
    ("Gurpreet", false),
    ("Manpreet", false),
    ("Jaspreet", false),
    ("Amandeep", false),
    ("Rajveer", false),
    ("Navdeep", false),
    ("Kulwinder", false),
    ("Harjit", false),
    ("Sukhwinder", false),
    ("Jasbir", false),
    ("Balwinder", false),
    ("Parminder", false),
    ("Tejinder", false),
    ("Gagandeep", false),
    ("Karanveer", false),
    ("Inderjit", false),
    ("Harleen", true),
    ("Simran", true),
    ("Ravneet", true),
    ("Jasleen", true),
    ("Manjot", true),
    ("Gurleen", true),
    ("Kiranjit", true),
    ("Sukhpreet", true),
    ("Rupinder", true),
    ("Baljeet", true),
    ("Paramjit", true),
    ("Harsimran", true),
    ("Jasmeet", true),
    ("Prabhjot", true),
    ("Mandeep", true),
    ("Navneet", true),
];

/// Punjabi family names, used beside Singh and Kaur.
const FAMILIES: [&str; 16] = [
    "Gill", "Dhillon", "Sandhu", "Brar", "Grewal", "Bains", "Sidhu", "Randhawa", "Bajwa", "Cheema",
    "Virk", "Mann", "Toor", "Johal", "Kang", "Aulakh",
];

/// Hindu first names from the north, each with whether it's a woman's.
const HINDU: [(&str, bool); 30] = [
    ("Arjun", false),
    ("Rohit", false),
    ("Vikram", false),
    ("Aditya", false),
    ("Ishaan", false),
    ("Siddharth", false),
    ("Rahul", false),
    ("Karan", false),
    ("Varun", false),
    ("Ankit", false),
    ("Nikhil", false),
    ("Saurabh", false),
    ("Mohit", false),
    ("Gaurav", false),
    ("Tarun", false),
    ("Priya", true),
    ("Neha", true),
    ("Ananya", true),
    ("Kavya", true),
    ("Pooja", true),
    ("Meera", true),
    ("Shruti", true),
    ("Riya", true),
    ("Anjali", true),
    ("Divya", true),
    ("Nidhi", true),
    ("Sakshi", true),
    ("Tanvi", true),
    ("Aarti", true),
    ("Swati", true),
];

/// Hindu family names from the north.
const SURNAMES: [&str; 18] = [
    "Sharma", "Verma", "Malhotra", "Kapoor", "Chopra", "Mehra", "Arora", "Bhatia", "Gupta",
    "Agarwal", "Khanna", "Joshi", "Saxena", "Tiwari", "Mishra", "Bansal", "Goyal", "Sethi",
];

/// Names the seed never writes, because people know someone by them.
const KNOWN: [&str; 12] = [
    "Rohit Sharma",
    "Arjun Kapoor",
    "Aditya Chopra",
    "Siddharth Malhotra",
    "Varun Sharma",
    "Akash Chopra",
    "Nikhil Chopra",
    "Neha Sharma",
    "Mohit Sharma",
    "Gaurav Kapoor",
    "Manpreet Singh",
    "Mandeep Singh",
];

const DOMAINS: [&str; 3] = ["example.com", "example.org", "example.net"];

/// What customers write at checkout, now and then.
const NOTES: [&str; 8] = [
    "Please leave it with the neighbour at the next house if nobody is home.",
    "It's a gift for my mother: please leave the bill out of the box.",
    "Call before coming, the gate stays locked in the afternoon.",
    "Deliver after 6 pm on weekdays, please.",
    "Could the diyas be packed well? They are for Diwali at my sister's.",
    "No rush, I'm travelling until Friday.",
    "Please use as little plastic as you can.",
    "Office delivery: reception is open from 10 to 6.",
];

const BRANDS: [&str; 3] = ["RuPay", "Visa", "Mastercard"];

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

/// `Harleen` as `harleen`, for an address.
fn ascii(word: &str) -> String {
    word.chars()
        .map(|c| c.to_ascii_lowercase())
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// The date of a unix time, in the shop's zone.
pub fn day_of(at: i64) -> NaiveDate {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0)
        .map(|time| time.date_naive())
        .unwrap_or_default()
}

/// `CS-20481` for order 1.
pub fn order_number(id: i64) -> String {
    format!("CS-{}", 20_480 + id)
}

/// A tracking number for an order, from the courier that collects it.
pub fn tracking(order: &Order, dice: &mut Dice) -> String {
    let prefix = match warehouse(&order.warehouse).carrier {
        "Yamuna Couriers" => "YC",
        "Kaveri Logistics" => "KL",
        _ => "DX",
    };
    format!(
        "{prefix} {:04} {:04} {}",
        dice.below(10_000),
        dice.below(10_000),
        if order.state == "EX" {
            order.country.as_str()
        } else {
            order.state.as_str()
        }
    )
}

/// Customer `id`'s phone number, in E.164: one made-up pattern,
/// `+91 98765 0xxxx`, so no number belongs to anyone.
pub fn phone_of(id: i64) -> String {
    format!("+9198765{:05}", id.rem_euclid(100_000))
}

/// `+91 98765 01234` from `+919876501234`.
pub fn phone_text(e164: &str) -> String {
    match e164.strip_prefix("+91") {
        Some(rest) if rest.len() == 10 => format!("+91 {} {}", &rest[..5], &rest[5..]),
        _ => e164.to_owned(),
    }
}

/// One product of the shop, by its index from 0: the item, the maker's town and
/// the edition vary so that the first few hundred products cover every item.
pub fn product(at: usize) -> Product {
    let item = &CATALOGUE[at % CATALOGUE.len()];
    let origin = (at / CATALOGUE.len()) % ORIGINS.len();
    let edition_at = (at / (CATALOGUE.len() * ORIGINS.len())) % EDITIONS.len();
    let (edition, percent) = EDITIONS[edition_at];
    let mut dice = Dice::new(u64::try_from(at).unwrap_or(0) * 31 + 7);
    // Prices end in 9 rupees, as shops in India price them.
    let rupees = (item.price * percent / 100 / 100 + 5) / 10 * 10 - 1;
    let price = rupees.max(9) * 100;
    let reorder_at = (item.reorder_at * 100 / percent).max(2);
    // Most are comfortably stocked; about one in thirty is low and one in a
    // hundred and fifty has run out.
    let stock = match dice.below(150) {
        0 => 0,
        1..=4 => dice.between(1, reorder_at + 1),
        _ => reorder_at + dice.between(4, 160),
    };
    let name = if edition.is_empty() {
        format!("{}, {}", item.name, ORIGINS[origin])
    } else {
        format!("{}, {}, {edition}", item.name, ORIGINS[origin])
    };
    Product {
        id: i64::try_from(at).unwrap_or(0) + 1,
        sku: format!("{}-{origin:02}{edition_at}", item.sku),
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

/// Customer `id`'s name: about three in five Punjabi, the rest Hindu names from
/// the north, never a name people know someone by.
fn name_of(id: i64, dice: &mut Dice) -> String {
    for _ in 0..8 {
        let name = if dice.chance(60) {
            let (first, woman) = PUNJABI[dice.pick(PUNJABI.len())];
            if dice.chance(55) {
                format!("{first} {}", if woman { "Kaur" } else { "Singh" })
            } else {
                format!("{first} {}", FAMILIES[dice.pick(FAMILIES.len())])
            }
        } else {
            let (first, _) = HINDU[dice.pick(HINDU.len())];
            format!("{first} {}", SURNAMES[dice.pick(SURNAMES.len())])
        };
        if !KNOWN.contains(&name.as_str()) {
            return name;
        }
    }
    format!("Customer {id}")
}

/// One customer, by id from 1.
pub fn customer(id: i64, now: i64) -> Customer {
    let n = usize::try_from(id - 1).unwrap_or(0);
    let mut dice = Dice::new(u64::try_from(id).unwrap_or(0) * 104_729);
    let name = name_of(id, &mut dice);
    let place = if dice.chance(2) {
        &ABROAD[dice.pick(ABROAD.len())]
    } else {
        &PLACES[dice.pick(PLACES.len())]
    };
    // Namesakes are told apart by a number in their address.
    let (first, last) = name.split_once(' ').unwrap_or((name.as_str(), ""));
    let pin = if place.pin == 0 {
        String::new()
    } else {
        (place.pin + u32::try_from(dice.below(12)).unwrap_or(0)).to_string()
    };
    Customer {
        id,
        email: format!("{}.{}{id}@{}", ascii(first), ascii(last), DOMAINS[n % 3]),
        name,
        phone: phone_of(id),
        country: place.country.to_owned(),
        state: place.state.to_owned(),
        city: place.city.to_owned(),
        pin,
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

/// How the customer pays: UPI most often, a card, or cash on delivery.
fn method(dice: &mut Dice, abroad: bool) -> &'static str {
    match dice.below(100) {
        _ if abroad => "card",
        0..=54 => "upi",
        55..=84 => "card",
        _ => "cod",
    }
}

/// One order placed at `placed_at`, with its lines and how far it got by `now`.
/// UPI and card orders are paid at once; cash on delivery is paid when the
/// courier hands it over.
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
    let shipping = shipping_for(subtotal, &customer.state);
    let abroad = customer.state == "EX";
    let paid_by = method(&mut dice, abroad);
    let card = paid_by == "card";
    let house = warehouse_for(&customer.state);
    let mut order = Order {
        id,
        number: order_number(id),
        customer_id: customer.id,
        customer: customer.name.clone(),
        email: customer.email.clone(),
        country: customer.country.clone(),
        state: customer.state.clone(),
        city: customer.city.clone(),
        pin: customer.pin.clone(),
        status: Status::Open.key().to_owned(),
        fulfilment: Fulfilment::Unfulfilled.key().to_owned(),
        payment: Payment::Pending.key().to_owned(),
        method: paid_by.to_owned(),
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
        warehouse: house.code.to_owned(),
        carrier: house.carrier.to_owned(),
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
    let cod = paid_by == "cod";
    let paid_at = placed_at + dice.between(3, 40);
    let packed_at = placed_at + dice.between(2 * 3600, 20 * 3600);
    let shipped_at = packed_at + dice.between(2 * 3600, 20 * 3600);
    // Within the warehouse's own state a parcel arrives in a day or two, across
    // India in two to five, abroad in a week or more.
    let delivered_at = shipped_at
        + if abroad {
            dice.between(6 * 86_400, 11 * 86_400)
        } else if house.state == order.state {
            dice.between(18 * 3600, 2 * 86_400)
        } else {
            dice.between(2 * 86_400, 5 * 86_400)
        };
    let cancelled = dice.chance(3) && now - placed_at > 6 * 3600;
    if !cod && paid_at <= now {
        push(Kind::Paid, paid_at, order.total, &order.customer, "");
        order.payment = Payment::Paid.key().to_owned();
    }
    if cancelled {
        let at = placed_at + dice.between(15 * 60, 90 * 60);
        push(
            Kind::Cancelled,
            at,
            0,
            STAFF,
            "The customer asked to cancel",
        );
        order.status = Status::Cancelled.key().to_owned();
        if !cod && paid_at <= at {
            push(Kind::Refunded, at + 30, order.total, STAFF, "Cancelled");
            order.refunded = order.total;
            order.payment = Payment::Refunded.key().to_owned();
        } else {
            order.payment = Payment::Pending.key().to_owned();
        }
        return (order, its_lines, its_events);
    }
    if packed_at <= now {
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
        if cod {
            push(
                Kind::Paid,
                delivered_at + 1,
                order.total,
                house.carrier,
                "Cash on delivery",
            );
            order.payment = Payment::Paid.key().to_owned();
        }
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
            assert!(customer.phone.starts_with("+9198765"), "{}", customer.phone);
            assert!(
                !KNOWN.contains(&customer.name.as_str()),
                "{}",
                customer.name
            );
            if customer.country == "IN" {
                assert_eq!(customer.pin.len(), 6, "{}", customer.city);
                assert!(!state_name(&customer.state).is_empty());
            }
        }
        let indian = people.iter().filter(|c| c.country == "IN").count();
        assert!(indian * 100 / people.len() >= 95, "{indian} in India");
        let punjab = people
            .iter()
            .filter(|c| c.state == "PB" || c.state == "CH")
            .count();
        assert!(punjab * 100 / people.len() >= 30, "{punjab} in Punjab");
    }

    #[test]
    fn a_phone_number_reads_the_indian_way() {
        assert_eq!(phone_of(1_234), "+919876501234");
        assert_eq!(phone_text("+919876501234"), "+91 98765 01234");
    }

    #[test]
    fn an_orders_customer_is_known_without_the_order() {
        let (orders, _, _) = newest(50);
        for order in &orders {
            assert_eq!(customer_of(order.id), order.customer_id);
        }
    }

    #[test]
    fn two_thousand_products_each_with_its_own_sku_and_a_price_in_rupees() {
        let all = products();
        assert_eq!(all.len(), 2_000);
        let mut skus: Vec<&str> = all.iter().map(|p| p.sku.as_str()).collect();
        skus.sort_unstable();
        skus.dedup();
        assert_eq!(skus.len(), 2_000);
        assert!(
            all.iter()
                .all(|p| p.price > 0 && p.price % 100 == 0 && p.reorder_at >= 2)
        );
        assert!(
            all.iter().all(|p| (p.price / 100) % 10 == 9),
            "prices end in 9"
        );
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
            // Only cash on delivery ships unpaid, and a cancelled order never ships.
            if Fulfilment::parse(&order.fulfilment) >= Fulfilment::Packed {
                assert!(
                    Payment::parse(&order.payment) != Payment::Pending || order.method == "cod",
                    "{}",
                    order.number
                );
                assert_ne!(Status::parse(&order.status), Status::Cancelled);
            }
            // A delivered order is paid, cash on delivery too.
            if Fulfilment::parse(&order.fulfilment) == Fulfilment::Delivered {
                assert_ne!(Payment::parse(&order.payment), Payment::Pending);
            }
        }
        assert!(events.iter().all(|event| event.at <= NOW));
    }

    #[test]
    fn the_numbers_look_like_an_indian_shop() {
        let (orders, _, _) = newest(4_000);
        let today = day_of(NOW);
        let placed_today = orders.iter().filter(|o| o.placed_on == today).count();
        assert!((150..900).contains(&placed_today), "{placed_today} today");
        assert!(orders.iter().any(|o| o.status == Status::Cancelled.key()));
        assert!(
            orders
                .iter()
                .filter(|o| o.status == Status::Open.key())
                .count()
                > 200
        );
        assert!(orders.iter().any(|o| o.method == "cod"));
        assert!(orders.iter().filter(|o| o.method == "upi").count() > orders.len() / 3);
        assert!(orders.iter().any(|o| o.city == "Ludhiana"));
        let abroad = orders.iter().filter(|o| o.country != "IN").count();
        assert!(
            abroad > 0 && abroad * 100 < orders.len() * 5,
            "{abroad} abroad"
        );
        // Refunds come days after delivery: look a few months back for them.
        let (older, _, _) = super::orders(150_001, 154_000, NOW, &products(), &customers(NOW));
        assert!(
            older
                .iter()
                .any(|o| o.payment == Payment::PartiallyRefunded.key())
        );
        assert!(older.iter().any(|o| o.payment == Payment::Refunded.key()));
        // A year back, the first orders are a year old.
        assert!(placed_at(1, NOW) < NOW - 360 * 86_400);
    }
}
