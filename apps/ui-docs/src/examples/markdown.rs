//! Markdown and CodeBlock.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("markdown.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("markdown", "thumb", "Markdown", markdown_thumb)
            .start()
            .width(520)
            .height(150),
        single!(
            "markdown",
            "default",
            "An answer in Markdown",
            markdown_default
        )
        .start()
        .width(620)
        .height(330),
        single!("markdown", "table", "A table", markdown_table)
            .start()
            .width(620)
            .height(210),
        single!(
            "markdown",
            "links",
            "Links to other sites",
            markdown_links
        )
        .start()
        .width(620)
        .height(150),
        single!(
            "markdown",
            "images",
            "Pictures in text you do not control",
            markdown_images
        )
        .start()
        .width(620)
        .height(110),
        single!("markdown", "raw-html", "Raw HTML is text", markdown_raw)
            .start()
            .width(620)
            .height(130),
        single!(
            "markdown",
            "headings",
            "Where the headings start",
            markdown_headings
        )
        .start()
        .width(620)
        .height(190),
        single!("markdown", "code", "Code in a reply", markdown_code)
            .start()
            .width(620)
            .height(240),
        single!("markdown", "rtl", "Right to left", markdown_rtl)
            .start()
            .width(520)
            .height(150),
        single!("code-block", "thumb", "A code block", code_block_thumb)
            .start()
            .width(520)
            .height(150),
        single!("code-block", "default", "Code with colours", code_block_default)
            .start()
            .width(620)
            .height(210),
        single!("code-block", "copy", "A copy button", code_block_copy)
            .start()
            .width(620)
            .height(150),
        gallery!(
            "code-block",
            "languages",
            "Languages",
            code_block_languages
        )
        .medium(),
    ]
}

fn markdown_thumb() -> Markdown {
    Markdown::new(
        "**Ludhiana** had *1,240* orders this week. Read the [full report](/reports/weekly).",
    )
}

fn markdown_default() -> Markdown {
    Markdown::new(
        r#"### Weekly summary

UPI payments from **Amritsar** grew *12%* this week. Three things need you:

1. Follow up with [Harleen Kaur](mailto:harleen.kaur@example.com) about the Patiala order.
2. Read the [full report](/reports/weekly).
3. Check the ₹1,00,000 refund for `order-1042`.

> The next settlement runs on Monday at 10:30 IST.

- [x] Send the invoices
- [ ] Call Gurpreet Singh"#,
    )
}

fn markdown_table() -> Markdown {
    Markdown::new(
        r#"| City | Orders | Revenue |
|:--|--:|--:|
| Ludhiana | 1,240 | ₹18,40,000 |
| Jalandhar | 860 | ₹11,25,500 |
| Patiala | 410 | ₹6,02,250 |"#,
    )
}

fn markdown_links() -> Markdown {
    Markdown::new(
        r#"A link on this site: [the report](/reports/weekly). A link to another site shows where it goes: [GST portal](https://www.gst.gov.in/). A link with a script in it keeps its words and loses the link: [click me](javascript:alert(1))."#,
    )
}

fn markdown_images() -> Markdown {
    // Text from a model or a visitor never draws a picture: a picture's address could carry
    // what the page knows to another site.
    Markdown::new("Here is the chart: ![Orders by city](https://images.example.com/orders.png)")
}

fn markdown_raw() -> Markdown {
    Markdown::new(
        r#"Raw HTML is shown as the text it is: <script>alert("hi")</script> <b onclick="steal()">bold?</b>"#,
    )
}

fn markdown_headings() -> Markdown {
    // A chat message in a page that has its own `h1` and `h2` starts at `h3`.
    Markdown::new("# The answer\n\nIt starts at an `h3`.\n\n## The detail\n\nAnd goes on from there.")
        .heading_level(3)
}

fn markdown_code() -> Markdown {
    Markdown::new(
        r#"Run it like this:

```sh
cargo run --release -p orders -- serve --port 8080
```

Then ask for the totals:

```sql
SELECT city, SUM(total) AS revenue FROM orders GROUP BY city;
```"#,
    )
}

fn markdown_rtl() -> Element {
    el("div").attr("dir", "rtl").child(Markdown::new(
        "**ਲੁਧਿਆਣਾ** ਵਿੱਚ ਇਸ ਹਫ਼ਤੇ *1,240* ਆਰਡਰ ਆਏ। [ਪੂਰੀ ਰਿਪੋਰਟ](/reports/weekly) ਪੜ੍ਹੋ।\n\n- ਚਲਾਨ ਭੇਜੋ\n- ਗੁਰਪ੍ਰੀਤ ਨੂੰ ਫ਼ੋਨ ਕਰੋ",
    ))
}

fn code_block_thumb() -> CodeBlock {
    CodeBlock::new("rust", "let total = orders.len();")
}

fn code_block_default() -> CodeBlock {
    CodeBlock::new(
        "rust",
        concat!(
            "#[derive(Debug)]\n",
            "struct Order {\n",
            "    city: String,\n",
            "    total: i64, // in paise\n",
            "}\n",
            "\n",
            "fn revenue(orders: &[Order]) -> i64 {\n",
            "    orders.iter().map(|order| order.total).sum()\n",
            "}\n",
        ),
    )
}

fn code_block_copy() -> CodeBlock {
    CodeBlock::new("sh", "navree new orders --kit crm").copy_button()
}

fn code_block_languages() -> Vec<(&'static str, CodeBlock)> {
    vec![
        ("Rust", CodeBlock::new("rust", "fn main() {\n    println!(\"Sat Sri Akal\");\n}")),
        (
            "TypeScript",
            CodeBlock::new("ts", "const city: string = \"Amritsar\";\nawait load(city);"),
        ),
        (
            "JSON",
            CodeBlock::new("json", "{ \"city\": \"Ludhiana\", \"pin\": 141001, \"open\": true }"),
        ),
        (
            "TOML",
            CodeBlock::new("toml", "[server]\nhost = \"0.0.0.0\"\nport = 8080"),
        ),
        (
            "SQL",
            CodeBlock::new("sql", "select city, count(*) from orders where total > 100000 group by city;"),
        ),
        (
            "Shell",
            CodeBlock::new("sh", "# build and run\ncargo run --release -- serve"),
        ),
        ("HTML", CodeBlock::new("html", "<a href=\"/orders\">Orders</a>")),
        ("CSS", CodeBlock::new("css", "a { color: #0b7a3e; margin: 0 4px; }")),
        ("YAML", CodeBlock::new("yaml", "services:\n  web:\n    port: 8080")),
        ("Diff", CodeBlock::new("diff", "@@ -1 +1 @@\n-city: Delhi\n+city: Patiala")),
        ("HTTP", CodeBlock::new("http", "GET /orders HTTP/1.1\nHost: example.com")),
    ]
}
