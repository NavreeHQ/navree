//! The inventory: every product with its stock, a badge when it runs low, and a
//! restock form in its row. The table runs on the server and follows the shop
//! live, so stock sold while the page is open goes down in front of you.

use navree::ui::*;
use serde::Deserialize;

use crate::data::{CATEGORIES, Level, Product};
use crate::shop::RESTOCK_MAX;
use crate::views::money;
use crate::views::order::product;

/// The level filter's choices.
pub const LEVELS: &[(&str, &str)] = &[
    ("in_stock", "In stock"),
    ("low", "Low stock"),
    ("out", "Out of stock"),
];

/// A product, as the inventory shows it.
#[derive(Clone, TableRow)]
#[table(sort = "stock", per = 25, per_pages(10, 25, 50))]
pub struct ProductRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, sticky, label = "Product", role = title)]
    pub name: String,
    #[col(search, hidden, label = "SKU", role = subtitle)]
    pub sku: String,
    #[col(skip)]
    pub icon: String,
    #[col(filter = multi, options = CATEGORIES, hide_on_mobile)]
    pub category: String,
    #[col(sortable, numeric, hide_on_mobile)]
    pub price: i64,
    #[col(sortable, numeric, label = "In stock", role = trailing)]
    pub stock: i64,
    #[col(filter = multi, options = LEVELS, label = "Level", role = status)]
    pub level: String,
    #[col(skip)]
    pub reorder_at: i64,
    #[col(label = "Restock", align = end, width = 220)]
    pub restock: i64,
}

impl From<Product> for ProductRow {
    fn from(product: Product) -> Self {
        // A low shelf gets back well above its reorder point; a full one, a usual
        // order's worth.
        let restock = (product.reorder_at * 3 - product.stock)
            .max(product.reorder_at * 2)
            .clamp(1, RESTOCK_MAX);
        Self {
            id: product.id,
            name: product.name,
            sku: product.sku,
            icon: product.icon,
            category: product.category,
            price: product.price,
            stock: product.stock,
            level: product.level,
            reorder_at: product.reorder_at,
            restock,
        }
    }
}

/// What a restock says: how many units arrived.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "restock")]
pub struct RestockForm {
    /// Units
    #[validate(range(min = 1, max = 500))]
    pub quantity: i64,
}

/// How much is left, as a badge: green, amber when low, red when out.
pub fn level_badge(key: &str) -> Badge {
    let level = Level::parse(key);
    Badge::new(level.label()).tone(level.tone()).dot()
}

/// The stock, and the point it reorders at under it.
fn stock_cell(row: &ProductRow) -> Node {
    Stack::gap(0)
        .children((
            Text::new(row.stock.to_string()).inline(),
            Text::muted(format!("Reorder at {}", row.reorder_at)),
        ))
        .into()
}

/// A number of units and a button, in a form of the row's own: the server adds
/// them and the stream redraws the row.
fn restock_cell(row: &ProductRow) -> Node {
    restock_form(row.id, row.restock, &row.name)
}

/// The restock form of product `id`: `units` to begin with.
fn restock_form(id: i64, units: i64, name: &str) -> Node {
    el("form")
        .attr("method", "post")
        .attr("action", format!("/inventory/{id}/restock"))
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(
            Row::gap(2)
                .align(Align::Center)
                .justify(Justify::End)
                .children((
                    Input::number("quantity")
                        .value(units.to_string())
                        .attr("min", 1_i64)
                        .attr("max", RESTOCK_MAX)
                        .attr("aria-label", format!("Units of {name} to add"))
                        .required(),
                    Button::outline("Restock")
                        .icon(icons::PACKAGE_PLUS)
                        .size(Size::Sm)
                        .submit(),
                )),
        )
        .into()
}

/// The id of the inventory table, which a restock draws again.
pub const TABLE: &str = "products";

/// The id of the sheet a row opens on a screen drawn by the system's own toolkit.
pub const SHEET: &str = "product";

/// The sheet a row opens, before it is filled.
pub fn empty_sheet() -> Sheet {
    Sheet::new(SHEET, "Product")
}

/// The sheet filled for one product: what is on the shelf, and the form that
/// adds to it.
pub fn product_sheet(product: Product) -> Sheet {
    let row = ProductRow::from(product);
    Sheet::new(SHEET, row.name.clone())
        .width(SheetWidth::Md)
        .description(format!("{} · {}", row.sku, row.category))
        .body(
            Stack::gap(4).children((
                FactGrid::new().cols(2).cards([
                    FactCard::new("In stock", row.stock.to_string())
                        .detail(format!("Reorder at {}", row.reorder_at)),
                    FactCard::new("Price", money(row.price)),
                    FactCard::new("Level", level_badge(&row.level)),
                ]),
                restock_form(row.id, row.restock, &row.name),
            )),
        )
}

/// The inventory table.
pub fn inventory_table<'a>(
    query: &'a TableQuery<ProductRow>,
    page: &'a TablePage<ProductRow>,
) -> DataTable<'a, ProductRow> {
    DataTable::new(TABLE, query, page)
        .caption("Inventory")
        .cell("name", |row| product(&row.icon, &row.name, &row.sku))
        .cell("price", |row| Text::new(money(row.price)).inline())
        .cell("stock", stock_cell)
        .cell("level", |row| level_badge(&row.level))
        .cell("restock", restock_cell)
        // A tap on a row opens the product, with its restock form, where the reader is.
        .row_opens(|row| format!("/inventory/{}/peek", row.id))
        .filter_bar()
        .sticky_header()
        .live_on(crate::live::stream().on("inventory"))
        .empty(
            Empty::new("No product matches")
                .icon(icons::PACKAGE_SEARCH)
                .description("Clear a filter, or search for part of a name or a SKU."),
        )
}
