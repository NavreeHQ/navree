//! The carousel: its roles and labels, the ids that tie its parts together, the
//! markup that works with no script, and what a page with one loads.

use std::collections::HashSet;
use std::time::Duration;

use navree_ui::{
    Button, Carousel, CarouselHeight, CarouselLabels, CarouselSlide, Heading, Node, Text, Uses,
    View, assets, render,
};

fn offers(id: &str) -> Carousel {
    Carousel::new(id, "Offers from Ludhiana")
        .slide(CarouselSlide::new(Heading::h3("Phulkari dupatta")))
        .slide(CarouselSlide::new(Heading::h3("Basmati rice, 5 kg")))
        .slide(CarouselSlide::new(Heading::h3("Desi ghee, 1 kg")))
}

fn html(carousel: Carousel) -> String {
    render(&carousel.into())
}

/// Every value of the attribute `name` in `html`, in order.
fn values(html: &str, name: &str) -> Vec<String> {
    let needle = format!(" {name}=\"");
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        let end = rest.find('"').unwrap_or(rest.len());
        out.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    out
}

#[test]
fn a_carousel_is_a_labelled_region_of_slides() {
    let out = html(offers("c"));
    assert!(
        out.starts_with(r#"<section data-slot="carousel" id="c" role="region" aria-roledescription="carousel" aria-label="Offers from Ludhiana""#),
        "{out}"
    );
    assert_eq!(
        out.matches(r#"aria-roledescription="slide""#).count(),
        3,
        "{out}"
    );
    assert_eq!(
        values(&out, "aria-label")
            .into_iter()
            .filter(|label| label.contains(" of "))
            .collect::<Vec<_>>(),
        ["1 of 3", "2 of 3", "3 of 3"]
    );
    assert_eq!(
        out.matches(r#"role="group""#).count(),
        4,
        "three slides and the track: {out}"
    );
    assert!(out.contains("Phulkari dupatta") && out.contains("Desi ghee, 1 kg"));
}

#[test]
fn the_track_can_be_reached_and_scrolled_by_keyboard_and_names_its_slides() {
    let out = html(offers("c"));
    assert!(
        out.contains(r#"data-slot="carousel-track" id="c-track" role="group" aria-label="Slides" tabindex="0" aria-live="polite""#),
        "{out}"
    );
}

#[test]
fn previous_and_next_are_buttons_that_name_the_track_they_move() {
    let out = html(offers("c"));
    for (which, label) in [("prev", "Previous slide"), ("next", "Next slide")] {
        assert!(
            out.contains(&format!(r#"data-carousel="{which}""#))
                && out.contains(&format!(r#"aria-label="{label}""#)),
            "{which}: {out}"
        );
    }
    assert_eq!(values(&out, "aria-controls"), ["c-track", "c-track"]);
    assert!(!out.contains(r#"type="submit""#), "{out}");
    assert!(out.contains(r#"type="button""#), "{out}");
    // Before the track in the page, so a keyboard user meets them first.
    assert!(out.find("data-carousel=\"prev\"").unwrap() < out.find("carousel-track").unwrap());
    assert!(out.find("data-carousel=\"next\"").unwrap() < out.find("carousel-track").unwrap());
}

#[test]
fn the_words_can_be_set_for_another_language() {
    let punjabi = CarouselLabels {
        previous: "ਪਿਛਲੀ ਸਲਾਈਡ".into(),
        next: "ਅਗਲੀ ਸਲਾਈਡ".into(),
        slides: "ਸਲਾਈਡਾਂ".into(),
        slide_of: "{total} ਵਿੱਚੋਂ {n}".into(),
        ..CarouselLabels::default()
    };
    let out = html(offers("c").labels(punjabi));
    assert!(out.contains(r#"aria-label="ਪਿਛਲੀ ਸਲਾਈਡ""#), "{out}");
    assert!(out.contains(r#"aria-label="3 ਵਿੱਚੋਂ 2""#), "{out}");
    assert!(out.contains(r#"aria-label="ਸਲਾਈਡਾਂ""#), "{out}");
}

#[test]
fn pickers_are_tabs_that_are_links_to_their_slides() {
    let out = html(offers("c").pickers());
    assert!(
        out.contains(r#"data-slot="carousel-pickers" role="tablist" aria-label="Choose a slide""#),
        "{out}"
    );
    assert_eq!(out.matches(r#"role="tab""#).count(), 3, "{out}");
    for n in 1..=3 {
        assert!(
            out.contains(&format!(r##"id="c-tab-{n}" href="#c-slide-{n}""##)),
            "{n}: {out}"
        );
        assert!(
            out.contains(&format!(
                r##"href="#c-slide-{n}" aria-controls="c-slide-{n}""##
            )),
            "{n}: {out}"
        );
    }
    assert_eq!(values(&out, "aria-selected"), ["true", "false", "false"]);
    // With no script every tab can be reached; the module narrows it to one stop.
    assert!(!out.contains("tabindex=\"-1\""), "{out}");
    // The slides are the tabs' panels, named by them.
    assert_eq!(out.matches(r#"role="tabpanel""#).count(), 3, "{out}");
    assert!(out.contains(r#"aria-labelledby="c-tab-2""#), "{out}");
    assert!(!out.contains(r#"aria-label="1 of 3""#), "{out}");
}

#[test]
fn a_picker_is_named_by_its_slide_title_or_its_number() {
    let out = html(
        Carousel::new("c", "Offers")
            .slide(CarouselSlide::new("One").title("Phulkari dupatta"))
            .slide(CarouselSlide::new("Two"))
            .pickers(),
    );
    assert!(out.contains(r#"aria-label="Phulkari dupatta""#), "{out}");
    assert!(out.contains(r#"aria-label="Slide 2""#), "{out}");
}

#[test]
fn pickers_are_left_out_where_they_would_not_make_sense() {
    // Several slides in view at once: a tab for each would claim panels that are all showing.
    assert!(!html(offers("c").per_view(2).pickers()).contains("role=\"tab\""));
    // One slide has nothing to pick.
    let one = html(
        Carousel::new("c", "One")
            .slide(CarouselSlide::new("Only"))
            .pickers(),
    );
    assert!(!one.contains("role=\"tab\""), "{one}");
    assert!(one.contains(r#"aria-label="1 of 1""#), "{one}");
}

#[test]
fn ids_are_unique_and_tied_on_a_page_with_two_carousels() {
    let page = html_of((
        offers("a").pickers(),
        offers("b").autoplay(Duration::from_secs(5)),
    ));
    let ids = values(&page, "id");
    let unique: HashSet<&String> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len(), "no id twice: {ids:?}");
    // Everything a label, a control or a tab points at is on the page.
    for name in ["aria-controls", "aria-labelledby"] {
        for target in values(&page, name) {
            assert!(ids.contains(&target), "{name}={target} has no element");
        }
    }
    for href in values(&page, "href") {
        assert!(ids.contains(&href[1..].to_owned()), "{href} has no slide");
    }
}

fn html_of(nodes: impl navree_ui::IntoNodes) -> String {
    render(&navree_ui::Node::list(nodes))
}

#[test]
fn rotation_always_comes_with_a_pause_button_and_a_quiet_live_region() {
    let out = html(offers("c").autoplay(Duration::from_secs(5)));
    assert!(out.contains(r#"data-autoplay="5000""#), "{out}");
    assert!(out.contains(r#"data-carousel="rotation""#), "{out}");
    assert!(
        out.contains(r#"aria-label="Pause automatic rotation""#),
        "{out}"
    );
    assert!(
        out.contains(r#"data-pause="Pause automatic rotation""#),
        "{out}"
    );
    assert!(
        out.contains(r#"data-play="Start automatic rotation""#),
        "{out}"
    );
    assert!(
        out.contains(r#"aria-live="off""#),
        "a rotating carousel is not announced: {out}"
    );
    // The button is before the track, as the first thing a keyboard user meets.
    assert!(
        out.find("data-carousel=\"rotation\"").unwrap()
            < out.find("data-carousel=\"prev\"").unwrap()
    );
    let still = html(offers("c"));
    assert!(
        !still.contains("rotation") && !still.contains("data-autoplay"),
        "{still}"
    );
    assert!(still.contains(r#"aria-live="polite""#), "{still}");
}

#[test]
fn rotation_is_held_between_two_seconds_and_a_minute() {
    for (asked, held) in [
        (Duration::from_millis(100), 2_000),
        (Duration::from_secs(0), 2_000),
        (Duration::from_secs(2), 2_000),
        (Duration::from_secs(7), 7_000),
        (Duration::from_secs(60), 60_000),
        (Duration::from_secs(3_600), 60_000),
    ] {
        let out = html(offers("c").autoplay(asked));
        assert!(
            out.contains(&format!(r#"data-autoplay="{held}""#)),
            "{asked:?}: {out}"
        );
    }
}

#[test]
fn looping_orientation_and_slides_in_view_are_attributes() {
    let out = html(offers("c").loop_around());
    assert!(out.contains(" data-loop"), "{out}");
    assert!(!html(offers("c")).contains("data-loop"));
    let vertical = html(offers("c").vertical().height(CarouselHeight::Tall));
    assert!(
        vertical.contains(r#"data-orientation="vertical""#),
        "{vertical}"
    );
    assert!(vertical.contains(r#"data-height="tall""#), "{vertical}");
    // Chevrons that point along the track.
    assert!(vertical.contains("m18 15-6-6-6 6"), "{vertical}");
    assert!(!html(offers("c")).contains("data-orientation"));
    assert!(
        !html(offers("c").height(CarouselHeight::Tall)).contains("data-height"),
        "a height is for a vertical carousel"
    );
    for (asked, held) in [
        (0_u8, None),
        (1, None),
        (2, Some("2")),
        (4, Some("4")),
        (9, Some("4")),
    ] {
        let out = html(offers("c").per_view(asked));
        match held {
            Some(held) => assert!(
                out.contains(&format!(r#"data-per-view="{held}""#)),
                "{asked}: {out}"
            ),
            None => assert!(!out.contains("data-per-view"), "{asked}: {out}"),
        }
    }
}

#[test]
fn a_page_with_a_carousel_loads_its_script_and_styles_and_nothing_else() {
    let page = View::new("Offers").body(offers("c").pickers().autoplay(Duration::from_secs(5)));
    assert_eq!(page.uses(), Uses::CAROUSEL);
    let out = page.to_html();
    assert!(out.contains(assets::CAROUSEL_URL), "{out}");
    assert!(out.contains(assets::CAROUSEL_STYLES_URL), "{out}");
    for other in [
        assets::CORE_URL,
        assets::MENUS_URL,
        assets::OVERLAY_URL,
        assets::OTP_URL,
        assets::UPLOAD_URL,
        assets::MENUBAR_URL,
        assets::OTP_STYLES_URL,
        assets::MARKDOWN_URL,
    ] {
        assert!(!out.contains(other), "{other} in {out}");
    }
}

#[test]
fn a_page_without_one_loads_neither() {
    let page = View::new("Plain").body(Text::new("Nothing to slide."));
    assert!(!page.uses().has(Uses::CAROUSEL));
    let out = page.to_html();
    assert!(
        !out.contains(assets::CAROUSEL_URL) && !out.contains(assets::CAROUSEL_STYLES_URL),
        "{out}"
    );
}

#[test]
fn the_markup_needs_no_script_to_be_used() {
    let out = html(offers("c").pickers());
    assert!(!out.contains("<script"), "{out}");
    assert!(!out.contains("style="), "{out}");
    // The track scrolls by touch, wheel and keys on its own; the pickers are links.
    assert!(out.contains(r#"tabindex="0""#), "{out}");
    assert_eq!(out.matches("<a ").count(), 3, "{out}");
    // The module marks the carousel ready; until then nothing says it is.
    assert!(!out.contains("data-ready"), "{out}");
}

#[test]
fn hostile_text_is_escaped_in_every_place_it_can_go() {
    let hostile = r#""><script>alert(1)</script>"#;
    let labels = CarouselLabels {
        previous: hostile.into(),
        next: hostile.into(),
        pause: hostile.into(),
        play: hostile.into(),
        slides: hostile.into(),
        slide_of: format!("{hostile}{{n}}"),
        picker: hostile.into(),
        pickers: hostile.into(),
    };
    for pickers in [false, true] {
        let mut carousel = Carousel::new(hostile, hostile)
            .slide(CarouselSlide::new("One").title(hostile))
            .slide(CarouselSlide::new("Two"))
            .autoplay(Duration::from_secs(5))
            .labels(labels.clone());
        if pickers {
            carousel = carousel.pickers();
        }
        let out = html(carousel);
        assert!(!out.contains("<script"), "{out}");
        assert!(out.contains("&lt;script&gt;"), "{out}");
    }
}

#[test]
fn a_slide_keeps_whatever_it_is_given() {
    let out = html(
        Carousel::new("c", "Offers").slide(CarouselSlide::new(Node::list((
            Heading::h3("Pashmina shawl"),
            Button::outline("Order now").href("/orders/new"),
        )))),
    );
    assert!(
        out.contains("Pashmina shawl") && out.contains(r#"href="/orders/new""#),
        "{out}"
    );
}

#[test]
fn a_carousel_with_no_slides_draws_nothing() {
    assert_eq!(html(Carousel::new("c", "Empty")), "");
}
