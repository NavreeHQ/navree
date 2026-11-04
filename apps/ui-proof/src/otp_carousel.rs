//! The one-time code field and the carousel, in the states the browser suites run against.
//!
//! | Page | Shows |
//! |---|---|
//! | `/otp` | code fields of every length and state, a form that posts them and one that sends itself |
//! | `/otp/echo` | what a form posted, and what the server made of each code |
//! | `/carousel` | carousels with buttons, pickers, going round, rotation, two in view, vertical and right to left |

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree::http::extract::urlencoded;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::*;

fn pairs(input: &str) -> Vec<(String, String)> {
    urlencoded::parse(input.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(6).children(content)))
}

// ---- Codes -----------------------------------------------------------------

fn otp_page(shell: &AppShell) -> AppPage {
    let form = el("form")
        .attr("id", "otp-form")
        .attr("method", "post")
        .attr("action", "/otp/echo")
        .child(
            Stack::gap(6).children((
                Field::new("otp-six", "Verification code")
                    .help("Six digits, sent to +91 98765 43210.")
                    .control(InputOtp::new("code", 6)),
                Field::new("otp-grouped", "Transaction PIN")
                    .control(InputOtp::new("pin", 6).group(3)),
                Field::new("otp-voucher", "Voucher")
                    .control(InputOtp::new("voucher", 8).kind(OtpKind::Alphanumeric)),
                Field::new("otp-four", "Four-digit PIN").control(InputOtp::new("pin4", 4)),
                Field::new("otp-twelve", "Twelve characters").control(InputOtp::new("long12", 12)),
                Row::gap(2).children(Button::primary("Verify").submit().attr("id", "otp-send")),
            )),
        );
    let sends_itself = el("form")
        .attr("id", "otp-auto-form")
        .attr("method", "post")
        .attr("action", "/otp/echo")
        .child(
            Field::new("otp-auto", "Code from the bank (sent when whole)")
                .control(InputOtp::new("auto", 6).submit_when_complete()),
        );
    let states = Stack::gap(6).children((
        Field::new("otp-prefilled", "Saved code").control(InputOtp::new("kept", 6).value("482915")),
        Field::new("otp-readonly", "Read-only code")
            .control(InputOtp::new("ro", 6).value("482915").readonly()),
        Field::new("otp-disabled", "Disabled code")
            .control(InputOtp::new("off", 6).value("482915").disabled(true)),
        Field::new("otp-invalid", "Expired code")
            .error("That code has expired. Ask for a new one.")
            .control(InputOtp::new("expired", 6).value("123456")),
    ));
    let rtl = el("div")
        .attr("dir", "rtl")
        .attr("lang", "ur")
        .child(Field::new("otp-rtl", "تصدیقی کوڈ").control(InputOtp::new("rtl", 6)));
    shell
        .page("One-time codes")
        .body(section("codes", "Codes", (form, sends_itself, states, rtl)))
}

/// What the form posted, and what the server made of each code under the field's rules.
fn echo(shell: &AppShell, body: &str) -> AppPage {
    let all = pairs(body);
    let mut posted: Vec<String> = all
        .iter()
        .filter(|(name, _)| name != "_csrf")
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    posted.sort();
    let rules = [
        ("code", OtpRules::new(6, OtpKind::Digits)),
        ("pin", OtpRules::new(6, OtpKind::Digits)),
        ("pin4", OtpRules::new(4, OtpKind::Digits)),
        ("long12", OtpRules::new(12, OtpKind::Digits)),
        ("auto", OtpRules::new(6, OtpKind::Digits)),
        ("voucher", OtpRules::new(8, OtpKind::Alphanumeric)),
    ];
    let judged: Vec<String> = rules
        .iter()
        .filter_map(|(name, rules)| {
            let value = all
                .iter()
                .find(|(field, _)| field.as_str() == *name)
                .map(|(_, value)| value.as_str())
                .filter(|value| !value.is_empty())?;
            Some(match rules.check(value) {
                Ok(code) => format!("{name}: ok {code}"),
                Err(error) => format!("{name}: {error}"),
            })
        })
        .collect();
    shell.page("Posted").body(Stack::gap(4).children((
        el("pre").attr("id", "posted").child(posted.join("\n")),
        el("pre").attr("id", "judged").child(judged.join("\n")),
        Button::outline("Back").href("/otp"),
    )))
}

// ---- Carousels -------------------------------------------------------------

/// A slide: a card with a name and a line under it.
fn offer(title: &'static str, text: &'static str) -> Card {
    Card::new().body(Stack::gap(2).children((Heading::h4(title), Text::muted(text))))
}

fn offers(order_link: bool) -> Vec<CarouselSlide> {
    vec![
        CarouselSlide::new(offer(
            "Phulkari dupatta",
            "Hand-embroidered in Patiala. ₹2,499.",
        ))
        .title("Phulkari dupatta"),
        CarouselSlide::new(offer(
            "Basmati rice, 5 kg",
            "Aged for a year, from Karnal. ₹1,150.",
        ))
        .title("Basmati rice, 5 kg"),
        CarouselSlide::new(Card::new().body(Stack::gap(2).children((
            Heading::h4("Desi ghee, 1 kg"),
            Text::muted("Churned in Amritsar. ₹750."),
            order_link.then(|| {
                Button::outline("Order now")
                    .href("/carousel#ordered")
                    .attr("id", "c-basic-order")
            }),
        ))))
        .title("Desi ghee, 1 kg"),
        CarouselSlide::new(offer("Pashmina shawl", "Woven in Jammu. ₹12,500."))
            .title("Pashmina shawl"),
    ]
}

fn mandi_days() -> Vec<CarouselSlide> {
    vec![
        CarouselSlide::new(offer("Monday", "Jalandhar mandi, from 6 am.")),
        CarouselSlide::new(offer("Wednesday", "Amritsar mandi, from 6 am.")),
        CarouselSlide::new(offer("Friday", "Ludhiana mandi, from 5 am.")),
    ]
}

fn shops() -> Vec<CarouselSlide> {
    [
        ("Sutlej Supply Co.", "Ludhiana"),
        ("Doaba Traders", "Jalandhar"),
        ("Pind Pantry", "Patiala"),
        ("Haveli Home Goods", "Amritsar"),
        ("Majha Mills", "Gurdaspur"),
        ("Malwa Metals", "Bathinda"),
    ]
    .into_iter()
    .map(|(name, city)| {
        CarouselSlide::new(
            Card::new().body(Stack::gap(2).children((Heading::h4(name), Text::muted(city)))),
        )
    })
    .collect()
}

fn mandi_news() -> Vec<CarouselSlide> {
    vec![
        CarouselSlide::new(offer("Wheat", "₹2,275 a quintal at the Khanna mandi.")),
        CarouselSlide::new(offer("Paddy", "Arrivals open in Sangrur on 1 October.")),
        CarouselSlide::new(offer("Maize", "₹2,090 a quintal in Jalandhar.")),
        CarouselSlide::new(offer("Cotton", "Bathinda opens its season next week.")),
    ]
}

fn urdu() -> Vec<CarouselSlide> {
    [
        ("پھلکاری دوپٹہ", "پٹیالہ میں ہاتھ سے کڑھائی۔"),
        ("باسمتی چاول", "کرنال سے، ایک سال پرانے۔"),
        ("دیسی گھی", "امرتسر میں بنایا گیا۔"),
        ("پشمینہ شال", "جموں میں بُنی گئی۔"),
    ]
    .into_iter()
    .map(|(title, text)| CarouselSlide::new(offer(title, text)).title(title))
    .collect()
}

fn carousel_page(shell: &AppShell) -> AppPage {
    let labelled = |title: &'static str, carousel: Carousel| {
        Stack::gap(3).children((Heading::h3(title), carousel))
    };
    shell.page("Carousels").body(section(
        "carousels",
        "Carousels",
        (
            labelled(
                "Buttons",
                Carousel::new("c-basic", "Offers from Ludhiana").slides(offers(true)),
            ),
            el("div").children((
                Button::outline("Before the carousel").attr("id", "c-pickers-before"),
                Stack::gap(3).children((
                    Heading::h3("Pickers"),
                    Carousel::new("c-pickers", "Offers with pickers")
                        .slides(offers(false))
                        .pickers(),
                )),
            )),
            labelled(
                "Going round",
                Carousel::new("c-loop", "Mandi days")
                    .slides(mandi_days())
                    .loop_around(),
            ),
            labelled(
                "Rotating",
                Carousel::new("c-auto", "Festival deals")
                    .slides(mandi_days())
                    .pickers()
                    .autoplay(Duration::from_secs(2)),
            ),
            labelled(
                "Two in view",
                Carousel::new("c-two", "Six shops")
                    .slides(shops())
                    .per_view(2),
            ),
            labelled(
                "Vertical",
                Carousel::new("c-vertical", "News from the mandi")
                    .slides(mandi_news())
                    .vertical(),
            ),
            labelled(
                "One slide",
                Carousel::new("c-one", "A single notice").slide(CarouselSlide::new(offer(
                    "Closed on Gurpurab",
                    "Every shop reopens the next morning.",
                ))),
            ),
            el("div")
                .attr("dir", "rtl")
                .attr("lang", "ur")
                .child(labelled(
                    "Right to left",
                    Carousel::new("c-rtl", "لدھیانہ کی پیشکشیں")
                        .slides(urdu())
                        .pickers(),
                )),
            labelled(
                "Hostile text",
                Carousel::new("c-hostile", "<img src=x onerror=window.__pwned=1>")
                    .slides(mandi_days())
                    .labels(CarouselLabels {
                        previous: "<b>Back</b>".into(),
                        ..CarouselLabels::default()
                    }),
            ),
            el("p").attr("id", "ordered").child("Ordered."),
        ),
    ))
}

// ---- The controller --------------------------------------------------------

pub struct CodePages;

impl Injectable for CodePages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for CodePages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/otp", |shell: AppShell| async move {
                otp_page(&shell)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/otp/echo",
                |shell: AppShell, body: String| async move { echo(&shell, &body) },
            )
            .public(),
            RouteDef::on(Method::GET, "!/carousel", |shell: AppShell| async move {
                carousel_page(&shell)
            })
            .public(),
        ]
    }
}
