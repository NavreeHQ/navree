//! A whole public page of marketing blocks, read the way search engines, answer engines
//! and assistive technology read it: one `h1`, headings that never skip a level, alt text
//! on every image, landmarks in order, links onward, questions whose visible answers are
//! the structured data's, and no script.

use navree_ui::marketing::{
    Brand, BrowserFrame, CtaBand, Faq, Feature, FeatureGrid, FeatureRow, FeatureRows, FooterColumn,
    Hero, LogoCloud, Newsletter, PhoneFrame, Plan, Pricing, Section, SectionHeading, SiteFooter,
    SiteHeader, SitePage, StatBand, Testimonial, Testimonials, ThemeToggle, accent,
};
use navree_ui::seo::{BreadcrumbList, JsonLd, Organization, Seo, SocialImage, WebPage, WebSite};
use navree_ui::theme::{Appearance, Mode, Theme};
use navree_ui::{Avatar, Button, Picture, View, el, icons};
use serde_json::Value;

const SITE: &str = "https://kesar.example";

fn faq() -> Faq {
    Faq::new("faq")
        .item(
            "Do I need a smartwatch to use Kesar?",
            "No. Kesar reads the steps, sleep and heart rate that your phone already records, and a watch only adds detail.",
        )
        .item(
            "Is Kesar a medical device?",
            "No. Kesar is a wellness guide. Its scores are estimates from your own data, never a diagnosis; see a doctor for that.",
        )
        .exclusive()
}

fn home() -> View {
    let org = format!("{SITE}/#organization");
    let website = format!("{SITE}/#website");
    let crumbs = format!("{SITE}/#breadcrumb");
    let page = SitePage::new()
        .header(
            SiteHeader::new(Brand::new("Kesar", "/").icon(icons::SUN))
                .link("Features", "/#features")
                .link("Pricing", "/pricing/")
                .link("FAQ", "/#faq")
                .action(Button::ghost("Sign in").href("/login"))
                .action(Button::primary("Start free").href("/signup")),
        )
        .section(
            Hero::new(("Know your body ", accent("calmly.")))
                .eyebrow("Health, read calmly")
                .lead("Sleep, heart and steps from the phone you carry.")
                .action(Button::primary("Start free").href("/signup"))
                .note(icons::LOCK, "Your data stays in India")
                .media(BrowserFrame::new(
                    Picture::new("/img/today.png", "Today: a score of 82", 1440, 900).priority(),
                )),
        )
        .section(
            Section::new("logos").child(
                LogoCloud::new()
                    .title("Used by clinics across Punjab")
                    .wordmark("Sutlej Clinics")
                    .logo(Picture::new("/img/doaba.svg", "Doaba Fitness", 120, 28)),
            ),
        )
        .section(
            Section::new("features")
                .heading(SectionHeading::new("Five lenses on one body").eyebrow("What it reads"))
                .child(
                    FeatureGrid::new()
                        .feature(Feature::new("Sleep", "Stages by night.").icon(icons::BED))
                        .feature(Feature::new("Heart", "Resting rate.").icon(icons::HEART)),
                ),
        )
        .section(
            Section::new("how")
                .heading(SectionHeading::new("How it reads you"))
                .child(
                    FeatureRows::new().row(
                        FeatureRow::new("Recovery you can trust")
                            .point("Six contributors")
                            .media(PhoneFrame::new(Picture::new(
                                "/img/r.png",
                                "Recovery at 87",
                                390,
                                844,
                            ))),
                    ),
                ),
        )
        .section(Section::new("numbers").child(StatBand::new().stat("1.2 lakh", "nights read")))
        .section(
            Section::new("voices")
                .heading(SectionHeading::new("What people say"))
                .child(
                    Testimonials::new().item(
                        Testimonial::new("Sunday nights explain my Mondays.")
                            .author("Simran Kaur Gill")
                            .role("Teacher, Ludhiana")
                            .avatar(Avatar::placeholder("SG"))
                            .rating(5),
                    ),
                ),
        )
        .section(
            Section::new("pricing")
                .heading(SectionHeading::new("Plans"))
                .child(
                    Pricing::new("plans")
                        .plan(Plan::new("Free").price("₹0", "for ever"))
                        .plan(
                            Plan::new("Plus")
                                .monthly("₹299", "a month")
                                .yearly("₹2,990", "a year"),
                        ),
                ),
        )
        .section(
            Section::new("faq-section")
                .heading(SectionHeading::new("Questions"))
                .child(faq()),
        )
        .section(
            Newsletter::new("letter", "A calm letter")
                .form(el("form").attr("action", "/subscribe")),
        )
        .section(
            CtaBand::new("cta", "Start reading your body")
                .action(Button::primary("Start").href("/signup")),
        )
        .footer(
            SiteFooter::new(Brand::new("Kesar", "/"))
                .column(FooterColumn::new("Product").link("Pricing", "/pricing/"))
                .column(
                    FooterColumn::new("Company")
                        .link("Privacy", "/privacy/")
                        .link("Terms", "/terms/"),
                )
                .legal("© 2026 Kesar Health Pvt. Ltd., Mohali")
                .theme_toggle(ThemeToggle::new(Mode::System).back("/")),
        );
    View::new("Kesar: sleep, heart and steps read calmly")
        .appearance(Appearance::new(Theme::OCEAN, Mode::System))
        .seo(
            Seo::new("Kesar reads the sleep, heart and step data your phone already has and turns it into one calm score a day.")
                .canonical(format!("{SITE}/"))
                .site_name("Kesar")
                .locale("en_IN")
                .image(SocialImage::new(format!("{SITE}/og.png"), 1200, 630, "Kesar's daily score"))
                .json_ld(
                    JsonLd::new()
                        .node(
                            Organization::new("Kesar", format!("{SITE}/"))
                                .id(&org)
                                .logo(format!("{SITE}/logo.png"))
                                .same_as("https://www.linkedin.com/company/kesar-health"),
                        )
                        .node(WebSite::new("Kesar", format!("{SITE}/")).id(&website).publisher(&org))
                        .node(
                            WebPage::new(format!("{SITE}/"), "Kesar", "Health, read calmly.")
                                .date_modified("2026-11-12")
                                .is_part_of(&website)
                                .breadcrumb(&crumbs),
                        )
                        .node(BreadcrumbList::new().id(&crumbs).step("Home", format!("{SITE}/")))
                        .node(faq().faq_page(format!("{SITE}/"))),
                ),
        )
        .body(page)
}

/// Every heading's level and text, in order.
fn headings(html: &str) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("<h") {
        rest = &rest[at + 2..];
        let Some(level) = rest.chars().next().and_then(|c| c.to_digit(10)) else {
            continue;
        };
        let close = format!("</h{level}>");
        let start = rest.find('>').unwrap() + 1;
        let end = rest.find(&close).unwrap();
        let mut text = String::new();
        let mut inside = false;
        for ch in rest[start..end].chars() {
            match ch {
                '<' => inside = true,
                '>' => inside = false,
                c if !inside => text.push(c),
                _ => {}
            }
        }
        out.push((level as u8, text));
    }
    out
}

#[test]
fn a_page_has_one_h1_and_its_headings_never_skip_a_level() {
    let html = home().to_html();
    let body = &html[html.find("<body").unwrap()..];
    let all = headings(body);
    assert_eq!(
        all.iter().filter(|(level, _)| *level == 1).count(),
        1,
        "{all:?}"
    );
    assert_eq!(all[0], (1, "Know your body calmly.".to_owned()));
    for pair in all.windows(2) {
        assert!(
            pair[1].0 <= pair[0].0 + 1,
            "{} after {}: {all:?}",
            pair[1].1,
            pair[0].1
        );
    }
}

#[test]
fn every_image_has_alt_text_and_the_hero_loads_first() {
    let html = home().to_html();
    let images: Vec<&str> = html
        .split("<img ")
        .skip(1)
        .map(|rest| &rest[..rest.find('>').unwrap()])
        .collect();
    assert_eq!(images.len(), 3);
    for image in &images {
        assert!(image.contains("alt=\""), "{image}");
        assert!(
            image.contains("width=\"") && image.contains("height=\""),
            "{image}"
        );
    }
    assert_eq!(html.matches(r#"fetchpriority="high""#).count(), 1);
}

#[test]
fn landmarks_come_in_order_with_a_skip_link_first() {
    let html = home().to_html();
    let body = &html[html.find("<body").unwrap()..];
    let order = ["skip-link", "<header", "<main", "<footer"].map(|mark| body.find(mark).unwrap());
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    assert!(body.contains(r##"href="#main""##));
    assert!(body.contains(r#"id="main""#));
    let internal = body.matches("href=\"/").count();
    assert!(internal >= 3, "{internal} internal links");
}

#[test]
fn every_answer_on_the_page_is_the_structured_datas() {
    let view = home();
    let html = view.to_html();
    let start = html.find("<script type=\"application/ld+json\">").unwrap() + 35;
    let end = start + html[start..].find("</script>").unwrap();
    let data: Value = serde_json::from_str(&html[start..end]).unwrap();
    let faq = data["@graph"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["@type"] == "FAQPage")
        .unwrap();
    let squash = |text: &str| {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    let page = squash(&html[html.find("<body").unwrap()..]);
    for item in faq["mainEntity"].as_array().unwrap() {
        let question = item["name"].as_str().unwrap();
        let answer = item["acceptedAnswer"]["text"].as_str().unwrap();
        assert!(
            html.contains(&format!("<h3 data-slot=\"faq-title\">{question}</h3>")),
            "{question}"
        );
        assert!(page.contains(&squash(answer)), "{answer}");
    }
    assert!(view.seo_problems().is_empty(), "{:#?}", view.seo_problems());
}

#[test]
fn a_whole_site_page_loads_only_its_two_sheets_and_no_script() {
    let view = home();
    let html = view.to_html();
    assert!(!html.contains("<script type=\"module\""), "{html}");
    assert_eq!(html.matches("rel=\"stylesheet\"").count(), 2, "{html}");
    assert!(html.contains(r#"data-mode="system""#), "{html}");
}

#[test]
fn hostile_text_in_every_block_stays_text() {
    let nasty = "<script>window.__pwned=1</script>\"'><img src=x onerror=alert(1)>";
    let page = SitePage::new()
        .header(SiteHeader::new(Brand::new(nasty, "/")).link(nasty, "javascript:alert(1)"))
        .section(
            Hero::new(nasty)
                .eyebrow(nasty)
                .lead(nasty)
                .note(icons::LOCK, nasty),
        )
        .section(
            Section::new("s")
                .heading(SectionHeading::new(nasty).lead(nasty))
                .child((
                    FeatureGrid::new().feature(Feature::new(nasty, nasty)),
                    StatBand::new().stat(nasty, nasty),
                    Testimonials::new().item(Testimonial::new(nasty).author(nasty).role(nasty)),
                    Pricing::new("p")
                        .plan(Plan::new(nasty).monthly(nasty, nasty).yearly(nasty, nasty)),
                    Faq::new("f").item(nasty, nasty),
                    LogoCloud::new().wordmark(nasty),
                    BrowserFrame::new(nasty).address(nasty).caption(nasty),
                )),
        )
        .footer(
            SiteFooter::new(Brand::new(nasty, "/"))
                .legal(nasty)
                .column(FooterColumn::new(nasty).link(nasty, "javascript:x")),
        );
    let html = View::new("x").body(page).to_html();
    assert!(!html.contains("<script>"), "{html}");
    assert!(!html.contains("<img src=x"), "{html}");
    assert!(!html.contains("javascript:"), "{html}");
}
