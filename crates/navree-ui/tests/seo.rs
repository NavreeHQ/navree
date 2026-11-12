//! What a page tells search engines, link previews and answer engines: the head tags in
//! the form search engines and the site checks read, safe with hostile text, and the
//! structured data as valid JSON.

use navree_ui::seo::{
    AI_CRAWLERS, BreadcrumbList, FaqPage, JsonLd, Offer, OgType, Organization, PageKind, RobotsTxt,
    Seo, Sitemap, SitemapUrl, SocialImage, SoftwareApplication, WebPage, WebSite,
};
use navree_ui::{PAGE_CSP, View, el};
use serde_json::Value;

const SITE: &str = "https://kesar.example";

/// The pricing page of an invented health app, with everything a public page says.
fn pricing() -> View {
    let org = format!("{SITE}/#organization");
    let website = format!("{SITE}/#website");
    let url = format!("{SITE}/pricing/");
    let crumbs = format!("{url}#breadcrumb");
    View::new("Kesar pricing: free, Plus and Family plans in rupees")
        .lang("en")
        .seo(
            Seo::new("Kesar is free for one person. Plus adds trends and exports for ₹299 a month, and Family covers five people.")
                .canonical(&url)
                .site_name("Kesar")
                .locale("en_IN")
                .other_locale("hi_IN")
                .image(
                    SocialImage::new(format!("{SITE}/og/pricing.png"), 1200, 630, "Kesar's three plans side by side")
                        .mime("image/png"),
                )
                .twitter_site("@kesarhealth")
                .language("en-IN", &url)
                .language("hi-IN", format!("{SITE}/hi/pricing/"))
                .language("x-default", &url)
                .format("text/markdown", format!("{SITE}/pricing.md"))
                .json_ld(
                    JsonLd::new()
                        .add(
                            Organization::new("Kesar", format!("{SITE}/"))
                                .id(&org)
                                .logo(format!("{SITE}/logo.png"))
                                .same_as("https://www.linkedin.com/company/kesar-health")
                                .slogan("Your health data, read calmly."),
                        )
                        .add(
                            WebSite::new("Kesar", format!("{SITE}/"))
                                .id(&website)
                                .publisher(&org)
                                .in_language("en-IN"),
                        )
                        .add(
                            WebPage::new(&url, "Pricing", "Plans for one person and for a family.")
                                .kind(PageKind::ItemPage)
                                .in_language("en-IN")
                                .date_modified("2026-11-10")
                                .is_part_of(&website)
                                .breadcrumb(&crumbs),
                        )
                        .add(
                            SoftwareApplication::new("Kesar", "HealthApplication")
                                .description("Sleep, heart and steps in one daily score.")
                                .operating_system("Web, Android")
                                .free(true)
                                .publisher(&org)
                                .offer(Offer::new("0", "INR").name("Free"))
                                .offer(Offer::new("299", "INR").name("Plus, monthly")),
                        )
                        .add(
                            BreadcrumbList::new()
                                .id(&crumbs)
                                .step("Home", format!("{SITE}/"))
                                .step("Pricing", &url),
                        ),
                ),
        )
        .body(el("main").child(el("h1").child("Pricing")))
}

/// `<meta {attr}="{key}" content="...">`, as the site checks read it.
fn meta(html: &str, key: &str) -> Option<String> {
    for attr in ["name", "property"] {
        let open = format!("<meta {attr}=\"{key}\" content=\"");
        if let Some(at) = html.find(&open) {
            let rest = &html[at + open.len()..];
            return Some(rest[..rest.find('"').unwrap()].to_owned());
        }
    }
    None
}

/// The JSON of every structured data block.
fn blocks(html: &str) -> Vec<Value> {
    html.split("<script type=\"application/ld+json\">")
        .skip(1)
        .map(|rest| serde_json::from_str(&rest[..rest.find("</script>").unwrap()]).unwrap())
        .collect()
}

#[test]
fn a_public_page_writes_every_tag_a_search_result_and_a_preview_read() {
    let page = pricing();
    let html = page.to_html();
    let head = &html[..html.find("</head>").unwrap()];
    assert!(
        head.contains(r#"<link rel="canonical" href="https://kesar.example/pricing/">"#),
        "{head}"
    );
    assert_eq!(head.matches(r#"<link rel="canonical""#).count(), 1);
    assert!((70..=160).contains(&meta(head, "description").unwrap().chars().count()));
    assert!(
        meta(head, "robots")
            .unwrap()
            .contains("max-image-preview:large")
    );
    for key in [
        "og:title",
        "og:description",
        "og:image",
        "og:image:alt",
        "og:image:width",
        "og:image:height",
        "og:site_name",
        "og:type",
        "og:locale",
        "twitter:card",
        "twitter:title",
        "twitter:description",
        "twitter:image",
        "twitter:image:alt",
    ] {
        assert!(
            meta(head, key).is_some_and(|v| !v.is_empty()),
            "no {key}: {head}"
        );
    }
    assert_eq!(
        meta(head, "og:url").unwrap(),
        "https://kesar.example/pricing/"
    );
    assert_eq!(meta(head, "og:type").unwrap(), "website");
    assert_eq!(meta(head, "og:image:width").unwrap(), "1200");
    assert_eq!(meta(head, "og:image:type").unwrap(), "image/png");
    assert_eq!(meta(head, "og:locale:alternate").unwrap(), "hi_IN");
    assert_eq!(meta(head, "twitter:card").unwrap(), "summary_large_image");
    assert_eq!(meta(head, "twitter:site").unwrap(), "@kesarhealth");
    assert_eq!(
        meta(head, "og:title").unwrap(),
        "Kesar pricing: free, Plus and Family plans in rupees"
    );
    assert!(head.contains(
        r#"<link rel="alternate" hreflang="hi-IN" href="https://kesar.example/hi/pricing/">"#
    ));
    assert!(head.contains(
        r#"<link rel="alternate" hreflang="x-default" href="https://kesar.example/pricing/">"#
    ));
    assert!(head.contains(
        r#"<link rel="alternate" type="text/markdown" href="https://kesar.example/pricing.md">"#
    ));
    assert!(html.starts_with(r#"<!doctype html><html lang="en""#));
    // The tags come after the title and before the stylesheets.
    let title = head.find("</title>").unwrap();
    assert!(head.find("<meta name=\"description\"").unwrap() > title);
    assert!(
        head.find("<script type=\"application/ld+json\">").unwrap()
            < head.find("rel=\"stylesheet\"").unwrap()
    );
    assert!(page.seo_problems().is_empty(), "{:#?}", page.seo_problems());
}

#[test]
fn structured_data_is_one_graph_that_reads_back() {
    let html = pricing().to_html();
    let blocks = blocks(&html);
    assert_eq!(blocks.len(), 1);
    let graph = &blocks[0];
    assert_eq!(graph["@context"], "https://schema.org");
    let kinds: Vec<&str> = graph["@graph"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["@type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "Organization",
            "WebSite",
            "ItemPage",
            "SoftwareApplication",
            "BreadcrumbList"
        ]
    );
    let app = &graph["@graph"][3];
    assert_eq!(app["offers"][1]["price"], "299");
    assert_eq!(app["offers"][1]["priceCurrency"], "INR");
    assert_eq!(app["isAccessibleForFree"], true);
    assert_eq!(
        graph["@graph"][2]["isPartOf"]["@id"],
        "https://kesar.example/#website"
    );
}

#[test]
fn hostile_text_is_escaped_in_every_tag() {
    let nasty = r#""><script>alert(1)</script><!-- & </script>"#;
    let html = View::new(nasty)
        .seo(
            Seo::new(nasty)
                .canonical(format!("{SITE}/\"><x>"))
                .site_name(nasty)
                .locale(nasty)
                .image(SocialImage::new(
                    format!("{SITE}/a.png?\"<"),
                    1200,
                    630,
                    nasty,
                ))
                .twitter_site(nasty)
                .language(nasty, format!("{SITE}/hi/"))
                .json_ld(
                    JsonLd::new().add(FaqPage::new(format!("{SITE}/")).question(nasty, nasty)),
                ),
        )
        .to_html();
    let head = &html[..html.find("</head>").unwrap()];
    // The only script is the data block, and it is closed once.
    assert_eq!(head.matches("<script").count(), 1, "{head}");
    assert_eq!(head.matches("</script>").count(), 1, "{head}");
    assert!(!head.contains("<x>"), "{head}");
    assert!(!head.contains("<!--"), "{head}");
    assert!(!head.contains("\"><script>alert"), "{head}");
    let data = &blocks(&html)[0]["@graph"][0]["mainEntity"][0];
    assert_eq!(data["name"], nasty);
    assert_eq!(data["acceptedAnswer"]["text"], nasty);
}

#[test]
fn an_address_that_runs_something_is_left_out() {
    let html = View::new("x")
        .seo(
            Seo::new("x")
                .canonical("javascript:alert(1)")
                .image(SocialImage::new("data:image/png;base64,AA", 1, 1, "x"))
                .language("hi", "vbscript:x")
                .format("text/markdown", "javascript:x"),
        )
        .to_html();
    assert!(!html.contains("javascript:"), "{html}");
    assert!(!html.contains("vbscript:"), "{html}");
    assert!(!html.contains("data:image/png"), "{html}");
    assert!(!html.contains("rel=\"canonical\""), "{html}");
    assert!(!html.contains("og:image"), "{html}");
}

#[test]
fn a_page_kept_out_of_results_says_so_and_has_no_other_problem() {
    let page = View::new("Not found").seo(Seo::new("x").noindex());
    let html = page.to_html();
    assert_eq!(meta(&html, "robots").unwrap(), "noindex, follow");
    assert!(page.seo_problems().is_empty());
    assert_eq!(View::new("x").seo_problems().len(), 1);
}

#[test]
fn problems_name_what_a_search_engine_holds_against_a_page() {
    let page = View::new("Short").seo(
        Seo::new("Too short.")
            .canonical("/pricing/")
            .og_type(OgType::Article),
    );
    let problems = page.seo_problems();
    let has = |part: &str| problems.iter().any(|problem| problem.contains(part));
    assert!(has("the title is 5 characters"), "{problems:#?}");
    assert!(has("the description is 10 characters"), "{problems:#?}");
    assert!(has("not an absolute https URL"), "{problems:#?}");
    assert!(has("no picture"), "{problems:#?}");
    assert!(has("no site name"), "{problems:#?}");
    assert!(has("no structured data"), "{problems:#?}");
}

#[test]
fn the_policy_is_unchanged_and_a_page_with_only_data_loads_no_script() {
    let page = pricing();
    let html = page.to_html();
    assert!(page.uses().is_none());
    assert!(!html.contains("<script type=\"module\""), "{html}");
    // A data block is never run, so the policy needs no nonce, hash or `unsafe-inline`.
    assert!(!PAGE_CSP.contains("unsafe"));
    assert!(!PAGE_CSP.contains("nonce"));
}

#[test]
fn the_sitemap_and_robots_file_point_at_each_other() {
    let sitemap = Sitemap::new()
        .url(SitemapUrl::new(format!("{SITE}/")).lastmod("2026-11-12"))
        .url(
            SitemapUrl::new(format!("{SITE}/pricing/"))
                .lastmod(chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap())
                .language("hi-IN", format!("{SITE}/hi/pricing/")),
        );
    let xml = sitemap.to_xml();
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\""));
    assert!(xml.contains("<lastmod>2026-11-10</lastmod>"), "{xml}");
    assert_eq!(sitemap.locations().count(), 2);
    let robots = RobotsTxt::new()
        .welcome(AI_CRAWLERS)
        .sitemap(format!("{SITE}/sitemap.xml"))
        .to_text();
    assert!(robots.starts_with("User-agent: *\nAllow: /\n"), "{robots}");
    assert!(robots.contains("Sitemap: https://kesar.example/sitemap.xml"));
    for crawler in AI_CRAWLERS {
        assert!(
            robots.contains(&format!("User-agent: {crawler}\nAllow: /\n")),
            "{crawler}"
        );
    }
    assert!(
        !robots
            .lines()
            .any(|line| line.starts_with("Disallow: ") && line.len() > 10)
    );
}
