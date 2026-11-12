//! The blocks of a public website: Kesar, an invented health app from Mohali, with its
//! header, hero, sections, features, numbers, frames, testimonials, pricing in rupees,
//! questions, sign-up band, legal page and footer; and the responsive picture.
#![allow(clippy::inconsistent_digit_grouping)]

use navree::ui::charts::*;
use navree::ui::marketing::*;
use navree::ui::theme::Mode;
use navree::ui::*;
use serde::Deserialize;

use crate::catalog::{Example, single};

const SOURCE: &str = include_str!("marketing.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("site-page", "default", "A whole page", site_page_default)
            .fill()
            .fit()
            .width(1280),
        single!(
            "site-header",
            "default",
            "Links, actions and a brand",
            site_header_default
        )
        .fill()
        .fit()
        .width(1280),
        single!(
            "site-header",
            "phone",
            "Folded into a menu on a phone",
            site_header_phone
        )
        .fill()
        .fit()
        .width(390),
        single!("hero", "default", "Words beside the product", hero_default)
            .fill()
            .fit()
            .width(1280),
        single!("hero", "centered", "Centred, over a grid", hero_centered)
            .fill()
            .fit()
            .width(1280),
        single!(
            "section",
            "default",
            "A heading and its content",
            section_default
        )
        .fill()
        .fit()
        .width(1280),
        single!(
            "logo-cloud",
            "default",
            "Names as wordmarks",
            logo_cloud_default
        )
        .start()
        .width(1024),
        single!(
            "feature-grid",
            "default",
            "Six features in three columns",
            feature_grid_default
        )
        .start()
        .width(1200),
        single!(
            "feature-grid",
            "bento",
            "A wide feature with a chart",
            feature_grid_bento
        )
        .start()
        .width(1200),
        single!(
            "feature-rows",
            "default",
            "Rows that alternate",
            feature_rows_default
        )
        .start()
        .width(1200),
        single!("stat-band", "default", "Four numbers", stat_band_default)
            .start()
            .width(1024),
        single!(
            "device-frames",
            "default",
            "A browser and a phone",
            device_frames_default
        )
        .start()
        .width(1100),
        single!(
            "device-frames",
            "modes",
            "Dark and light whatever the page",
            device_frames_modes
        )
        .start()
        .width(1100),
        single!(
            "testimonials",
            "default",
            "Quotes in columns",
            testimonials_default
        )
        .start()
        .width(1200),
        single!(
            "pricing",
            "default",
            "Three plans, monthly or yearly",
            pricing_default
        )
        .start()
        .width(1200),
        single!("pricing", "yearly", "Yearly first", pricing_yearly)
            .start()
            .width(1200),
        single!(
            "faq",
            "default",
            "Questions, one open at a time",
            faq_default
        )
        .start()
        .width(820),
        single!("cta-band", "default", "A call to action", cta_band_default)
            .fill()
            .fit()
            .width(1280),
        single!(
            "newsletter",
            "default",
            "A sign-up in one line",
            newsletter_default
        )
        .fill()
        .fit()
        .width(1280),
        single!(
            "legal-page",
            "default",
            "A policy with its contents",
            legal_page_default
        )
        .fill()
        .fit()
        .width(1280),
        single!(
            "site-footer",
            "default",
            "Columns and a theme toggle",
            site_footer_default
        )
        .fill()
        .fit()
        .width(1280),
        single!("picture", "default", "A responsive image", picture_default)
            .start()
            .width(820),
    ]
}

/// The sign-up form of the newsletter example.
#[derive(Debug, Deserialize, Validate, Form)]
struct Subscribe {
    /// Email
    #[validate(email)]
    #[form(placeholder = "you@example.in")]
    email: String,
}

/// The brand every example shares.
fn kesar() -> Brand {
    Brand::new("Kesar", "/").icon(icons::SUN)
}

/// What the app looks like today, drawn by the toolkit.
fn today() -> Grid {
    Grid::cols(2).gap(3).children((
        Card::new().body(
            Ring::new("Daily score", 82)
                .id("doc-today-score")
                .label("Daily score")
                .sublabel("Above your usual")
                .fill(Gradient::sheen(Ink::slot(2)))
                .size(140),
        ),
        Stack::gap(3).children((
            Stat::new("Sleep", "7 h 36 min").change(Trend::new(24).suffix(" min").decimals(0)),
            Stat::new("Resting heart", "58 bpm")
                .change(Trend::new(-2).suffix(" bpm").decimals(0).up_is_bad()),
        )),
    ))
}

fn site_page_default() -> SitePage {
    SitePage::new()
        .header(
            SiteHeader::new(kesar())
                .link("Features", "/#features")
                .link("Pricing", "/pricing/")
                .action(Button::primary("Start free").href("/signup")),
        )
        .section(
            Hero::new(("Know your body, ", accent("one calm score a day.")))
                .lead("Sleep, heart and steps from the phone you already carry.")
                .action(Button::primary("Start free").href("/signup")),
        )
        .section(
            Section::new("features")
                .heading(SectionHeading::new("What it reads").center())
                .child(
                    FeatureGrid::new()
                        .feature(Feature::new("Sleep", "Stages, night by night.").icon(icons::BED))
                        .feature(Feature::new("Heart", "Your own baseline.").icon(icons::HEART))
                        .feature(
                            Feature::new("Steps", "A goal that follows your sleep.")
                                .icon(icons::FOOTPRINTS),
                        ),
                ),
        )
        .footer(SiteFooter::new(kesar()).legal("© 2026 Kesar Health Pvt. Ltd., Mohali"))
}

fn site_header_default() -> SiteHeader {
    SiteHeader::new(kesar())
        .link("Features", "/#features")
        .link("How it works", "/#how")
        .current("Pricing", "/pricing/")
        .link("FAQ", "/#faq")
        .action(Button::ghost("Sign in").href("/login"))
        .action(Button::primary("Start free").href("/signup"))
}

fn site_header_phone() -> SiteHeader {
    SiteHeader::new(kesar())
        .link("Features", "/#features")
        .link("Pricing", "/pricing/")
        .action(Button::primary("Start free").href("/signup"))
        .menu_label("Menu")
}

fn hero_default() -> Hero {
    Hero::new(("Know your body, ", accent("one calm score a day.")))
        .eyebrow("Health, read calmly")
        .lead("Kesar reads the sleep, heart and step data your phone already keeps, and turns it into one daily score with the reasons behind it.")
        .action(Button::primary("Start free").href("/signup").icon_end(icons::ARROW_RIGHT))
        .action(Button::outline("See how it works").href("/#how"))
        .note(icons::SHIELD_CHECK, "Data stored in India")
        .note(icons::HEART, "Free for one person")
        .media(BrowserFrame::new(today()).address("app.kesar.example/today"))
}

fn hero_centered() -> Hero {
    Hero::new("Plans in rupees, for one person or five")
        .eyebrow("Pricing")
        .lead("Start free, for ever. Plus and Family add a year of trends and help in three languages.")
        .action(Button::primary("Compare plans").href("/pricing/#plans"))
        .centered()
        .backdrop(Backdrop::Grid)
}

fn section_default() -> Section {
    Section::new("features")
        .heading(
            SectionHeading::new((
                "Five lenses on one body. ",
                accent("All from data you have."),
            ))
            .eyebrow("What it reads")
            .lead("Every number says how it was worked out, from your own baseline.")
            .split(),
        )
        .child(
            FeatureGrid::new()
                .feature(
                    Feature::new(
                        "Sleep in stages",
                        "Deep, REM and light sleep, night by night.",
                    )
                    .icon(icons::BED),
                )
                .feature(
                    Feature::new("Resting heart", "Against your own two weeks.").icon(icons::HEART),
                )
                .feature(
                    Feature::new("Movement", "A goal that follows how you slept.")
                        .icon(icons::FOOTPRINTS),
                ),
        )
}

fn logo_cloud_default() -> LogoCloud {
    LogoCloud::new()
        .title("Used by clinics, coaches and running clubs across North India")
        .wordmark("Sutlej Clinics")
        .wordmark("Doaba Fitness")
        .wordmark("Ambala Runners")
        .wordmark("Shivalik Yoga")
}

fn feature_grid_default() -> FeatureGrid {
    FeatureGrid::new()
        .cols(3)
        .feature(
            Feature::new(
                "Sleep in stages",
                "Deep, REM and light sleep, with how much sleep you owe.",
            )
            .icon(icons::BED),
        )
        .feature(
            Feature::new(
                "Resting heart",
                "Your resting rate against your own two weeks.",
            )
            .icon(icons::HEART),
        )
        .feature(
            Feature::new(
                "Movement",
                "Steps and active minutes, with a goal that follows your sleep.",
            )
            .icon(icons::FOOTPRINTS),
        )
        .feature(
            Feature::new("Recovery", "Six contributors, each shown with its weight.")
                .icon(icons::ACTIVITY),
        )
        .feature(
            Feature::new(
                "Patterns",
                "Which habits move your numbers, checked for chance.",
            )
            .icon(icons::CHART_LINE),
        )
        .feature(
            Feature::new(
                "Private by design",
                "Stored in India, never sold, deleted when you leave.",
            )
            .icon(icons::LOCK)
            .link("Read the privacy policy", "/privacy/"),
        )
}

fn feature_grid_bento() -> FeatureGrid {
    FeatureGrid::new()
        .cols(3)
        .feature(
            Feature::new(
                "Resting heart, 30 days",
                "A slow fall after two weeks of earlier dinners.",
            )
            .icon(icons::HEART)
            .wide()
            .media(
                Sparkline::new(
                    "Resting heart rate, last 30 days",
                    [
                        64, 63, 63, 62, 64, 62, 61, 61, 60, 61, 60, 59, 60, 59, 58, 59, 58, 58, 57,
                        58,
                    ],
                )
                .id("doc-bento-heart")
                .area()
                .smooth()
                .height(96),
            ),
        )
        .feature(
            Feature::new("Daily score", "One number, with its reasons.")
                .icon(icons::SUN)
                .media(Ring::new("Daily score", 82).id("doc-bento-score").size(120)),
        )
}

fn feature_rows_default() -> FeatureRows {
    FeatureRows::new()
        .row(
            FeatureRow::new("Last night, in stages")
                .eyebrow("Sleep")
                .text(
                    "Kesar splits the night into deep, REM and light sleep and shows what you owe.",
                )
                .point("Efficiency, debt and timing")
                .point("A goal that moves with your week")
                .media(
                    Card::titled("Last night").body(
                        Meter::new("Sleep stages")
                            .id("doc-rows-sleep")
                            .minutes()
                            .segment(Segment::new("Deep", 92).ink(Ink::slot(7)))
                            .segment(Segment::new("REM", 98).ink(Ink::slot(1)))
                            .segment(Segment::new("Light", 215).ink(Ink::slot(3)))
                            .segment(Segment::new("Awake", 31).ink(Ink::slot(4))),
                    ),
                ),
        )
        .row(
            FeatureRow::new("Your heart against your own baseline")
                .eyebrow("Heart")
                .text("Kesar compares today's resting rate with your last two weeks.")
                .action(Button::outline("See the daily score").href("/#score"))
                .media(Card::new().body(Stat::new("Resting heart", "58 bpm"))),
        )
}

fn stat_band_default() -> StatBand {
    StatBand::new()
        .stat_with("1.2 lakh", "nights read last month", "Across 9,400 people")
        .stat_with("20+", "measures from one phone", "Through Health Connect")
        .stat_with("6", "contributors to recovery", "Each with its weight")
        .stat_with("0", "records sold or shared", "Ever")
}

fn device_frames_default() -> Grid {
    Grid::cols(3).gap(8).children((
        BrowserFrame::new(today())
            .address("app.kesar.example/today")
            .span(2),
        PhoneFrame::new(
            Stack::gap(3).children((
                Heading::h3("Last night"),
                Ring::new("Sleep score", 84)
                    .id("doc-phone-ring")
                    .label("Sleep")
                    .size(140),
            )),
        ),
    ))
}

fn device_frames_modes() -> Grid {
    Grid::cols(2).gap(8).children((
        BrowserFrame::new(today())
            .address("app.kesar.example/today")
            .mode(Mode::Dark)
            .caption("Always dark"),
        BrowserFrame::new(today())
            .address("app.kesar.example/today")
            .mode(Mode::Light)
            .caption("Always light"),
    ))
}

fn testimonials_default() -> Testimonials {
    Testimonials::new()
        .item(
            Testimonial::new("I finally know why Mondays feel heavy: Sunday nights. Two weeks of earlier dinners and my resting heart rate came down by four.")
                .author("Simran Kaur Gill")
                .role("Teacher, Ludhiana")
                .initials("SG")
                .rating(5)
                .featured(),
        )
        .item(
            Testimonial::new("No ring, no watch. My old phone does it all, and the score explains itself.")
                .author("Arjun Malhotra")
                .role("Software engineer, Gurugram")
                .initials("AM")
                .rating(5),
        )
        .item(
            Testimonial::new("Training for my first marathon at fifty-one. Kesar told me when to rest before my knees did.")
                .author("Harjit Singh Sandhu")
                .role("Farmer and runner, Moga")
                .initials("HS")
                .rating(4),
        )
}

fn plans() -> Pricing {
    Pricing::new("plans")
        .yearly_badge("2 months free")
        .plan(
            Plan::new("Free")
                .description("One person, the essentials.")
                .price("₹0", "for ever")
                .feature("Daily score and its reasons")
                .feature("30 days of history")
                .action(Button::outline("Start free").href("/signup")),
        )
        .plan(
            Plan::new("Plus")
                .description("The whole picture for one person.")
                .monthly("₹299", "a month")
                .yearly("₹2,990", "a year")
                .yearly_note("₹249 a month, billed once a year")
                .feature("A year of trends")
                .feature("A PDF for your doctor")
                .action(Button::primary("Start Plus").href("/signup?plan=plus"))
                .featured("Most popular"),
        )
        .plan(
            Plan::new("Family")
                .description("Up to five people.")
                .monthly("₹499", "a month")
                .yearly("₹4,990", "a year")
                .feature("Everything in Plus, for five")
                .action(Button::outline("Start Family").href("/signup?plan=family")),
        )
        .footnote("Prices in Indian rupees, including GST.")
}

fn pricing_default() -> Pricing {
    plans()
}

fn pricing_yearly() -> Pricing {
    plans().yearly()
}

fn faq_default() -> Faq {
    Faq::new("faq")
        .exclusive()
        .open_first()
        .item(
            "Do I need a smartwatch to use Kesar?",
            "No. Kesar reads the steps, sleep and heart rate your phone already records. A watch adds detail when you have one.",
        )
        .item(
            "Is Kesar a medical device?",
            "No. Kesar is a wellness guide. Its scores are estimates from your own data, never a diagnosis.",
        )
        .item(
            "How much does Kesar cost?",
            "Free for one person. Plus is ₹299 a month or ₹2,990 a year, and Family is ₹499 a month for five people.",
        )
}

fn cta_band_default() -> CtaBand {
    CtaBand::new(
        "start",
        ("Your data is already there. ", accent("Start reading it.")),
    )
    .lead("Free for one person, for ever. Two minutes to connect.")
    .action(
        Button::primary("Build my dashboard")
            .href("/signup")
            .icon_end(icons::ARROW_RIGHT),
    )
    .action(Button::outline("Talk to us").href("/contact/"))
    .note("No card needed.")
}

fn newsletter_default() -> Newsletter {
    Newsletter::new("letter", "A calm letter, once a month")
        .lead("What changed in Kesar, and one thing worth knowing about sleep.")
        .form(
            FormView::<Subscribe>::new()
                .post("/subscribe")
                .submit("Subscribe"),
        )
        .note("No spam. Leave with one click from any letter.")
        .inline()
}

fn legal_page_default() -> LegalPage {
    LegalPage::new("Privacy policy")
        .eyebrow("Legal")
        .intro("What Kesar collects, why, where it is kept and how to take it back.")
        .updated("Last updated 10 November 2026")
        .section(
            LegalSection::new("collect", "1. What we collect")
                .text("From your phone, with your permission: steps, sleep and heart rate.")
                .points([
                    "Your name and email address, to sign you in.",
                    "Nothing from your contacts, photos or location.",
                ]),
        )
        .section(
            LegalSection::new("where", "2. Where it is kept")
                .text("On servers in Mumbai, encrypted at rest and in transit."),
        )
        .section(
            LegalSection::new("rights", "3. Your rights")
                .text("See, export or delete everything from Settings at any time."),
        )
}

fn site_footer_default() -> SiteFooter {
    SiteFooter::new(kesar())
        .tagline("Your sleep, heart and steps, read calmly. Made in Mohali.")
        .social(icons::GLOBE, "Kesar on the web", "https://kesar.example/")
        .social(
            icons::MAIL,
            "Write to Kesar",
            "mailto:namaste@kesar.example",
        )
        .column(
            FooterColumn::new("Product")
                .link("Features", "/#features")
                .link("Pricing", "/pricing/"),
        )
        .column(
            FooterColumn::new("Company")
                .link("Contact", "/contact/")
                .link("Privacy", "/privacy/"),
        )
        .legal("© 2026 Kesar Health Pvt. Ltd., Mohali, Punjab.")
        .legal("Kesar is a wellness guide, not a medical device.")
        .theme_toggle(ThemeToggle::new(Mode::System).back("/"))
}

fn picture_default() -> Picture {
    Picture::new(
        "../_frame/picture-800.svg",
        "Resting heart rate over 30 days, falling from 64 to 58 beats a minute",
        800,
        450,
    )
    .srcset([
        (400, "../_frame/picture-400.svg"),
        (800, "../_frame/picture-800.svg"),
    ])
    .sizes("(min-width: 50rem) 50rem, 100vw")
    .fill()
}
