//! The reader on whole messages: structure, bodies, attachments, limits.

use super::*;

fn read(raw: &str) -> Message {
    parse(&Bytes::from(raw.to_owned()), Limits::default()).unwrap()
}

fn crlf(raw: &str) -> String {
    raw.replace('\n', "\r\n")
}

fn names(message: &Message) -> Vec<(Option<&str>, &str)> {
    message
        .attachments
        .iter()
        .map(|a| (a.filename.as_deref(), a.content_type.as_str()))
        .collect()
}

const ALTERNATIVE: &str = "From: a@b.test
Content-Type: multipart/alternative; boundary=\"alt\"

preamble
--alt
Content-Type: text/plain; charset=utf-8

Plain text.
--alt
Content-Type: text/html; charset=utf-8

<p>HTML text.</p>
--alt--
epilogue
";

#[test]
fn an_alternative_gives_the_text_and_the_html() {
    for raw in [ALTERNATIVE.to_owned(), crlf(ALTERNATIVE)] {
        let message = read(&raw);
        assert_eq!(message.text.as_deref(), Some("Plain text."));
        assert_eq!(message.html.as_deref(), Some("<p>HTML text.</p>"));
        assert!(message.attachments.is_empty());
        assert_eq!(message.headers[0].0, "From");
        assert_eq!(&message.headers[0].1[..], b" a@b.test");
    }
}

#[test]
fn a_single_part_message_is_its_body() {
    let message = read("Subject: x\n\nline one\nline two\n");
    assert_eq!(message.text.as_deref(), Some("line one\nline two\n"));
    assert_eq!(message.html, None);

    let message = read("Subject: x\nContent-Type: text/html\n\n<p>Only&nbsp;HTML</p>");
    assert_eq!(message.html.as_deref(), Some("<p>Only&nbsp;HTML</p>"));
    assert_eq!(message.text.as_deref(), Some("Only\u{a0}HTML\n"));

    // All header: no body at all; an empty body is an empty text.
    assert_eq!(read("Subject: x\n").text, None);
    assert_eq!(read("Subject: x\n\n").text.as_deref(), Some(""));
}

#[test]
fn transfer_encodings_and_charsets_are_decoded() {
    let message = read(
        "Content-Type: text/plain; charset=ISO-8859-1\nContent-Transfer-Encoding: quoted-printable\n\nSch=F6ne Gr=FC=DFe!",
    );
    assert_eq!(message.text.as_deref(), Some("Schöne Grüße!"));

    let message = read(
        "Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: BASE64\n\nR3LDvMOfZQ==\n",
    );
    assert_eq!(message.text.as_deref(), Some("Grüße"));
}

#[test]
fn mixed_messages_keep_inline_parts_and_files() {
    let message = read(
        "From: a@b.test
Content-Type: multipart/mixed; boundary=m

--m
Content-Type: text/plain

See attached.
--m
Content-Type: image/png
Content-Transfer-Encoding: base64
Content-Disposition: inline; filename=dot.png
Content-ID: <dot@x>

iVBORw0K
--m
Content-Type: text/csv; charset=windows-1252; name=\"Q3.csv\"
Content-Disposition: attachment

caf\u{e9}
--m
Content-Type: application/octet-stream
Content-Disposition: attachment; filename*=utf-8''r%C3%A9sum%C3%A9.bin

raw bytes
--m--
",
    );
    assert_eq!(message.text.as_deref(), Some("See attached."));
    assert_eq!(
        names(&message),
        vec![
            (Some("dot.png"), "image/png"),
            (Some("Q3.csv"), "text/csv"),
            (Some("résumé.bin"), "application/octet-stream")
        ]
    );
    let image = &message.attachments[0];
    assert!(image.inline);
    assert_eq!(image.content_id.as_deref(), Some("dot@x"));
    assert_eq!(&image.content[..4], b"\x89PNG");
    // Text attachments are decoded to UTF-8 (here from é as UTF-8 bytes
    // read as windows-1252).
    assert_eq!(message.attachments[1].content.as_ref(), "cafÃ©".as_bytes());
    assert!(!message.attachments[1].inline);
    assert_eq!(message.attachments[2].content.as_ref(), b"raw bytes");
}

#[test]
fn related_parts_after_the_first_are_attachments() {
    let message = read(
        "Content-Type: multipart/related; boundary=r

--r
Content-Type: text/html

<img src=cid:logo>
--r
Content-Type: image/gif
Content-ID: <logo>

GIF89a
--r--
",
    );
    assert_eq!(message.html.as_deref(), Some("<img src=cid:logo>"));
    assert_eq!(message.text.as_deref(), Some(""), "the HTML, as text");
    assert_eq!(names(&message), vec![(None, "image/gif")]);
}

#[test]
fn an_alternative_with_one_kind_serves_as_both() {
    let message = read(
        "Content-Type: multipart/alternative; boundary=a

--a
Content-Type: text/html

<b>bold</b>
--a--
",
    );
    assert_eq!(message.html.as_deref(), Some("<b>bold</b>"));
    assert_eq!(message.text.as_deref(), Some("bold"));
}

#[test]
fn forwarded_messages_keep_their_bytes() {
    let message = read(
        "Content-Type: multipart/mixed; boundary=f

--f
Content-Type: text/plain

FYI
--f
Content-Type: message/rfc822

Subject: inner
Content-Type: multipart/alternative; boundary=inner

--inner
Content-Type: text/plain

inner text
--inner--
--f
Content-Type: message/rfc822
Content-Transfer-Encoding: base64

U3ViamVjdDogZW5jb2RlZAoKYm9keQo=
--f--
",
    );
    assert_eq!(message.text.as_deref(), Some("FYI"));
    assert_eq!(
        names(&message),
        vec![(None, "message/rfc822"), (None, "message/rfc822")]
    );
    assert_eq!(
        message.attachments[0].content.as_ref(),
        &b"Subject: inner\nContent-Type: multipart/alternative; boundary=inner\n\n--inner\nContent-Type: text/plain\n\ninner text\n--inner--"[..]
    );
    assert_eq!(
        message.attachments[1].content.as_ref(),
        b"Subject: encoded\n\nbody\n"
    );

    // In a digest, a part without a type is a message.
    let digest =
        read("Content-Type: multipart/digest; boundary=d\n\n--d\n\nSubject: one\n\n1\n--d--\n");
    assert_eq!(names(&digest), vec![(None, "message/rfc822")]);
    assert_eq!(digest.text, None);
}

#[test]
fn delimiters_are_whole_lines() {
    let message = read(
        "Content-Type: multipart/mixed; boundary=b

--b
Content-Type: multipart/alternative; boundary=b-inner

--b-inner
Content-Type: text/plain

text with --b in the middle
--b-inner
Content-Type: text/html

<p>html</p>
--b-inner--
--b  \t
Content-Type: application/pdf

%PDF
--b--
",
    );
    assert_eq!(message.text.as_deref(), Some("text with --b in the middle"));
    assert_eq!(message.html.as_deref(), Some("<p>html</p>"));
    assert_eq!(names(&message), vec![(None, "application/pdf")]);
    assert_eq!(message.attachments[0].content.as_ref(), b"%PDF");
}

#[test]
fn broken_multiparts_are_read_as_far_as_they_go() {
    // No closing delimiter: the last part runs to the end.
    let message = read(
        "Content-Type: multipart/mixed; boundary=b\n\n--b\nContent-Type: text/plain\n\ncut short\n",
    );
    assert_eq!(message.text.as_deref(), Some("cut short\n"));

    // No delimiter at all: the body is a text attachment.
    let message = read("Content-Type: multipart/mixed; boundary=b\n\njust text\n");
    assert_eq!(message.text, None);
    assert_eq!(names(&message), vec![(None, "multipart/mixed")]);
    assert_eq!(message.attachments[0].content.as_ref(), b"just text\n");

    // No boundary: a part kept as it is.
    let message = read("Content-Type: multipart/mixed\n\n--b\n\nx\n--b--\n");
    assert_eq!(names(&message), vec![(None, "multipart/mixed")]);
    assert_eq!(
        message.attachments[0].content.as_ref(),
        b"--b\n\nx\n--b--\n"
    );

    // A part without a header, and a part without a blank line.
    let message = read(
        "Content-Type: multipart/mixed; boundary=b\n\n--b\n\nfirst\n--b\nContent-Type: text/plain\n--b--\n",
    );
    assert_eq!(message.text.as_deref(), Some("first"));
}

#[test]
fn without_a_header_there_is_no_message() {
    for raw in ["", "\n\nbody", "no colon\n\nbody", ": no name\n\n"] {
        assert_eq!(
            parse(&Bytes::from(raw), Limits::default()),
            Err(MimeError::NoHeaders),
            "{raw:?}"
        );
    }
}

/// Header fields hold memory each: past the limit the message is refused
/// before the next is kept (here at a tenth of the default, so the test
/// says nothing about the default but how it is applied).
#[test]
fn a_flood_of_header_fields_is_refused() {
    let limits = Limits {
        headers: 100,
        ..Limits::default()
    };
    let raw = "X: y\n".repeat(100) + "\nbody";
    assert!(parse(&Bytes::from(raw), limits).is_ok());
    let raw = "X: y\n".repeat(101) + "\nbody";
    assert_eq!(
        parse(&Bytes::from(raw), limits),
        Err(MimeError::TooManyHeaders(100))
    );

    let raw = "X: y\n".repeat(DEFAULT_MAX_HEADERS + 1) + "\nbody";
    assert_eq!(
        parse(&Bytes::from(raw), Limits::default()),
        Err(MimeError::TooManyHeaders(DEFAULT_MAX_HEADERS))
    );
}

/// A part costs a few bytes to write and a record to hold: a message of
/// empty parts is refused at the limit, while it is being split.
#[test]
fn a_flood_of_empty_parts_is_refused() {
    let flood = |parts: usize| {
        format!(
            "Content-Type: multipart/mixed; boundary=b\n\n{}--b--\n",
            "--b\n".repeat(parts)
        )
    };
    // The message itself and its multipart count as one part.
    let message = parse(
        &Bytes::from(flood(DEFAULT_MAX_PARTS - 1)),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(message.text.as_deref(), Some(""));
    assert_eq!(
        parse(&Bytes::from(flood(DEFAULT_MAX_PARTS)), Limits::default()),
        Err(MimeError::TooManyParts(DEFAULT_MAX_PARTS))
    );
    assert_eq!(
        parse(&Bytes::from(flood(200_000)), Limits::default()),
        Err(MimeError::TooManyParts(DEFAULT_MAX_PARTS))
    );
}

/// Each level of nesting is read by its own call: past the depth limit
/// the message is refused before the stack runs out.
#[test]
fn deeply_nested_multiparts_are_refused() {
    let nested = |levels: usize| {
        let mut raw = String::from("Subject: deep\n");
        for level in 0..levels {
            raw.push_str(&format!(
                "Content-Type: multipart/mixed; boundary=b{level}\n\n--b{level}\n"
            ));
        }
        raw.push_str("Content-Type: text/plain\n\ncore\n");
        raw
    };
    let message = parse(&Bytes::from(nested(DEFAULT_MAX_DEPTH)), Limits::default()).unwrap();
    assert_eq!(message.text.as_deref(), Some("core\n"));
    assert_eq!(
        parse(
            &Bytes::from(nested(DEFAULT_MAX_DEPTH + 1)),
            Limits::default()
        ),
        Err(MimeError::TooDeep(DEFAULT_MAX_DEPTH))
    );
    assert_eq!(
        parse(&Bytes::from(nested(20_000)), Limits::default()),
        Err(MimeError::TooDeep(DEFAULT_MAX_DEPTH))
    );
}

#[test]
fn ids_are_read_from_angle_brackets() {
    fn all(value: &str) -> Vec<&str> {
        ids(value).collect()
    }
    assert_eq!(all("<a@b>"), vec!["a@b"]);
    assert_eq!(all(" <a@b> (comment)\r\n <c@d>"), vec!["a@b", "c@d"]);
    assert_eq!(all("< spaced@b >"), vec!["spaced@b"]);
    assert_eq!(all("bare@id"), vec!["bare@id"]);
    assert_eq!(all("bare <unclosed"), vec!["bare"]);
    assert_eq!(all("<a> <unclosed"), vec!["a"]);
    assert_eq!(all("<>"), Vec::<&str>::new());
    assert_eq!(all(""), Vec::<&str>::new());
}

#[test]
fn header_values_decode() {
    assert_eq!(
        text(" =?utf-8?q?Gr=C3=BC=C3=9Fe?=\r\n  aus\tM\u{fc}nchen ".as_bytes()).as_deref(),
        Some("Grüße aus\tMünchen")
    );
    assert_eq!(text(b"  "), None);
    assert_eq!(
        addresses(b" Anu\r\n Lakhotia <anu@example.com>, b@c.test", 2),
        Ok(vec![
            Mailbox {
                name: Some("Anu Lakhotia".to_owned()),
                email: "anu@example.com".to_owned()
            },
            Mailbox {
                name: None,
                email: "b@c.test".to_owned()
            }
        ])
    );
    assert_eq!(
        addresses(b"a@b.test, c@d.test", 1),
        Err(MimeError::TooManyAddresses(1))
    );
    assert!(date(b" Tue, 27 Oct 2026\r\n 09:14:00 +0000").is_some());
}
