//! The one-time code field: its markup, what a page with it loads, and what the server
//! makes of a posted code.

use navree_ui::{Field, InputOtp, OtpError, OtpKind, OtpRules, Uses, View, assets, render};

fn html(otp: InputOtp) -> String {
    render(&otp.into())
}

/// The text of the attribute `name` on the first `<input` of `html`.
fn attr_of(html: &str, name: &str) -> Option<String> {
    let start = html.find("<input")?;
    let tag = &html[start..html[start..].find('>').map(|end| start + end)?];
    let needle = format!(" {name}=\"");
    let at = tag.find(&needle)? + needle.len();
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
}

#[test]
fn a_code_field_is_one_text_input_that_a_phone_can_fill_in() {
    let out = html(InputOtp::new("code", 6));
    assert_eq!(out.matches("<input").count(), 1, "{out}");
    for wanted in [
        r#"data-slot="input-otp-field""#,
        r#"type="text""#,
        r#"name="code""#,
        r#"autocomplete="one-time-code""#,
        r#"inputmode="numeric""#,
        r#"maxlength="6""#,
        r#"minlength="6""#,
        r#"pattern="[0-9]{6}""#,
        r#"spellcheck="false""#,
        r#"autocapitalize="off""#,
        r#"data-ui-otp="digits""#,
    ] {
        assert!(out.contains(wanted), "{wanted} in {out}");
    }
    assert!(!out.contains("password"), "{out}");
}

#[test]
fn the_slots_are_drawn_by_the_wrapper_and_not_by_extra_inputs() {
    let out = html(InputOtp::new("code", 6));
    assert!(
        out.starts_with(r#"<span data-slot="input-otp" data-length="6">"#),
        "{out}"
    );
    assert!(out.ends_with("</span>"), "{out}");
    assert!(!out.contains("style="), "no inline style: {out}");
    assert!(!out.contains("<script"), "{out}");
}

#[test]
fn letters_and_digits_ask_for_the_text_keyboard_and_capitals() {
    let out = html(InputOtp::new("code", 8).kind(OtpKind::Alphanumeric));
    for wanted in [
        r#"inputmode="text""#,
        r#"autocapitalize="characters""#,
        r#"pattern="[0-9A-Za-z]{8}""#,
        r#"data-ui-otp="alnum""#,
        r#"maxlength="8""#,
    ] {
        assert!(out.contains(wanted), "{wanted} in {out}");
    }
}

#[test]
fn a_length_is_held_to_three_through_twelve_in_the_field_and_in_the_check() {
    for (asked, held) in [
        (0_u8, 3_u8),
        (1, 3),
        (2, 3),
        (3, 3),
        (12, 12),
        (13, 12),
        (255, 12),
    ] {
        let out = html(InputOtp::new("code", asked));
        assert!(out.contains(&format!(r#"maxlength="{held}""#)), "{out}");
        assert!(out.contains(&format!(r#"data-length="{held}""#)), "{out}");
        assert_eq!(OtpRules::new(asked, OtpKind::Digits).length(), held);
        assert_eq!(InputOtp::new("code", asked).rules().length(), held);
    }
}

#[test]
fn a_group_divides_the_slots_only_where_it_fits() {
    let grouped = html(InputOtp::new("code", 6).group(3));
    assert!(grouped.contains(r#"data-group="3""#), "{grouped}");
    for (length, group) in [(6_u8, 0_u8), (6, 1), (6, 5), (6, 6), (4, 4), (3, 3)] {
        let out = html(InputOtp::new("code", length).group(group));
        assert!(!out.contains("data-group"), "{length} in {group}: {out}");
    }
    for group in [2_u8, 3, 4] {
        let out = html(InputOtp::new("code", 8).group(group));
        assert!(out.contains(&format!(r#"data-group="{group}""#)), "{out}");
    }
}

#[test]
fn a_page_with_a_code_field_loads_its_script_and_styles_and_nothing_else() {
    let page = View::new("Sign in").body(InputOtp::new("code", 6));
    assert_eq!(page.uses(), Uses::OTP);
    let out = page.to_html();
    assert!(out.contains(assets::OTP_URL), "{out}");
    assert!(out.contains(assets::OTP_STYLES_URL), "{out}");
    for other in [
        assets::CORE_URL,
        assets::MENUS_URL,
        assets::OVERLAY_URL,
        assets::UPLOAD_URL,
        assets::CAROUSEL_URL,
        assets::MENUBAR_URL,
        assets::CAROUSEL_STYLES_URL,
        assets::MARKDOWN_URL,
    ] {
        assert!(!out.contains(other), "{other} in {out}");
    }
}

#[test]
fn a_page_without_one_loads_neither_the_script_nor_the_styles() {
    let page = View::new("Plain").body(Field::new("name", "Name"));
    assert!(!page.uses().has(Uses::OTP));
    let out = page.to_html();
    assert!(!out.contains(assets::OTP_URL), "{out}");
    assert!(!out.contains(assets::OTP_STYLES_URL), "{out}");
}

#[test]
fn a_field_gives_the_input_its_id_its_label_and_its_error() {
    let out = html_node(
        Field::new("otp", "Verification code")
            .help("Six digits, sent to +91 98765 43210.")
            .error("That code is not right.")
            .control(InputOtp::new("code", 6)),
    );
    assert!(
        out.contains(r#"<label data-slot="label" for="otp">"#),
        "{out}"
    );
    assert_eq!(attr_of(&out, "id").as_deref(), Some("otp"), "{out}");
    assert_eq!(
        attr_of(&out, "aria-invalid").as_deref(),
        Some("true"),
        "{out}"
    );
    assert_eq!(
        attr_of(&out, "aria-describedby").as_deref(),
        Some("otp-help otp-error"),
        "{out}"
    );
    // The wrapper takes none of those.
    assert!(
        out.contains(r#"<span data-slot="input-otp" data-length="6">"#),
        "{out}"
    );
}

fn html_node(node: impl Into<navree_ui::Node>) -> String {
    render(&node.into())
}

#[test]
fn states_are_flags_on_the_input() {
    let out = html(
        InputOtp::new("code", 6)
            .required()
            .readonly()
            .autofocus()
            .invalid(true)
            .label("Code from the SMS")
            .submit_when_complete(),
    );
    for wanted in [
        " required",
        " readonly",
        " autofocus",
        r#"aria-invalid="true""#,
        r#"aria-label="Code from the SMS""#,
        " data-ui-submit",
    ] {
        assert!(out.contains(wanted), "{wanted} in {out}");
    }
    let off = html(InputOtp::new("code", 6).disabled(true));
    assert!(off.contains(" disabled"), "{off}");
    let on = html(InputOtp::new("code", 6).disabled(false).invalid(false));
    assert!(
        !on.contains("disabled") && !on.contains("aria-invalid"),
        "{on}"
    );
    assert!(!on.contains("data-ui-submit"), "{on}");
}

#[test]
fn a_value_is_kept_only_as_far_as_the_code_could_hold_it() {
    assert_eq!(
        attr_of(&html(InputOtp::new("code", 6).value("482915")), "value").as_deref(),
        Some("482915")
    );
    // Too long: cut. Spaces, dashes and other signs: dropped.
    assert_eq!(
        attr_of(
            &html(InputOtp::new("code", 6).value("48 29-15 77")),
            "value"
        )
        .as_deref(),
        Some("482915")
    );
    // A kind that takes capitals writes them.
    assert_eq!(
        attr_of(
            &html(
                InputOtp::new("code", 6)
                    .kind(OtpKind::Alphanumeric)
                    .value("ab12cd")
            ),
            "value"
        )
        .as_deref(),
        Some("AB12CD")
    );
    // Nothing usable: no value attribute at all.
    assert_eq!(
        attr_of(&html(InputOtp::new("code", 6).value("")), "value"),
        None
    );
    assert_eq!(
        attr_of(&html(InputOtp::new("code", 6).value("--  ")), "value"),
        None
    );
}

#[test]
fn hostile_text_is_escaped_or_dropped() {
    let out = html(
        InputOtp::new("co\"de", 6)
            .label(r#""><script>alert(1)</script>"#)
            .value("12<script>alert(x)</script>"),
    );
    assert!(!out.contains("<script"), "{out}");
    assert!(out.contains("&lt;script&gt;"), "{out}");
    assert_eq!(attr_of(&out, "value").as_deref(), Some("12"), "{out}");
}

#[test]
fn the_root_attribute_and_id_go_to_the_input() {
    let out = html(
        InputOtp::new("code", 6)
            .id("sign-in-code")
            .attr("data-test", "x"),
    );
    assert_eq!(
        attr_of(&out, "id").as_deref(),
        Some("sign-in-code"),
        "{out}"
    );
    assert_eq!(attr_of(&out, "data-test").as_deref(), Some("x"), "{out}");
}

// ---- What the server makes of a posted code ----------------------------------------

fn digits(length: u8) -> OtpRules {
    OtpRules::new(length, OtpKind::Digits)
}

#[test]
fn a_posted_code_is_trimmed_and_read_without_its_separators() {
    for posted in [
        "482915",
        " 482915 ",
        "482 915",
        "482-915",
        "4 8 2 - 9 1 5",
        "\t482915\r\n",
        "  482 915\n",
    ] {
        assert_eq!(
            digits(6).check(posted).as_deref(),
            Ok("482915"),
            "{posted:?}"
        );
    }
}

#[test]
fn a_code_of_the_wrong_length_or_with_nothing_in_it_is_refused() {
    assert_eq!(digits(6).check(""), Err(OtpError::Empty));
    assert_eq!(digits(6).check("   "), Err(OtpError::Empty));
    assert_eq!(digits(6).check("- -"), Err(OtpError::Empty));
    assert_eq!(digits(6).check("48291"), Err(OtpError::WrongLength));
    assert_eq!(digits(6).check("4829155"), Err(OtpError::WrongLength));
    assert_eq!(digits(4).check("482915"), Err(OtpError::WrongLength));
}

#[test]
fn only_ascii_digits_are_digits() {
    for posted in [
        // Arabic-Indic, Extended Arabic-Indic, Devanagari and fullwidth digits.
        "\u{0661}\u{0662}\u{0663}\u{0664}\u{0665}\u{0666}",
        "\u{06f1}\u{06f2}\u{06f3}\u{06f4}\u{06f5}\u{06f6}",
        "\u{0967}\u{0968}\u{0969}\u{096a}\u{096b}\u{096c}",
        "\u{ff11}\u{ff12}\u{ff13}\u{ff14}\u{ff15}\u{ff16}",
        // A superscript, a circled digit and a roman numeral that look like digits.
        "12345\u{00b9}",
        "12345\u{2460}",
        "12345\u{2160}",
        // Invisible characters, which would make a code that looks right and is not.
        "123\u{200b}456",
        "123\u{200d}456",
        "\u{feff}123456",
        "123456\u{200e}",
        // Spaces that are not the ASCII one.
        "123\u{00a0}456",
        "123\u{3000}456",
        "123\u{2009}456",
        // Controls inside the code.
        "123\t456",
        "123\n456",
        "123456\0",
        "\x00123456",
        // Letters and signs.
        "12345a",
        "12345.",
        "+12345",
        "1e3456",
    ] {
        assert_eq!(
            digits(6).check(posted),
            Err(OtpError::BadCharacter),
            "{posted:?}"
        );
    }
}

#[test]
fn a_code_with_letters_reads_capitals_from_ascii_letters_only() {
    let rules = OtpRules::new(6, OtpKind::Alphanumeric);
    assert_eq!(rules.check("ab12cd").as_deref(), Ok("AB12CD"));
    assert_eq!(rules.check("AB 12-cd").as_deref(), Ok("AB12CD"));
    assert_eq!(rules.check("Ab12Cd").as_deref(), Ok("AB12CD"));
    for posted in [
        // The long s and the dotless i have capitals that are ASCII letters; the Kelvin
        // sign lower-cases to one. None of them is an ASCII letter.
        "\u{017f}b12cd1",
        "\u{0131}b12cd1",
        "\u{212a}b12cd1",
        // Cyrillic and Greek letters that look Latin.
        "\u{0430}b12cd1",
        "\u{03bf}b12cd1",
    ] {
        assert_eq!(
            rules.check(posted),
            Err(OtpError::BadCharacter),
            "{posted:?}"
        );
    }
}

#[test]
fn a_huge_post_is_not_walked() {
    assert_eq!(
        digits(6).check(&"1".repeat(10_000)),
        Err(OtpError::WrongLength)
    );
    assert_eq!(
        digits(6).check(&"\u{0661}".repeat(1_000_000)),
        Err(OtpError::WrongLength)
    );
    assert_eq!(
        digits(6).check(&" ".repeat(10_000)),
        Err(OtpError::WrongLength)
    );
    // A code with room around it is still a code.
    assert_eq!(
        digits(6)
            .check(&format!("{}482915{}", " ".repeat(20), " ".repeat(20)))
            .as_deref(),
        Ok("482915")
    );
}

#[test]
fn nothing_but_ascii_digits_of_the_right_length_ever_comes_out() {
    // Every character of the first three planes, at the end of a code that is one short.
    for code in 0..0x3_0000_u32 {
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        let posted = format!("12345{ch}");
        if let Ok(read) = digits(6).check(&posted) {
            assert!(
                read.len() == 6 && read.bytes().all(|byte| byte.is_ascii_digit()),
                "{ch:?} gave {read:?}"
            );
            assert!(ch.is_ascii_digit(), "{ch:?} was taken as a digit");
        }
        let posted = format!("12345{ch}");
        if let Ok(read) = OtpRules::new(6, OtpKind::Alphanumeric).check(&posted) {
            assert!(
                read.bytes()
                    .all(|byte| byte.is_ascii_digit() || byte.is_ascii_uppercase()),
                "{ch:?} gave {read:?}"
            );
        }
    }
}

#[test]
fn the_field_and_the_check_hold_the_same_rules() {
    let otp = InputOtp::new("code", 40).kind(OtpKind::Alphanumeric);
    let rules = otp.rules();
    assert_eq!((rules.length(), rules.kind()), (12, OtpKind::Alphanumeric));
    assert_eq!(rules.check("ABCDEFGHJKLM").as_deref(), Ok("ABCDEFGHJKLM"));
    assert_eq!(rules.check("ABCDEFGHJKL"), Err(OtpError::WrongLength));
}

#[test]
fn a_refusal_says_what_is_wrong_in_words() {
    for error in [
        OtpError::Empty,
        OtpError::WrongLength,
        OtpError::BadCharacter,
    ] {
        assert!(!error.message().is_empty());
        assert_eq!(error.to_string(), error.message());
    }
    assert_ne!(OtpError::Empty.message(), OtpError::WrongLength.message());
}
