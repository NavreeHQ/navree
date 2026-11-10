//! InputOtp and Carousel.

use std::time::Duration;

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("otp_carousel.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("input-otp", "thumb", "A code field", otp_thumb)
            .start()
            .height(80),
        single!("input-otp", "default", "Six digits", otp_default)
            .start()
            .height(150),
        single!("input-otp", "groups", "In groups", otp_groups)
            .start()
            .height(110),
        single!("input-otp", "letters", "Letters and digits", otp_letters)
            .start()
            .height(110),
        single!("input-otp", "error", "With an error", otp_error)
            .start()
            .height(150),
        gallery!("input-otp", "states", "States", otp_states).medium(),
        single!("input-otp", "send", "Sent when whole", otp_send)
            .start()
            .height(110),
        single!("input-otp", "rtl", "Right to left", otp_rtl)
            .start()
            .height(110),
        single!("input-otp", "check", "Checking a post", otp_check)
            .start()
            .height(70),
        single!("carousel", "thumb", "A carousel", carousel_thumb)
            .start()
            .height(230),
        single!("carousel", "default", "Buttons", carousel_default)
            .start()
            .height(230),
        single!("carousel", "pickers", "Pickers", carousel_pickers)
            .start()
            .height(270),
        single!("carousel", "per-view", "Two in view", carousel_per_view)
            .start()
            .height(230),
        single!("carousel", "loop", "Going round", carousel_loop)
            .start()
            .height(230),
        single!("carousel", "rotation", "Rotating", carousel_rotation)
            .start()
            .height(270),
        single!("carousel", "vertical", "Vertical", carousel_vertical)
            .start()
            .height(340),
        single!("carousel", "rtl", "Right to left", carousel_rtl)
            .start()
            .height(270),
    ]
}

fn otp_thumb() -> Field {
    Field::new("otp-thumb", "Code").control(InputOtp::new("code", 6))
}

fn otp_default() -> Field {
    Field::new("otp-default", "Verification code")
        .help("Six digits, sent to +91 98765 43210.")
        .control(InputOtp::new("code", 6))
}

fn otp_groups() -> Field {
    Field::new("otp-groups", "Transaction PIN").control(InputOtp::new("pin", 6).group(3))
}

fn otp_letters() -> Field {
    Field::new("otp-letters", "Voucher")
        .control(InputOtp::new("voucher", 8).kind(OtpKind::Alphanumeric))
}

fn otp_error() -> Field {
    Field::new("otp-error", "Verification code")
        .error("That code has expired. Ask for a new one.")
        .control(InputOtp::new("code", 6).value("482915"))
}

fn otp_states() -> Vec<(&'static str, Field)> {
    vec![
        (
            "Filled",
            Field::new("otp-filled", "Code").control(InputOtp::new("a", 6).value("482915")),
        ),
        (
            "Disabled",
            Field::new("otp-disabled", "Code")
                .control(InputOtp::new("b", 6).value("482915").disabled(true)),
        ),
        (
            "Read only",
            Field::new("otp-readonly", "Code")
                .control(InputOtp::new("c", 6).value("482915").readonly()),
        ),
        (
            "Invalid",
            Field::new("otp-invalid", "Code").control(InputOtp::new("d", 6).invalid(true)),
        ),
    ]
}

fn otp_send() -> Element {
    // The form is sent as soon as the sixth digit is in. The button still works.
    el("form")
        .attr("method", "post")
        .attr("action", "/verify")
        .child(
            Field::new("otp-send", "Code from the bank")
                .control(InputOtp::new("code", 6).submit_when_complete()),
        )
}

fn otp_rtl() -> Element {
    // A code reads left to right in every language.
    el("div")
        .attr("dir", "rtl")
        .child(Field::new("otp-rtl", "تصدیقی کوڈ").control(InputOtp::new("code", 6)))
}

fn otp_check() -> Text {
    // The rules the field draws are the rules the server reads a posted code under.
    let rules = InputOtp::new("code", 6).rules();
    match rules.check(" 482 915 ") {
        Ok(code) => Text::new(format!("Code {code}")),
        Err(error) => Text::new(error.message()),
    }
}

fn carousel_thumb() -> Carousel {
    Carousel::new("c-thumb", "Offers from Ludhiana")
        .slide(CarouselSlide::new(
            Card::titled("Phulkari dupatta").description("Hand-embroidered in Patiala. ₹2,499."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Basmati rice, 5 kg").description("Aged for a year, from Karnal. ₹1,150."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Desi ghee, 1 kg").description("Churned in Amritsar. ₹750."),
        ))
}

fn carousel_default() -> Carousel {
    Carousel::new("c-default", "Offers from Ludhiana")
        .slide(CarouselSlide::new(
            Card::titled("Phulkari dupatta").description("Hand-embroidered in Patiala. ₹2,499."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Basmati rice, 5 kg").description("Aged for a year, from Karnal. ₹1,150."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Desi ghee, 1 kg").description("Churned in Amritsar. ₹750."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Pashmina shawl").description("Woven in Jammu. ₹12,500."),
        ))
}

fn carousel_pickers() -> Carousel {
    // A picker is named by its slide's title.
    Carousel::new("c-pickers", "Offers from Ludhiana")
        .slide(
            CarouselSlide::new(
                Card::titled("Phulkari dupatta")
                    .description("Hand-embroidered in Patiala. ₹2,499."),
            )
            .title("Phulkari dupatta"),
        )
        .slide(
            CarouselSlide::new(
                Card::titled("Basmati rice, 5 kg")
                    .description("Aged for a year, from Karnal. ₹1,150."),
            )
            .title("Basmati rice, 5 kg"),
        )
        .slide(
            CarouselSlide::new(
                Card::titled("Desi ghee, 1 kg").description("Churned in Amritsar. ₹750."),
            )
            .title("Desi ghee, 1 kg"),
        )
        .pickers()
}

fn carousel_per_view() -> Carousel {
    Carousel::new("c-two", "Shops")
        .per_view(2)
        .slide(CarouselSlide::new(
            Card::titled("Sutlej Supply Co.").description("Ludhiana"),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Doaba Traders").description("Jalandhar"),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Pind Pantry").description("Patiala"),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Haveli Home Goods").description("Amritsar"),
        ))
}

fn carousel_loop() -> Carousel {
    Carousel::new("c-loop", "Mandi days")
        .loop_around()
        .slide(CarouselSlide::new(
            Card::titled("Monday").description("Jalandhar mandi, from 6 am."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Wednesday").description("Amritsar mandi, from 6 am."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Friday").description("Ludhiana mandi, from 5 am."),
        ))
}

fn carousel_rotation() -> Carousel {
    // It turns every five seconds, and stops while the pointer or focus is on it, when
    // someone presses Pause, and for anyone who asks their device for less motion.
    Carousel::new("c-rotation", "Festival deals")
        .autoplay(Duration::from_secs(5))
        .pickers()
        .slide(CarouselSlide::new(
            Card::titled("Lohri").description("Sarson da saag, ready to cook."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Baisakhi").description("New wheat flour, 10 kg."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Diwali").description("Brass diyas, a set of twelve. ₹1,299."),
        ))
}

fn carousel_vertical() -> Carousel {
    Carousel::new("c-vertical", "News from the mandi")
        .vertical()
        .slide(CarouselSlide::new(
            Card::titled("Wheat").description("₹2,275 a quintal at the Khanna mandi."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Paddy").description("Arrivals open in Sangrur on 1 October."),
        ))
        .slide(CarouselSlide::new(
            Card::titled("Maize").description("₹2,090 a quintal in Jalandhar."),
        ))
}

fn carousel_rtl() -> Element {
    el("div").attr("dir", "rtl").child(
        Carousel::new("c-rtl", "لدھیانہ کی پیشکشیں")
            .pickers()
            .slide(CarouselSlide::new(
                Card::titled("پھلکاری دوپٹہ").description("پٹیالہ میں ہاتھ سے کڑھائی۔"),
            ))
            .slide(CarouselSlide::new(
                Card::titled("باسمتی چاول").description("کرنال سے، ایک سال پرانے۔"),
            ))
            .slide(CarouselSlide::new(
                Card::titled("دیسی گھی").description("امرتسر میں بنایا گیا۔"),
            )),
    )
}
