# App blocks, a marketing site, SEO and the sign-in pages

The parts of a product past the list and the form: the app's own look, the
blocks of a dashboard, a public website drawn by the same app, what its
pages tell search engines, and the sign-in pages in the app's look. Each
block compiles, and the blocks nest: a later one uses the items of the ones
above it.

## App styles: the app's own stylesheet, fonts and pictures

A theme sets the colours, corners, density and text face. What a theme
can't say (a backdrop behind a headline, a typeface of the brand's, a rhythm
the components don't have) goes in app styles: `AppSheet` (a stylesheet),
`AppFont` (a typeface) and `AppImage` (the app's mark, a logo). Each is a
`static` compiled in with `include_bytes!`, with a ceiling on its size that
fails the build when passed. The toolkit serves each under its content hash
at `/_navree/ui/app/`, cached for good, so the page keeps its content
security policy of files from its own origin and nothing inline.

- Contribute each from one of the app's modules: `m.contribute(BRAND)`.
- Link them on every page with `Shell::stylesheet`, `Shell::font` and
  `Shell::logo`, on one page with `AppPage::stylesheet`, or on a page outside
  the shell with `View::stylesheet` and `View::font`.
- The app's sheet comes after the toolkit's, so its rule wins over one of
  equal weight. Keep to the toolkit's tokens (`var(--accent)`,
  `var(--ink-1)`) so it follows light and dark. A `class` on an element is
  read by this sheet only; the toolkit's own sheet reads `data-*`
  attributes.
- An SVG with a script, an event attribute or a `javascript:` address fails
  the build. Never write an `AppImage`'s bytes into a page.

A brand colour belongs in a theme of the app's own (`Theme::custom`), which
refuses a palette whose text falls short of WCAG AA.

```rust
use navree::ui::theme::{Appearance, Mode, Rgb, Theme, ThemeId};
use navree::ui::{AppFont, AppImage, AppSheet, NavGroup, NavItem, Shell, icons};

/// The app's stylesheet. A real app writes `include_bytes!("../assets/brand.css")`.
pub static BRAND: AppSheet = AppSheet::new(
    "brand",
    b".day-hero{background:linear-gradient(135deg,var(--accent),var(--ink-2))}",
    16 * 1024,
);

/// A display face for headings, from a woff2 file (`include_bytes!` in a real app).
pub static DISPLAY: AppFont = AppFont::new("display", "Fraunces", b"wOF2....", 200 * 1024).weights(400, 700);

/// The app's mark, shown in the shell's header.
pub static MARK: AppImage = AppImage::new(
    "mark",
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><circle cx="16" cy="16" r="14"/></svg>"#,
    8 * 1024,
);

/// The brand's teal as the accent in both palettes; the text on it and the focus ring
/// are derived.
pub fn brand_theme() -> Theme {
    let spec = Theme::NAVREE.spec().with_accent(Rgb::hex(0x2dd4bf), Rgb::hex(0x0f766e));
    Theme::custom(ThemeId::parse("sutlej-fit").expect("a theme id"), "Sutlej Fit", spec)
        .expect("the brand's colours pass WCAG AA")
}

/// The shell, with the app's look, its mark and how the browser's tab reads.
pub fn shell() -> Shell {
    Shell::new("Sutlej Fit")
        .logo(&MARK)
        .title_template("{} · Sutlej Fit")
        .appearance(Appearance::new(brand_theme(), Mode::System))
        .font(&DISPLAY)
        .stylesheet(&BRAND)
        .group(
            NavGroup::new("Today")
                .item(NavItem::new("Home", "/").icon(icons::HOUSE))
                .item(NavItem::new("Sleep", "/sleep").icon(icons::MOON))
                .item(NavItem::new("Heart", "/heart").icon(icons::HEART_PULSE)),
        )
}

#[module(register = Self::wire)]
pub struct BrandModule;

impl BrandModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.contribute(BRAND);
        m.contribute(DISPLAY);
        m.contribute(MARK);
        m.factory(Factory::<Shell>::new(|_| async { Ok(Arc::new(shell())) }))
            .exports::<Shell>();
    }
}
```

`Shell::title_template("{} · Sutlej Fit")` makes a page headed "Sleep" read
"Sleep · Sutlej Fit" in the tab. More in
[App styles](https://navree.com/docs/ui/app-styles/).

## The blocks of a dashboard

- **The figure tile** is a `Stat`: `.icon(..)` and `.tone(..)` for the chip
  before the label, `.unit(..)`, `.status(Badge)`, `.change(Trend)` with a
  `.caption(..)`, a `Sparkline` under it with `.chart(..)`, or a small `Ring`
  beside it with `.aside(..)`, and `.href(..)` to make the tile a link.
- **`Grid::fit(min)`** lays tiles out in as many columns as fit, each at
  least `min` rem wide (8 to 24), from one on a phone to six on a wide
  screen. **`Grid::ratio([7, 5])`** makes unequal columns, two to four, one
  column under 768 px.
- **The info tip** (`InfoTip`) is a "?" or (i) chip that opens a note: how to
  read a chart, what a score is made of. It opens by click, Enter or Space,
  and Esc closes it.
- **The chart skeleton** (`ChartSkeleton`) holds the shape of a chart that is
  loading: `ring`, `gauge`, `line`, `bars`, `heat_map`, `matrix`, `meter`.
  Put it inside a lazy `Region`, so the chart takes its place.
- **The breathing guide** (`BreathingGuide`) is a paced 4-7-8 breathing
  circle that runs from CSS alone and holds still for a reader who asks for
  less motion.
- **`Dock`** is a floating row of icon links at the bottom of the page, the
  current one lit. `Dock::below(Screen::Md)` shows it on phones only, when
  the shell holds the same places from that width up.
- **The day pager** (`DayPager`) moves between days, with the day's scores
  and a calendar behind the date. `AppPage::topbar(..)` puts it in the
  middle of the top bar, where it stays on phones.
- **The copy field** (`CopyField`) is a read-only value with a Copy button:
  an invite link, a key.
- `AppPage::document_title(..)` titles the tab by what the page is when its
  heading is a greeting.

```rust
use navree::ui::charts::{Ring, RingValue, Rings, Screen, Sparkline};
use navree::ui::{
    AppPage, AppShell, Badge, BreathingGuide, Card, ChartSkeleton, CopyField, DayPager, Dock,
    DockItem, Grid, Heading, InfoMark, InfoTip, Region, Row, Stack, Stat, Text, Tone, Trend,
};

/// The figure tiles of the day.
pub fn tiles() -> Grid {
    Grid::fit(14).gap(4).children((
        Stat::new("Resting heart rate", "58")
            .icon(icons::HEART_PULSE)
            .tone(Tone::Danger)
            .unit("bpm")
            .status(Badge::new("Athletic").tone(Tone::Success))
            .change(Trend::new(-3.3).up_is_bad())
            .caption("vs last week")
            .chart(Sparkline::new("Resting heart rate, 7 days", [61, 60, 62, 59, 58, 59, 58]).smooth().height(36)),
        Stat::new("Steps", "9,412")
            .icon(icons::FOOTPRINTS)
            .tone(Tone::named("orange"))
            .change(Trend::new(12.5))
            .caption("Goal 10,000")
            .aside(Ring::new("Steps", 9_412).max(10_000).tone(Tone::named("orange")).size(64))
            .href("/fitness"),
    ))
}

/// The day before, the day shown and the day after, with the day's scores.
pub fn pager(day: &str, previous: &str, next: &str) -> DayPager {
    DayPager::new("day", day.to_owned())
        .previous(format!("/?date={previous}"))
        .next(format!("/?date={next}"))
        .scores(
            Rings::new("Scores for the day")
                .ring(RingValue::new("Movement", 64))
                .ring(RingValue::new("Recovery", 72))
                .ring(RingValue::new("Sleep", 78))
                .size(28)
                .inline(),
        )
}

/// The app's main places on a phone; the shell holds them from 768 px up.
pub fn dock() -> Dock {
    Dock::new("Sections")
        .below(Screen::Md)
        .item(DockItem::new("Sleep", "/sleep").icon(icons::MOON))
        .item(DockItem::new("Home", "/").icon(icons::HOUSE).prominent().current())
        .item(DockItem::new("Fitness", "/fitness").icon(icons::ACTIVITY))
}

/// Today: a greeting as the heading, "Today" in the tab, the pager in the top bar.
pub fn today(shell: &AppShell) -> AppPage {
    shell
        .page("Good evening, Simran")
        .document_title("Today")
        .topbar(pager("Wed, 11 Nov", "2026-11-10", "2026-11-12"))
        .body(Stack::gap(5).children((
            tiles(),
            Grid::ratio([7, 5]).children((
                Card::titled("24-hour heart rate").body(
                    Region::new("heart-rate")
                        .src("/fragments/heart-rate")
                        .lazy()
                        .children(ChartSkeleton::line(160).label("Loading heart rate")),
                ),
                Card::titled("Relax").description("4-7-8 breathing").body(
                    BreathingGuide::new("relax", "4-7-8 breathing").size(200).tone(Tone::named("cyan")),
                ),
            )),
            Row::gap(2).children((
                Heading::h3("Training status"),
                InfoTip::new("training-tip", "Training status")
                    .mark(InfoMark::Info)
                    .title("How to read this")
                    .body(Text::new("Productive, maintaining or detraining, from your last four weeks.")),
            )),
            Card::titled("Family plan").body(
                CopyField::new("invite", "Invite link", "https://sutlej.example/join/8k2m-q4rt")
                    .help("Anyone with the link can ask to join your family plan."),
            ),
            dock(),
        )))
}
```

## A marketing site

The marketing blocks (`navree::ui::marketing`) draw a public website from
the same app as the product. Each block turns into a node and takes nodes,
so a hero can hold a live chart in a browser frame and a band can hold a
form. None needs a script: the phone menu and the questions are
`<details>`, the pricing switch is a pair of radio buttons, and the theme
toggle is a form.

| Block | What it is |
|---|---|
| `SitePage` | the skip link, the header, `<main>` with the sections, the footer |
| `SiteHeader` | the brand, the main links and the calls to action; a menu on a phone |
| `Hero` | the page's `h1`, a lead, calls to action, short notes and a picture; `centered()`, `backdrop(..)` |
| `Section` | a part of the page with its `SectionHeading`; its id is what `/#pricing` names; `narrow()`, `compact()` |
| `LogoCloud` | the names or marks of those who use the product |
| `FeatureGrid` | features in cards, two to four columns; a `wide()` feature with `media` makes a bento grid |
| `FeatureRows` | features told at length, each beside its picture, alternating sides |
| `StatBand` | a few large numbers, each with what it counts |
| Device frames (`BrowserFrame`, `PhoneFrame`) | a screenshot or the live product in a browser window or a phone, in its own `mode` |
| `Testimonials` | quotes in columns, with names, roles and stars |
| `Pricing` | plans side by side, with a monthly or yearly switch when a plan has both |
| `Faq` | questions that open to their answers; `faq_page(url)` is their structured data |
| `CtaBand` | the call to action near the end of a page |
| `Newsletter` | a band with a `FormView`, checked on the server like any form |
| `LegalPage` | a policy with its contents beside the text |
| `SiteFooter` | columns of links, accounts elsewhere, the small print and a `ThemeToggle` |
| `Picture` | a responsive image in AVIF, WebP and its own format, with its size, that never moves the page |

`accent(text)` sets the half of a headline that carries the accent colour.
Draw a public page with `Appearance::for_reader(&headers, query)`, so the
reader's choice from the footer's toggle holds.

```rust
use http::HeaderMap;
use navree::ui::marketing::{
    Brand, BrowserFrame, CtaBand, Faq, Feature, FeatureGrid, FeatureRow, FeatureRows, FooterColumn,
    Hero, LegalPage, LegalSection, LogoCloud, Newsletter, Plan, Pricing, Section, SectionHeading,
    SiteFooter, SiteHeader, SitePage, StatBand, Testimonial, Testimonials, ThemeToggle, accent,
};
use navree::ui::form::FormView;
use navree::ui::{Button, Form, Picture, Validate, View};
use serde::Deserialize;

/// The newsletter's form: one field, checked on the server.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct Subscribe {
    /// Email
    #[validate(email)]
    #[form(widget = email, placeholder = "neha@example.in")]
    pub email: String,
}

fn brand() -> Brand {
    Brand::new("Sutlej Fit", "/").icon(icons::SUN)
}

fn questions() -> Faq {
    Faq::new("faq")
        .exclusive()
        .item("Do I need a smartwatch?", "No. Sutlej Fit reads the steps, sleep and heart rate your phone already records.")
        .item("How much does it cost?", "Free for one person. Plus is ₹299 a month or ₹2,990 a year.")
}

/// The home page's blocks, top to bottom.
pub fn home_page(mode: Mode) -> SitePage {
    let screenshot = Picture::new("/img/today-1440.png", "Today in Sutlej Fit: a score of 82", 1440, 900)
        .webp([(1440, "/img/today-1440.webp")])
        .priority();
    SitePage::new()
        .header(
            SiteHeader::new(brand())
                .link("Features", "/#features")
                .link("Pricing", "/#pricing")
                .action(Button::ghost("Sign in").href("/sign-in"))
                .action(Button::primary("Start free").href("/sign-up")),
        )
        .section(
            Hero::new(("Know your body, ", accent("one calm score a day.")))
                .eyebrow("Health, read calmly")
                .lead("Sleep, heart and steps from the phone you already carry.")
                .action(Button::primary("Start free").href("/sign-up"))
                .note(icons::SHIELD_CHECK, "Data stored in India")
                .media(BrowserFrame::new(screenshot).address("app.sutlej.example/today")),
        )
        .section(
            Section::new("customers").compact().child(
                LogoCloud::new()
                    .title("Used by clinics and running clubs across North India")
                    .wordmark("Amritsar Runners")
                    .wordmark("Jaipur Physio")
                    .wordmark("Lucknow Yoga"),
            ),
        )
        .section(
            Section::new("features")
                .heading(SectionHeading::new("What it reads").eyebrow("Features").center())
                .child(
                    FeatureGrid::new()
                        .cols(3)
                        .feature(Feature::new("Sleep in stages", "Deep, REM and light sleep, night by night.").icon(icons::BED))
                        .feature(Feature::new("Resting heart", "Against your own two weeks.").icon(icons::HEART))
                        .feature(Feature::new("Movement", "A goal that follows how you slept.").icon(icons::FOOTPRINTS)),
                )
                .child(
                    FeatureRows::new().row(
                        FeatureRow::new("Your heart against your own baseline")
                            .eyebrow("Heart")
                            .text("Today's resting rate beside your last two weeks.")
                            .point("A band for what is usual for you")
                            .media(Card::new().body(Stat::new("Resting heart", "58 bpm"))),
                    ),
                ),
        )
        .section(
            Section::new("numbers").compact().child(
                StatBand::new()
                    .stat_with("1.2 lakh", "nights read last month", "Across 9,400 people")
                    .stat("0", "records sold or shared"),
            ),
        )
        .section(
            Section::new("stories").child(
                Testimonials::new().item(
                    Testimonial::new("Two weeks of earlier dinners and my resting heart rate came down by four.")
                        .author("Simran Kaur")
                        .role("Teacher, Ludhiana")
                        .initials("SK")
                        .rating(5),
                ),
            ),
        )
        .section(
            Section::new("pricing").heading(SectionHeading::new("Plans in rupees")).child(
                Pricing::new("plans")
                    .yearly_badge("2 months free")
                    .plan(Plan::new("Free").price("₹0", "for ever").feature("30 days of history").action(Button::outline("Start free").href("/sign-up")))
                    .plan(
                        Plan::new("Plus")
                            .monthly("₹299", "a month")
                            .yearly("₹2,990", "a year")
                            .feature("A year of trends")
                            .action(Button::primary("Start Plus").href("/sign-up?plan=plus"))
                            .featured("Most popular"),
                    )
                    .footnote("Prices in Indian rupees, including GST."),
            ),
        )
        .section(Section::new("faq").narrow().heading(SectionHeading::new("Questions")).child(questions()))
        .section(
            Newsletter::new("letter", "A calm letter, once a month")
                .form(FormView::<Subscribe>::new().post("/subscribe").submit("Subscribe"))
                .note("Leave with one click from any letter."),
        )
        .section(
            CtaBand::new("start", "Your data is already there. Start reading it.")
                .action(Button::primary("Start free").href("/sign-up"))
                .note("No card needed."),
        )
        .footer(footer(mode))
}

fn footer(mode: Mode) -> SiteFooter {
    SiteFooter::new(brand())
        .tagline("Your sleep, heart and steps, read calmly. Made in Jaipur.")
        .column(FooterColumn::new("Product").link("Features", "/#features").link("Pricing", "/#pricing"))
        .column(FooterColumn::new("Company").link("Privacy", "/privacy/"))
        .legal("© 2026 Sutlej Fit Pvt. Ltd., Jaipur.")
        .theme_toggle(ThemeToggle::new(mode).back("/"))
}

/// The privacy policy, with its contents beside the text.
pub fn privacy(mode: Mode) -> SitePage {
    SitePage::new()
        .header(SiteHeader::new(brand()).link("Pricing", "/#pricing"))
        .section(
            LegalPage::new("Privacy policy")
                .intro("What Sutlej Fit collects, why, where it is kept and how to take it back.")
                .updated("Last updated 10 November 2026")
                .section(LegalSection::new("collect", "1. What we collect").text("Steps, sleep and heart rate, with your permission."))
                .section(LegalSection::new("where", "2. Where it is kept").text("On servers in Mumbai, encrypted at rest and in transit.")),
        )
        .footer(footer(mode))
}
```

The whole set, with the light and dark rules and caching, is in
[Marketing sites](https://navree.com/docs/ui/marketing-sites/).

## Page SEO, the sitemap and robots.txt

`View::seo(Seo)` writes what a page tells search engines and link previews
in its head: the description, the canonical address, robots rules, Open
Graph and Twitter cards, other languages (`language(hreflang, url)`) and
structured data (`json_ld(JsonLd)` with `Organization`, `WebSite`,
`WebPage`, `SoftwareApplication`, `FaqPage` or a `Thing` of your own). The
JSON-LD block is a `<script type="application/ld+json">`, which the
browser never runs, so the page keeps its content security policy.
`View::seo_problems()` lists what a search engine would hold against the
page (a title or a description too short or too long, no canonical, an
`Organization` with no logo): assert it is empty for every public page.

`Sitemap` and `RobotsTxt` are the two files beside the pages; a route
returns each and it answers with its type. `RobotsTxt::disallow(path)` keeps
crawlers out of an account area, and `block(agent)` keeps one crawler out of
the whole site.

```rust
use navree::http::extract::RawQuery;
use navree::ui::seo::{
    JsonLd, Organization, RobotsTxt, Seo, Sitemap, SitemapUrl, SocialImage, WebSite,
};
use navree::ui::theme::Appearance;

const SITE: &str = "https://sutlej.example";

/// The home page: its blocks, its head and the reader's own light or dark.
pub fn home_view(headers: &HeaderMap, query: Option<&str>) -> View {
    let look = Appearance::new(brand_theme(), Mode::System).for_reader(headers, query);
    let seo = Seo::new("Sutlej Fit reads the health data your phone already keeps and turns it into one calm daily score.")
        .canonical(format!("{SITE}/"))
        .site_name("Sutlej Fit")
        .locale("en_IN")
        .image(SocialImage::new(format!("{SITE}/og.png"), 1200, 630, "The daily score: a ring at 82"))
        .json_ld(
            JsonLd::new()
                .node(
                    Organization::new("Sutlej Fit", format!("{SITE}/"))
                        .id(format!("{SITE}/#organization"))
                        .logo(format!("{SITE}/logo.png")),
                )
                .node(WebSite::new("Sutlej Fit", format!("{SITE}/")).publisher(format!("{SITE}/#organization")))
                .node(questions().faq_page(format!("{SITE}/"))),
        );
    View::new("Sutlej Fit: sleep, heart and steps in one daily score")
        .appearance(look.clone())
        .seo(seo)
        .body(home_page(look.mode()))
        .cache("private, no-cache")
}

#[injectable]
pub struct SitePages;

#[controller]
impl SitePages {
    #[get("!/privacy")]
    #[public]
    async fn privacy(&self) -> Result<View> {
        Ok(View::new("Privacy policy · Sutlej Fit")
            .seo(Seo::new("What Sutlej Fit collects, why, where it is kept in India and how to take it back.").canonical(format!("{SITE}/privacy/")))
            .body(privacy(Mode::System)))
    }

    #[get("!/sitemap.xml")]
    #[public]
    async fn sitemap(&self) -> Result<Sitemap> {
        Ok(Sitemap::new()
            .url(SitemapUrl::new(format!("{SITE}/")).lastmod("2026-11-12"))
            .url(SitemapUrl::new(format!("{SITE}/privacy/")).lastmod("2026-11-10")))
    }

    #[get("!/robots.txt")]
    #[public]
    async fn robots(&self) -> Result<RobotsTxt> {
        Ok(RobotsTxt::new()
            .disallow("/account/")
            .sitemap(format!("{SITE}/sitemap.xml")))
    }
}

#[test]
fn the_home_page_gives_search_engines_what_they_need() {
    let page = home_view(&HeaderMap::new(), None);
    assert!(page.seo_problems().is_empty(), "{:?}", page.seo_problems());
}
```

More in [SEO](https://navree.com/docs/ui/seo/).

## One address for guests and signed-in readers

A site's home is often the app's home too: the public page for a guest, the
reader's own day once they sign in. `RouteDef::on_guest_or(method, path,
guest, signed_in)` answers one address with two handlers, each with its own
extractors, so the signed-in one may take an extractor that refuses guests
and the guest one never runs it. The route is public.

**Caching.** The answer varies by `Cookie` and `Authorization`, and a
signed-in reader's answer goes out as `Cache-Control: private, no-store`
unless its handler kept it private itself. `Vary` alone is not enough:
several CDNs ignore `Vary: Cookie`. Set the CDN or proxy to skip its cache
for any request with the session cookie or an `Authorization` header, or
don't cache the guest's page at the edge.

```rust
use http::Method;
use navree::http::{Controller, CurrentPrincipal, RouteDef};

#[injectable]
pub struct HomeRoute;

impl Controller for HomeRoute {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on_guest_or(
            Method::GET,
            "!/",
            |headers: HeaderMap, RawQuery(query): RawQuery| async move { home_view(&headers, query.as_deref()) },
            |CurrentPrincipal(_reader): CurrentPrincipal, shell: AppShell| async move { today(&shell) },
        )
        .name("home")]
    }
}

#[module(controllers = [SitePages, HomeRoute])]
pub struct SiteModule;
```

## The sign-in pages in the app's look

`AuthPages` (navree-auth) draws the sign-in, two-factor, password and
sign-out pages; the routes are listed in `SKILL.md`. They are drawn in the
app's look: the registered `Shell`'s appearance, stylesheets and fonts, with
the reader's own theme and mode laid over them.

- `sign_up()` adds `GET` and `POST /sign-up` when `auth.registration` is on
  (`sign_up_path(..)` moves it), and the sign-in page links to it.
  `after_sign_up(path)` is where a new account lands with no `next`.
- `after_sign_out(path)` sends a browser to the public home page after
  signing out, in place of the sign-in page.
- `appearance(..)`, `stylesheet(..)` and `font(..)` set the look for an app
  with no shell, or a look of their own; `aside(..)` puts the app's panel
  beside the form on a wide screen; `theme_menu()` puts light and dark in
  the corner; `view(fn)` dresses each page last (its language, say).

The page around each form is the auth layout (`AuthLayout`): the form, and
the app's panel beside it on a wide screen (alone on a phone). Use it for a
guest page of your own, such as accepting an invitation.

```rust
use navree::auth::AuthPages;
use navree::ui::{AuthLayout, Field, Input};

/// Panel beside every sign-in form: what the app is, and an example day.
fn about_panel() -> Stack {
    Stack::gap(4).children((
        Heading::h2("Your health, clearly understood."),
        Text::new("Sleep, heart, recovery and movement, scored every day."),
        Rings::new("An example day: movement 64, recovery 82, sleep 88")
            .ring(RingValue::new("Movement", 64))
            .ring(RingValue::new("Recovery", 82))
            .ring(RingValue::new("Sleep", 88))
            .size(180)
            .legend(),
        Text::muted("An example day, not real data."),
    ))
}

/// Add it to the app root beside `SessionModule`, `AuthModule` and `UiModule`.
pub fn auth_pages() -> AuthPages {
    AuthPages::new()
        .app_name("Sutlej Fit")
        .sign_up()
        .after_sign_up("/welcome")
        .after_sign_out("/")
        .appearance(Appearance::new(brand_theme(), Mode::System))
        .stylesheet(&BRAND)
        .font(&DISPLAY)
        .aside(about_panel())
        .theme_menu()
}

/// A guest page of the app's own, in the same layout.
pub fn join_page(family: &str) -> View {
    View::new("Join a family plan").body(
        AuthLayout::new(Stack::gap(4).children((
            Heading::h1(format!("Join {family}")),
            Card::new().body(Stack::gap(4).children((
                Field::new("join-email", "Email").control(Input::email("email").autocomplete("email")),
                Button::primary("Ask to join"),
            ))),
        )))
        .aside(about_panel())
        .aside_label("About Sutlej Fit"),
    )
}
```
