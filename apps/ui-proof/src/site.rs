//! A public website made of the marketing blocks: Kesar, an invented Indian health app,
//! with its home page, pricing, a privacy policy, a contact page, a sign-up form checked on
//! the server, and its sitemap and robots file.
//!
//! | Page | Shows |
//! |---|---|
//! | `/site` | the home page: header, hero, logos, features, rows, numbers, testimonials, pricing, questions, sign-up, call to action, footer |
//! | `/site/pricing` | the plans; `?billing=yearly` draws the yearly prices first |
//! | `/site/privacy` | a legal page with its contents |
//! | `/site/contact` | a contact form checked on the server |
//! | `/site/screen` | the phone screen the home page shows as a picture |
//! | `/site/sitemap.xml`, `/site/robots.txt` | the two files beside the pages |
//!
//! The reader's light, dark or system choice from the footer's toggle is kept in the
//! toolkit's cookie and read back with `Appearance::for_reader`; `?mode=` overrides it for one
//! page, as the screenshots use.

use std::sync::Arc;

use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use navree::http::extract::{Path, Query};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::charts::*;
use navree::ui::form::{FormView, Rejected};
use navree::ui::marketing::*;
use navree::ui::seo::{
    AI_CRAWLERS, BreadcrumbList, JsonLd, Offer, Organization, PageKind, RobotsTxt, Seo, Sitemap,
    SitemapUrl, SocialImage, SoftwareApplication, Thing, WebPage, WebSite,
};
use navree::ui::theme::{Appearance, Mode, Theme};
use navree::ui::*;
use serde::Deserialize;

/// Where the site would live.
const SITE: &str = "https://kesar.example";
/// When the pages last changed.
const UPDATED: &str = "2026-11-10";

/// The phone screen, at two widths in three formats, made from `/site/screen`.
const SCREEN: [(&str, &str, &[u8]); 6] = [
    (
        "sleep-390.png",
        "image/png",
        include_bytes!("../assets/site/sleep-390.png"),
    ),
    (
        "sleep-780.png",
        "image/png",
        include_bytes!("../assets/site/sleep-780.png"),
    ),
    (
        "sleep-390.webp",
        "image/webp",
        include_bytes!("../assets/site/sleep-390.webp"),
    ),
    (
        "sleep-780.webp",
        "image/webp",
        include_bytes!("../assets/site/sleep-780.webp"),
    ),
    (
        "sleep-390.avif",
        "image/avif",
        include_bytes!("../assets/site/sleep-390.avif"),
    ),
    (
        "sleep-780.avif",
        "image/avif",
        include_bytes!("../assets/site/sleep-780.avif"),
    ),
];

#[derive(Debug, Default, Deserialize)]
pub struct SiteQuery {
    pub billing: Option<String>,
    pub mode: Option<String>,
    pub theme: Option<String>,
    pub subscribed: Option<String>,
    pub sent: Option<String>,
}

impl SiteQuery {
    fn query(&self) -> String {
        let mut parts = Vec::new();
        if let Some(mode) = &self.mode {
            parts.push(format!("mode={mode}"));
        }
        if let Some(theme) = &self.theme {
            parts.push(format!("theme={theme}"));
        }
        parts.join("&")
    }
}

/// The sign-up form: one address.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct Subscribe {
    /// Email
    #[validate(email)]
    #[form(
        placeholder = "you@example.in",
        error = "Enter an email address, like you@example.in."
    )]
    pub email: String,
}

/// The contact form.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct ContactMessage {
    /// Your name
    #[validate(length(min = 2, max = 80))]
    #[form(placeholder = "Gurpreet Kaur", error = "Tell us your name.")]
    pub name: String,
    /// Email
    #[validate(email)]
    #[form(
        placeholder = "you@example.in",
        error = "Enter an email address, like you@example.in."
    )]
    pub email: String,
    /// Message
    #[validate(length(min = 10, max = 2000))]
    #[form(widget = textarea, wide, placeholder = "How can we help?", error = "Write at least a sentence.")]
    pub message: String,
}

/// How the page looks: the site's theme, the reader's choice laid over it.
fn look(headers: &HeaderMap, query: &SiteQuery) -> Appearance {
    let url = query.query();
    Appearance::new(Theme::SUNSET, Mode::System)
        .for_reader(headers, (!url.is_empty()).then_some(url.as_str()))
}

fn brand() -> Brand {
    Brand::new("Kesar", "/site").icon(icons::SUN)
}

fn header(current: &str) -> SiteHeader {
    let links = [
        ("Features", "/site#features"),
        ("How it works", "/site#how"),
        ("Pricing", "/site/pricing"),
        ("FAQ", "/site#faq"),
        ("Contact", "/site/contact"),
    ];
    links
        .into_iter()
        .fold(SiteHeader::new(brand()), |header, (label, href)| {
            if href == current {
                header.current(label, href)
            } else {
                header.link(label, href)
            }
        })
        .action(Button::ghost("Sign in").href("/site/contact"))
        .action(Button::primary("Start free").href("/site/pricing"))
}

fn footer(mode: Mode, back: &str) -> SiteFooter {
    SiteFooter::new(brand())
        .tagline("Your sleep, heart and steps, read calmly. Made in Mohali for people everywhere.")
        .social(icons::GLOBE, "Kesar on the web", "https://kesar.example/")
        .social(icons::MAIL, "Write to Kesar", "mailto:namaste@kesar.example")
        .column(
            FooterColumn::new("Product")
                .link("Features", "/site#features")
                .link("How it works", "/site#how")
                .link("Pricing", "/site/pricing")
                .link("Questions", "/site#faq"),
        )
        .column(
            FooterColumn::new("Company")
                .link("Contact", "/site/contact")
                .link("Privacy", "/site/privacy")
                .link("Sitemap", "/site/sitemap.xml"),
        )
        .column(
            FooterColumn::new("Get the app")
                .link("Web", "/site/pricing")
                .link("Android", "/site/pricing")
                .link("Letter", "/site#letter"),
        )
        .legal("© 2026 Kesar Health Pvt. Ltd., Mohali, Punjab.")
        .legal("Kesar is a wellness guide, not a medical device. Its scores are estimates, never a diagnosis.")
        .theme_toggle(ThemeToggle::new(mode).back(back))
}

/// The head every page shares, for the page at `path` titled `name`.
fn seo(path: &str, name: &str, description: &str, kind: PageKind, extra: Vec<Thing>) -> Seo {
    let url = format!("{SITE}{path}");
    let org = format!("{SITE}/site#organization");
    let website = format!("{SITE}/site#website");
    let crumbs = format!("{url}#breadcrumb");
    let mut trail = BreadcrumbList::new()
        .id(&crumbs)
        .step("Home", format!("{SITE}/site"));
    if path != "/site" {
        trail = trail.step(name, &url);
    }
    let graph = JsonLd::new()
        .node(
            Organization::new("Kesar", format!("{SITE}/site"))
                .id(&org)
                .logo(format!("{SITE}/site/logo.png"))
                .same_as("https://www.linkedin.com/company/kesar-health")
                .slogan("Your health data, read calmly.")
                .email("namaste@kesar.example"),
        )
        .node(
            WebSite::new("Kesar", format!("{SITE}/site"))
                .id(&website)
                .publisher(&org)
                .in_language("en-IN"),
        )
        .node(
            WebPage::new(&url, name, description)
                .kind(kind)
                .in_language("en-IN")
                .date_modified(UPDATED)
                .is_part_of(&website)
                .breadcrumb(&crumbs),
        )
        .node(trail);
    let graph = extra.into_iter().fold(graph, JsonLd::node);
    Seo::new(description)
        .canonical(&url)
        .site_name("Kesar")
        .locale("en_IN")
        .image(
            SocialImage::new(
                format!("{SITE}/site/og.png"),
                1200,
                630,
                "Kesar's daily score: a ring at 82 with sleep, heart and steps under it",
            )
            .mime("image/png"),
        )
        .twitter_site("@kesarhealth")
        .json_ld(graph)
}

fn app() -> Thing {
    SoftwareApplication::new("Kesar", "HealthApplication")
        .description("Kesar turns the sleep, heart and step data your phone already keeps into one calm score a day.")
        .operating_system("Web, Android")
        .url(format!("{SITE}/site"))
        .free(true)
        .publisher(format!("{SITE}/site#organization"))
        .feature("Daily score")
        .feature("Sleep stages")
        .feature("Resting heart rate and its trend")
        .offer(Offer::new("0", "INR").name("Free"))
        .offer(Offer::new("299", "INR").name("Plus, monthly"))
        .offer(Offer::new("2990", "INR").name("Plus, yearly"))
        .offer(Offer::new("499", "INR").name("Family, monthly"))
        .into()
}

fn page(look: Appearance, title: &str, seo: Seo, body: SitePage) -> View {
    View::new(title.to_owned())
        .lang("en")
        .appearance(look)
        .seo(seo)
        .body(body)
        .cache("private, no-cache")
}

/// What the app looks like today, drawn by the toolkit: the hero's picture.
fn today() -> Node {
    let score = Card::new().body(
        Stack::gap(3).children((
            Row::gap(2).justify(Justify::Between).children((
                Text::muted("Today, Thursday 12 November"),
                Badge::new("Ready to train"),
            )),
            Row::gap(6).wrap().children((
                Ring::new("Daily score", 82)
                    .id("hero-score")
                    .label("Daily score")
                    .sublabel("Above your usual")
                    .fill(Gradient::sheen(Ink::slot(2)))
                    .glow()
                    .size(152),
                Stack::gap(3).children((
                    Stat::new("Sleep", "7 h 36 min")
                        .change(Trend::new(24).suffix(" min").decimals(0)),
                    Stat::new("Resting heart", "58 bpm")
                        .change(Trend::new(-2).suffix(" bpm").decimals(0).up_is_bad()),
                )),
            )),
        )),
    );
    let night = Card::titled("Last night").body(
        Meter::new("Sleep stages")
            .id("hero-sleep")
            .table(TableMode::ScreenReader)
            .minutes()
            .gradient()
            .segment(Segment::new("Deep", 92).ink(Ink::slot(7)))
            .segment(Segment::new("REM", 98).ink(Ink::tone(Tone::named("cyan"))))
            .segment(Segment::new("Light", 215).ink(Ink::slot(3)))
            .segment(Segment::new("Awake", 31).ink(Ink::slot(4))),
    );
    let heart = Card::titled("Resting heart, 14 days").body(
        Sparkline::new(
            "Resting heart rate, last 14 days",
            [63, 62, 62, 61, 63, 60, 60, 59, 61, 59, 58, 59, 58, 58],
        )
        .id("hero-heart")
        .area()
        .smooth()
        .ink(Ink::slot(2))
        .height(56),
    );
    Stack::gap(3)
        .children((score, Grid::cols(2).gap(3).children((night, heart))))
        .into()
}

/// The phone screen the home page shows as a picture.
fn screen() -> Node {
    Stack::gap(3)
        .children((
            Heading::h2("Last night"),
            Text::muted("11:20 pm to 6:56 am · Mohali"),
            Ring::new("Sleep score", 84)
                .id("screen-ring")
                .label("Sleep score")
                .sublabel("Great")
                .fill(Gradient::sheen(Ink::slot(7)))
                .glow()
                .size(180),
            Meter::new("Sleep stages")
                .id("screen-stages")
                .table(TableMode::ScreenReader)
                .minutes()
                .gradient()
                .segment(Segment::new("Deep", 92).ink(Ink::slot(7)))
                .segment(Segment::new("REM", 98).ink(Ink::tone(Tone::named("cyan"))))
                .segment(Segment::new("Light", 215).ink(Ink::slot(3)))
                .segment(Segment::new("Awake", 31).ink(Ink::slot(4))),
            StatBand::new()
                .stat("93%", "Efficiency")
                .stat("65 min", "Sleep debt")
                .stat("7.6 h", "Asleep")
                .stat("4", "Cycles"),
            Card::titled("Last 7 nights").body(
                Sparkline::new("Sleep score, last 7 nights", [71, 76, 68, 80, 77, 82, 84])
                    .id("screen-week")
                    .area()
                    .smooth()
                    .ink(Ink::slot(7))
                    .height(64),
            ),
        ))
        .into()
}

fn rupee_plans(yearly: bool) -> Pricing {
    let pricing = Pricing::new("plans")
        .yearly_badge("2 months free")
        .plan(
            Plan::new("Free")
                .description("One person, the essentials, for ever.")
                .price("₹0", "for ever")
                .feature("Daily score and its reasons")
                .feature("Sleep, heart and steps for 30 days")
                .feature("Android and the web")
                .action(Button::outline("Start free").href("/site/contact")),
        )
        .plan(
            Plan::new("Plus")
                .description("The whole picture for one person.")
                .monthly("₹299", "a month")
                .monthly_note("Cancel any time")
                .yearly("₹2,990", "a year")
                .yearly_note("₹249 a month, billed once a year")
                .feature("Everything in Free")
                .feature("Trends over a whole year")
                .feature("A PDF for your doctor")
                .feature("Correlations checked for chance")
                .action(
                    Button::primary("Start Plus").href("/site/contact?plan=plus&billing=monthly"),
                )
                .yearly_action(
                    Button::primary("Start Plus").href("/site/contact?plan=plus&billing=yearly"),
                )
                .featured("Most popular"),
        )
        .plan(
            Plan::new("Family")
                .description("Up to five people, each with their own data.")
                .monthly("₹499", "a month")
                .yearly("₹4,990", "a year")
                .yearly_note("₹416 a month, billed once a year")
                .feature("Everything in Plus, for five")
                .feature("A shared weekly summary, if each person agrees")
                .feature("Priority help in Hindi, Punjabi or English")
                .action(Button::outline("Start Family").href("/site/contact?plan=family")),
        )
        .footnote(
            "Prices in Indian rupees, including GST. Cancel from Settings and keep your data.",
        );
    if yearly { pricing.yearly() } else { pricing }
}

/// The questions on the home page. Each answer is 40 to 80 words, as answer engines quote.
fn questions() -> Faq {
    Faq::new("questions")
        .exclusive()
        .item(
            "Do I need a smartwatch to use Kesar?",
            "No. Kesar reads the steps, sleep and heart rate that your Android phone already records through Health Connect. A watch or a ring adds detail, such as heart rate variability through the night, and Kesar uses it when you have one, but every score works from the phone alone, and you never need to buy new hardware to begin.",
        )
        .item(
            "How is the daily score worked out?",
            "The daily score weighs last night's sleep, your resting heart rate against your own two-week baseline, and how much you moved yesterday. Each part is shown with its number and its weight, so you can see why the score moved. It is an estimate for your own wellness, written in plain words, and never a medical diagnosis or advice.",
        )
        .item(
            "Is Kesar a medical device?",
            "No. Kesar is a wellness guide. It shows patterns in your own data, such as how late nights change your resting heart rate, but it does not diagnose, treat or prevent any illness. If something worries you, or a number looks wrong for days, please speak to a doctor. Kesar can export a short PDF for that visit.",
        )
        .item(
            "Where is my data stored, and who can see it?",
            "Your data is stored on servers in Mumbai, encrypted at rest and in transit. Only you can see it, unless you share a summary with your family plan or export a PDF yourself. Kesar never sells your data, never shows advertising, and deletes everything within thirty days when you close your account from Settings.",
        )
        .item(
            "How much does Kesar cost?",
            "Kesar is free for one person, with the daily score and thirty days of history. Plus costs ₹299 a month or ₹2,990 a year and adds a year of trends and the doctor's PDF. Family costs ₹499 a month for up to five people. Prices include GST, and you can cancel from Settings at any time.",
        )
        .item(
            "Which languages does Kesar speak?",
            "Kesar's app and its help are in English, Hindi and Punjabi, and you can switch at any time from Settings. Numbers are written the Indian way, in lakh and crore, with times in Indian Standard Time. Our support team answers in any of the three languages, usually within one working day, from Mohali.",
        )
}

fn home(look: Appearance, query: &SiteQuery, letter: Option<FormView<Subscribe>>) -> View {
    let faq = questions();
    let subscribed = query.subscribed.is_some();
    let letter_form = letter.unwrap_or_else(|| {
        FormView::<Subscribe>::new()
            .post("/site/subscribe#letter")
            .submit("Subscribe")
    });
    let body = SitePage::new()
        .header(header("/site"))
        .section(
            Hero::new(("Know your body, ", accent("one calm score a day.")))
                .eyebrow("Health, read calmly")
                .lead("Kesar reads the sleep, heart and step data your phone already keeps, and turns it into one daily score with the reasons behind it. No new watch to buy.")
                .action(Button::primary("Start free").href("/site/pricing").icon_end(icons::ARROW_RIGHT))
                .action(Button::outline("See how it works").href("/site#how"))
                .note(icons::SHIELD_CHECK, "Data stored in India")
                .note(icons::SMARTPHONE, "No wearable needed")
                .note(icons::HEART, "Free for one person")
                .media(BrowserFrame::new(today()).address("app.kesar.example/today")),
        )
        .section(
            Section::new("trusted").compact().child(
                LogoCloud::new()
                    .title("Used by clinics, coaches and running clubs across North India")
                    .wordmark("Sutlej Clinics")
                    .wordmark("Doaba Fitness")
                    .wordmark("Ambala Runners")
                    .wordmark("Shivalik Yoga")
                    .wordmark("Jalandhar Physio"),
            ),
        )
        .section(
            Section::new("features")
                .heading(
                    SectionHeading::new(("Five lenses on one body. ", accent("All from data you have.")))
                        .eyebrow("What it reads")
                        .lead("Every number says how it was worked out, from your own baseline, not a stranger's average.")
                        .split(),
                )
                .child(
                    FeatureGrid::new()
                        .feature(Feature::new("Sleep in stages", "Deep, REM and light sleep, night by night, with how much sleep you owe.").icon(icons::BED))
                        .feature(Feature::new("Resting heart", "Your resting rate against your own two weeks, and what moves it.").icon(icons::HEART))
                        .feature(Feature::new("Movement", "Steps and active minutes, with a goal that follows how you slept.").icon(icons::FOOTPRINTS))
                        .feature(Feature::new("Recovery", "Six contributors, each shown with its weight, so a score is never a black box.").icon(icons::ACTIVITY))
                        .feature(Feature::new("Patterns", "Which habits move your numbers, checked for chance before Kesar says so.").icon(icons::CHART_LINE))
                        .feature(Feature::new("Private by design", "Stored in India, never sold, deleted when you leave.").icon(icons::LOCK).link("Read the privacy policy", "/site/privacy")),
                ),
        )
        .section(
            Section::new("how")
                .tone(SectionTone::Muted)
                .heading(
                    SectionHeading::new("How Kesar reads you")
                        .eyebrow("How it works")
                        .lead("Three things a morning check should tell you, each from the phone in your pocket."),
                )
                .child(
                    FeatureRows::new()
                        .row(
                            FeatureRow::new("Last night, in stages")
                                .eyebrow("Sleep")
                                .text("Kesar splits the night into deep, REM and light sleep and shows what you owe from the week before.")
                                .point("Efficiency, debt and timing")
                                .point("A goal that moves with your week")
                                .media(PhoneFrame::new(
                                    Picture::new("/site/img/sleep-780.png", "Last night: a sleep score of 84, 7 hours 36 minutes in four stages", 780, 1688)
                                        .srcset([(390, "/site/img/sleep-390.png"), (780, "/site/img/sleep-780.png")])
                                        .avif([(390, "/site/img/sleep-390.avif"), (780, "/site/img/sleep-780.avif")])
                                        .webp([(390, "/site/img/sleep-390.webp"), (780, "/site/img/sleep-780.webp")])
                                        .sizes("18.5rem"),
                                )),
                        )
                        .row(
                            FeatureRow::new("Your heart against your own baseline")
                                .eyebrow("Heart")
                                .text("A resting rate of 58 means little alone. Kesar compares it with your last two weeks and says when it drifts.")
                                .point("Two-week baseline, updated daily")
                                .point("Late meals and late nights, named")
                                .action(Button::outline("See the daily score").href("/site#features"))
                                .media(
                                    Card::titled("Resting heart rate").description("Last 30 days, beats a minute").body(
                                        Sparkline::new(
                                            "Resting heart rate, last 30 days",
                                            [64, 63, 63, 62, 64, 63, 62, 61, 61, 62, 60, 61, 60, 59, 60, 61, 60, 59, 59, 58, 60, 59, 58, 58, 59, 57, 58, 58, 57, 58],
                                        )
                                        .id("row-heart")
                                        .area()
                                        .smooth()
                                        .ink(Ink::slot(2))
                                        .height(120),
                                    ),
                                ),
                        )
                        .row(
                            FeatureRow::new("Move with how you slept")
                                .eyebrow("Movement")
                                .text("After a short night your step goal eases; after a good one it asks a little more.")
                                .point("Steps, active minutes and stand hours")
                                .media(
                                    Card::new().body(
                                        Rings::new("Activity today")
                                            .id("row-rings")
                                            .ring(RingValue::new("Move", 540).max(600).unit("kcal").fill(Gradient::sheen(Ink::slot(8))))
                                            .ring(RingValue::new("Exercise", 38).max(30).unit("min").fill(Gradient::sheen(Ink::slot(3))))
                                            .ring(RingValue::new("Steps", 8_249).max(10_000).fill(Gradient::sheen(Ink::slot(2))))
                                            .track(RingTrack::Tinted)
                                            .legend()
                                            .size(200),
                                    ),
                                ),
                        ),
                ),
        )
        .section(
            Section::new("numbers").compact().child(
                StatBand::new()
                    .stat_with("1.2 lakh", "nights read last month", "Across 9,400 people")
                    .stat_with("20+", "measures from one phone", "Through Health Connect")
                    .stat_with("6", "contributors to recovery", "Each with its weight")
                    .stat_with("0", "records sold or shared", "Ever"),
            ),
        )
        .section(
            Section::new("voices")
                .heading(SectionHeading::new("People who read their mornings").eyebrow("In their words").center())
                .child(
                    Testimonials::new()
                        .item(Testimonial::new("I finally know why Mondays feel heavy: Sunday nights. Two weeks of earlier dinners and my resting heart rate came down by four.").author("Simran Kaur Gill").role("Teacher, Ludhiana").initials("SG").rating(5).featured())
                        .item(Testimonial::new("No ring, no watch. My old phone does it all, and the score explains itself.").author("Arjun Malhotra").role("Software engineer, Gurugram").initials("AM").rating(5))
                        .item(Testimonial::new("I recommend it to patients who want to see their recovery between visits. The PDF is short and honest.").author("Neha Sharma").role("Physiotherapist, Chandigarh").initials("NS").rating(5))
                        .item(Testimonial::new("Training for my first marathon at fifty-one. Kesar told me when to rest before my knees did.").author("Harjit Singh Sandhu").role("Farmer and runner, Moga").initials("HS").rating(4))
                        .item(Testimonial::new("The Hindi is natural, not translated. My mother reads her own sleep now.").author("Kavya Agarwal").role("Product designer, Jaipur").initials("KA").rating(5))
                        .item(Testimonial::new("Our family plan's weekly summary is the one message everyone reads.").author("Rajat Khanna").role("Retired banker, Amritsar").initials("RK").rating(5)),
                ),
        )
        .section(
            Section::new("pricing")
                .tone(SectionTone::Muted)
                .heading(
                    SectionHeading::new("Start free. Pay when it earns it.")
                        .eyebrow("Pricing")
                        .lead("Every plan reads the same data the same way. Plus and Family keep more of it and share it as you choose.")
                        .center(),
                )
                .child(rupee_plans(query.billing.as_deref() == Some("yearly"))),
        )
        .section(
            Section::new("faq")
                .narrow()
                .heading(SectionHeading::new("Questions, answered").eyebrow("FAQ").center())
                .child(faq.clone()),
        )
        .section(
            Newsletter::new("letter", "A calm letter, once a month")
                .lead(if subscribed {
                    "Thank you. The next letter comes on the first Sunday of the month."
                } else {
                    "What changed in Kesar, and one thing worth knowing about sleep. In English, Hindi or Punjabi."
                })
                .form(letter_form)
                .note("No spam. Leave with one click from any letter.")
                .inline(),
        )
        .section(
            CtaBand::new("start", ("Your data is already there. ", accent("Start reading it.")))
                .lead("Free for one person, for ever. Two minutes to connect Health Connect.")
                .action(Button::primary("Build my dashboard").href("/site/pricing").icon_end(icons::ARROW_RIGHT))
                .action(Button::outline("Talk to us").href("/site/contact"))
                .note("No card needed. Android 9 or newer, or any browser."),
        )
        .footer(footer(look.mode(), "/site"));
    let description = "Kesar reads the sleep, heart and step data your phone already keeps and turns it into one calm daily score. Free for one person.";
    page(
        look,
        "Kesar: sleep, heart and steps in one daily score",
        seo(
            "/site",
            "Kesar",
            description,
            PageKind::WebPage,
            vec![app(), faq.faq_page(format!("{SITE}/site")).into()],
        ),
        body,
    )
}

fn pricing_page(look: Appearance, query: &SiteQuery) -> View {
    let faq = Faq::new("pricing-questions")
        .item(
            "Can I change plans later?",
            "Yes. Move between Free, Plus and Family from Settings at any time. When you move up, you pay only for the days left in your period; when you move down, the change starts at the end of the period you have paid for, and your history stays.",
        )
        .item(
            "Do prices include GST?",
            "Yes. Every price on this page is in Indian rupees and includes GST at the current rate. Your invoice shows the tax separately, with Kesar Health's GSTIN, and is emailed to you and kept in Settings for download whenever you need it for your records.",
        );
    let body = SitePage::new()
        .header(header("/site/pricing"))
        .section(
            Hero::new("Plans in rupees, for one person or five")
                .eyebrow("Pricing")
                .lead("Start free, for ever. Plus and Family add a year of trends, the doctor's PDF and help in three languages.")
                .backdrop(Backdrop::Grid),
        )
        .section(
            Section::new("plans-section")
                .heading(SectionHeading::new("Every plan reads the same data the same way").center())
                .child(rupee_plans(query.billing.as_deref() == Some("yearly"))),
        )
        .section(
            Section::new("pricing-faq")
                .narrow()
                .heading(SectionHeading::new("About billing").center())
                .child(faq.clone()),
        )
        .section(
            CtaBand::new("start", "Try Plus free for 14 days")
                .action(Button::primary("Start the trial").href("/site/contact?plan=plus")),
        )
        .footer(footer(look.mode(), "/site/pricing"));
    let description = "Kesar is free for one person. Plus is ₹299 a month or ₹2,990 a year; Family covers five people for ₹499 a month. GST included.";
    page(
        look,
        "Kesar pricing: free, Plus and Family plans in rupees",
        seo(
            "/site/pricing",
            "Pricing",
            description,
            PageKind::ItemPage,
            vec![app(), faq.faq_page(format!("{SITE}/site/pricing")).into()],
        ),
        body,
    )
}

fn privacy_page(look: Appearance) -> View {
    let legal = LegalPage::new("Privacy policy")
        .eyebrow("Legal")
        .intro("What Kesar collects, why, where it is kept and how to take it back. In plain words, without the small print.")
        .updated("Last updated 10 November 2026")
        .section(
            LegalSection::new("collect", "1. What we collect")
                .text("From your phone, with your permission through Health Connect: steps, sleep sessions and stages, heart rate, resting heart rate and, if a watch provides it, heart rate variability.")
                .points(["Your name and email address, to sign you in.", "The plan you pay for, and its invoices.", "Nothing from your contacts, photos, location or microphone."]),
        )
        .section(
            LegalSection::new("use", "2. How we use it")
                .text("To work out your scores and trends, and to show them to you. We never sell your data, never show advertising and never train models on it for anyone else.")
                .body(el("p").children(("Every formula behind a score is written in the app, under ", el("strong").child("How this is worked out"), "."))),
        )
        .section(
            LegalSection::new("where", "3. Where it is kept")
                .text("On servers in Mumbai, encrypted at rest with keys held in India and in transit with TLS 1.3. Backups are kept for thirty days in the same region."),
        )
        .section(
            LegalSection::new("share", "4. Who we share it with")
                .text("No one, unless you choose to: a family member on your plan sees only the weekly summary they are invited to, and a PDF you export goes where you send it.")
                .points(["Payment: Razorpay processes card and UPI payments and sees no health data.", "Email: our letter is sent from our own servers."]),
        )
        .section(
            LegalSection::new("rights", "5. Your rights")
                .text("You can see, export or delete everything from Settings at any time. Deleting your account removes your data within thirty days, backups included.")
                .body(el("p").children(("Write to ", el("a").href("mailto:privacy@kesar.example").child("privacy@kesar.example"), " for anything Settings does not do."))),
        )
        .section(
            LegalSection::new("children", "6. Children")
                .text("Kesar is for people of eighteen and over. We delete any account we learn belongs to someone younger."),
        )
        .section(
            LegalSection::new("changes", "7. Changes to this policy")
                .text("We will email you thirty days before any change that affects what we collect or who sees it, and keep every earlier version on this page."),
        );
    let body = SitePage::new()
        .header(header("/site/privacy"))
        .section(legal)
        .footer(footer(look.mode(), "/site/privacy"));
    page(
        look,
        "Kesar privacy policy: what we keep and how to delete it",
        seo(
            "/site/privacy",
            "Privacy policy",
            "What Kesar collects from your phone, why, where it is kept in India, who sees it, and how to export or delete all of it.",
            PageKind::WebPage,
            Vec::new(),
        ),
        body,
    )
}

fn contact_page(
    look: Appearance,
    query: &SiteQuery,
    form: Option<FormView<ContactMessage>>,
) -> View {
    let form = form.unwrap_or_else(|| {
        FormView::<ContactMessage>::new()
            .post("/site/contact#write")
            .submit("Send message")
    });
    let lead = if query.sent.is_some() {
        "Thank you. We answer within one working day, in the language you wrote in."
    } else {
        "Questions about a plan, your data or the app. We answer within one working day, in English, Hindi or Punjabi."
    };
    let body = SitePage::new()
        .header(header("/site/contact"))
        .section(
            Hero::new("We read every message")
                .eyebrow("Contact")
                .lead("Kesar Health Pvt. Ltd., Plot 14, Sector 82, JLPL Industrial Area, Mohali, Punjab 140306.")
                .note(icons::MAIL, "namaste@kesar.example")
                .note(icons::GLOBE, "Mon to Sat, 10 am to 6 pm IST")
                .backdrop(Backdrop::None),
        )
        .section(Newsletter::new("write", "Write to us").lead(lead).form(form))
        .footer(footer(look.mode(), "/site/contact"));
    page(
        look,
        "Contact Kesar: help in English, Hindi or Punjabi",
        seo(
            "/site/contact",
            "Contact",
            "Write to Kesar about a plan, your data or the app. We answer within one working day, in English, Hindi or Punjabi, from Mohali.",
            PageKind::ContactPage,
            Vec::new(),
        ),
        body,
    )
}

fn screen_page(look: Appearance) -> View {
    View::new("Kesar: last night")
        .appearance(look)
        .seo(Seo::new("The phone screen the home page shows.").noindex())
        .body(SitePage::new().section(Section::new("screen").child(screen())))
}

fn sitemap() -> Sitemap {
    ["/site", "/site/pricing", "/site/privacy", "/site/contact"]
        .into_iter()
        .fold(Sitemap::new(), |map, path| {
            map.url(SitemapUrl::new(format!("{SITE}{path}")).lastmod(UPDATED))
        })
}

fn robots() -> RobotsTxt {
    RobotsTxt::new()
        .disallow("/site/screen")
        .welcome(AI_CRAWLERS)
        .sitemap(format!("{SITE}/site/sitemap.xml"))
}

fn image(name: &str) -> Response {
    match SCREEN.iter().find(|(file, _, _)| *file == name) {
        Some((_, kind, bytes)) => (
            [
                (header::CONTENT_TYPE, HeaderValue::from_static(kind)),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                ),
            ],
            *bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub struct SitePages;

impl Injectable for SitePages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for SitePages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/site", |headers: HeaderMap, Query(query): Query<SiteQuery>| async move {
                home(look(&headers, &query), &query, None)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/site/subscribe",
                |headers: HeaderMap, submit: Result<Submit<Subscribe>, Rejected<Subscribe>>| async move {
                    match submit {
                        Ok(_) => navree::ui::Reply::redirect("/site?subscribed=1#letter").into_response(),
                        Err(rejected) => rejected.respond(|form| {
                            let query = SiteQuery::default();
                            home(look(&headers, &query), &query, Some(form))
                        }),
                    }
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/site/pricing", |headers: HeaderMap, Query(query): Query<SiteQuery>| async move {
                pricing_page(look(&headers, &query), &query)
            })
            .public(),
            RouteDef::on(Method::GET, "!/site/privacy", |headers: HeaderMap, Query(query): Query<SiteQuery>| async move {
                privacy_page(look(&headers, &query))
            })
            .public(),
            RouteDef::on(Method::GET, "!/site/contact", |headers: HeaderMap, Query(query): Query<SiteQuery>| async move {
                contact_page(look(&headers, &query), &query, None)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/site/contact",
                |headers: HeaderMap, submit: Result<Submit<ContactMessage>, Rejected<ContactMessage>>| async move {
                    match submit {
                        Ok(_) => navree::ui::Reply::redirect("/site/contact?sent=1#write").into_response(),
                        Err(rejected) => rejected.respond(|form| {
                            let query = SiteQuery::default();
                            contact_page(look(&headers, &query), &query, Some(form))
                        }),
                    }
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/site/screen", |headers: HeaderMap, Query(query): Query<SiteQuery>| async move {
                screen_page(look(&headers, &query))
            })
            .public(),
            RouteDef::on(Method::GET, "!/site/sitemap.xml", || async { sitemap() }).public(),
            RouteDef::on(Method::GET, "!/site/robots.txt", || async { robots() }).public(),
            RouteDef::on(Method::GET, "!/site/img/{file}", |Path(file): Path<String>| async move {
                image(&file)
            })
            .public(),
        ]
    }
}
