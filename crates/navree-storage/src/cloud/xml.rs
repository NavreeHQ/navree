//! The XML answers of S3, Azure and the services beside them, read with
//! quick-xml into a small tree of [`Element`]s within [`Limits`], and
//! [`escape`] for the XML a driver writes.
//!
//! [`parse`] reads what those services send: elements with text, in UTF-8.
//! Names are local names (a prefix and the namespace are dropped) and
//! attributes are not kept. It refuses what none of them sends and what
//! could make a small answer large: a document type declaration (so no
//! entity is defined or expanded), an entity that is not one of the five
//! XML has, an answer that is not UTF-8, and an answer that is longer,
//! deeper or has more elements than its limits.
//!
//! Text is kept exactly as sent, once its references are resolved: it is
//! not trimmed, because a key may begin or end with a space.
//!
//! ```
//! use navree_storage::cloud::xml;
//!
//! let listing = br#"<?xml version="1.0" encoding="UTF-8"?>
//! <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
//!   <Name>ludhiana-invoices</Name>
//!   <IsTruncated>true</IsTruncated>
//!   <NextContinuationToken>1ueGcxLPRx1Tr</NextContinuationToken>
//!   <Contents><Key>2026/INV 1101 &amp; 1102.pdf</Key><Size>100000</Size></Contents>
//!   <Contents><Key>2026/INV-1103.pdf</Key><Size>42</Size></Contents>
//! </ListBucketResult>"#;
//! let root = xml::parse(listing)?;
//! assert_eq!(root.name(), "ListBucketResult");
//! assert_eq!(root.value("NextContinuationToken"), Some("1ueGcxLPRx1Tr"));
//! let keys: Vec<&str> = root.all("Contents").filter_map(|file| file.value("Key")).collect();
//! assert_eq!(keys, ["2026/INV 1101 & 1102.pdf", "2026/INV-1103.pdf"]);
//! # Ok::<(), navree_storage::cloud::Error>(())
//! ```

use std::borrow::Cow;

use quick_xml::events::Event;

use super::{Error, ErrorKind, Result};

/// How large an XML answer may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The longest answer, in bytes (default 16 MiB).
    pub max_bytes: usize,
    /// The deepest nesting of elements (default 32).
    pub max_depth: usize,
    /// The most elements (default 400,000: a page of 5,000 blobs with
    /// every property Azure lists).
    pub max_elements: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 16 * 1024 * 1024,
            max_depth: 32,
            max_elements: 400_000,
        }
    }
}

/// An element of an XML answer: its local name, its own text and its child
/// elements, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    name: String,
    text: String,
    children: Vec<Element>,
}

impl Element {
    /// The local name: `Key` for `<Key>` and for `<s3:Key>`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The element's own text, with references (`&amp;`, `&#13;`) resolved
    /// and CDATA sections included, exactly as sent: not trimmed. The text
    /// of its children is theirs; around children it is the white space
    /// the answer was laid out with.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The child elements, in order.
    pub fn children(&self) -> &[Element] {
        &self.children
    }

    /// The first child named `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.name == name)
    }

    /// Every child named `name`, in order.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// The text of the first child named `name`.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.child(name).map(Element::text)
    }

    /// The element reached by going down `path`, one child name at a time:
    /// `["AssumeRoleWithWebIdentityResult", "Credentials"]`. The empty path
    /// is this element.
    pub fn find(&self, path: &[&str]) -> Option<&Element> {
        path.iter()
            .try_fold(self, |element, name| element.child(name))
    }
}

/// Reads `body`, the XML of an answer, within the default [`Limits`].
pub fn parse(body: &[u8]) -> Result<Element> {
    parse_within(body, Limits::default())
}

/// Reads `body`, the XML of an answer, within `limits`. What cannot be
/// read fails with [`ErrorKind::Answer`]; the failure says what is wrong
/// with the answer and repeats none of its text.
pub fn parse_within(body: &[u8], limits: Limits) -> Result<Element> {
    if body.len() > limits.max_bytes {
        return Err(unreadable("it is longer than its limit"));
    }
    let text = std::str::from_utf8(body).map_err(|_| unreadable("it is not UTF-8"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut events = quick_xml::Reader::from_str(text);
    {
        let config = events.config_mut();
        config.check_end_names = true;
        config.trim_text(false);
    }
    let mut tree = Tree {
        open: Vec::new(),
        root: None,
        elements: 0,
        limits,
    };
    loop {
        let event = events
            .read_event()
            .map_err(|_| unreadable("it is not well-formed"))?;
        match event {
            Event::Start(start) => tree.start(start.local_name().into_inner())?,
            Event::Empty(start) => {
                tree.start(start.local_name().into_inner())?;
                tree.end()?;
            }
            Event::End(_) => tree.end()?,
            Event::Text(text) => tree.text(&text),
            Event::CData(data) => tree.text(&data),
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(character)) => character,
                    Ok(None) => match &*reference {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "apos" => '\'',
                        "quot" => '"',
                        _ => return Err(unreadable("it uses an entity XML does not define")),
                    },
                    Err(_) => {
                        return Err(unreadable("it is not well-formed"));
                    }
                };
                tree.text(resolved.encode_utf8(&mut [0; 4]));
            }
            Event::DocType(_) => {
                return Err(unreadable("it has a document type declaration"));
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    if !tree.open.is_empty() {
        return Err(unreadable("it ends inside an element"));
    }
    tree.root.ok_or_else(|| unreadable("it has no element"))
}

/// The tree as it is read.
struct Tree {
    /// The elements that are open, the innermost last.
    open: Vec<Element>,
    /// The root, once it is closed.
    root: Option<Element>,
    /// How many elements were started.
    elements: usize,
    limits: Limits,
}

impl Tree {
    /// An element named `name` starts.
    fn start(&mut self, name: &str) -> Result<()> {
        if self.open.is_empty() && self.root.is_some() {
            return Err(unreadable("it has more than one root element"));
        }
        if self.open.len() >= self.limits.max_depth {
            return Err(unreadable("its elements are nested deeper than its limit"));
        }
        self.elements += 1;
        if self.elements > self.limits.max_elements {
            return Err(unreadable("it has more elements than its limit"));
        }
        self.open.push(Element {
            name: name.to_owned(),
            text: String::new(),
            children: Vec::new(),
        });
        Ok(())
    }

    /// The innermost open element ends.
    fn end(&mut self) -> Result<()> {
        let Some(closed) = self.open.pop() else {
            return Err(unreadable("it closes an element that is not open"));
        };
        match self.open.last_mut() {
            Some(parent) => parent.children.push(closed),
            None => self.root = Some(closed),
        }
        Ok(())
    }

    /// Text of the innermost open element; outside the root it is layout.
    fn text(&mut self, text: &str) {
        if let Some(element) = self.open.last_mut() {
            element.text.push_str(text);
        }
    }
}

/// An XML answer that cannot be read, and what is wrong with it.
fn unreadable(what: &'static str) -> Error {
    Error::new(
        ErrorKind::Answer,
        format!("the XML answer cannot be read: {what}"),
    )
}

/// `text` as it is written inside an element or an attribute of the XML a
/// driver sends: `<`, `>`, `&`, `'` and `"` as their entities.
///
/// ```
/// use navree_storage::cloud::xml::escape;
///
/// let key = "2026/INV 1101 & 1102 <final>.pdf";
/// assert_eq!(
///     format!("<Key>{}</Key>", escape(key)),
///     "<Key>2026/INV 1101 &amp; 1102 &lt;final&gt;.pdf</Key>"
/// );
/// ```
pub fn escape(text: &str) -> Cow<'_, str> {
    quick_xml::escape::escape(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(body: &str, limits: Limits) -> String {
        let error = parse_within(body.as_bytes(), limits).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Answer, "{body}");
        error.to_string()
    }

    #[test]
    fn an_answer_is_read_into_elements_with_their_text_as_sent() {
        let root = parse(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!-- a listing -->
<s3:ListBucketResult xmlns:s3="http://s3.amazonaws.com/doc/2006-03-01/">
  <s3:Contents>
    <s3:Key> a &amp; b&#13;&#x41;&lt;&gt;&apos;&quot; </s3:Key>
    <s3:Size>5</s3:Size>
    <s3:Owner><s3:ID>75aa</s3:ID></s3:Owner>
  </s3:Contents>
  <s3:Contents><s3:Key><![CDATA[c <d>]]> e</s3:Key><s3:Size/></s3:Contents>
  <s3:IsTruncated>false</s3:IsTruncated>
</s3:ListBucketResult>"#,
        )
        .unwrap();
        assert_eq!(root.name(), "ListBucketResult");
        assert_eq!(root.children().len(), 3);
        let files: Vec<&Element> = root.all("Contents").collect();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].value("Key"), Some(" a & b\rA<>'\" "));
        assert_eq!(files[0].value("Size"), Some("5"));
        assert_eq!(
            files[0].find(&["Owner", "ID"]).map(Element::text),
            Some("75aa")
        );
        assert_eq!(files[1].value("Key"), Some("c <d> e"));
        assert_eq!(files[1].value("Size"), Some(""));
        assert_eq!(root.value("IsTruncated"), Some("false"));
        assert_eq!(root.value("NextContinuationToken"), None);
        assert!(root.child("Missing").is_none());
        assert!(root.find(&["Contents", "Owner", "Missing"]).is_none());
        assert_eq!(root.find(&[]), Some(&root));
        // Around children, the text is the layout of the answer.
        assert!(root.text().trim().is_empty());
    }

    #[test]
    fn a_byte_order_mark_and_other_scripts_are_read() {
        let root = parse("\u{feff}<a><Key>चालान/ਲੁਧਿਆਣਾ ₹.pdf</Key></a>".as_bytes()).unwrap();
        assert_eq!(root.value("Key"), Some("चालान/ਲੁਧਿਆਣਾ ₹.pdf"));
    }

    #[test]
    fn what_no_provider_sends_is_refused() {
        let limits = Limits::default();
        let doctype = refusal(
            r#"<!DOCTYPE a [<!ENTITY big "aaaaaaaaaa">]><a>&big;&big;</a>"#,
            limits,
        );
        assert_eq!(
            doctype,
            "the XML answer cannot be read: it has a document type declaration"
        );
        assert!(refusal("<a>&big;</a>", limits).contains("entity"));
        assert!(refusal("<a><b></a></b>", limits).contains("well-formed"));
        assert!(refusal("<a><b>", limits).contains("ends inside"));
        assert!(refusal("<a/><b/>", limits).contains("more than one root"));
        assert!(refusal("", limits).contains("no element"));
        assert!(refusal("<!-- nothing -->", limits).contains("no element"));
        let error = parse(b"<a>\xff</a>").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Answer);
        assert!(error.to_string().contains("UTF-8"));
    }

    #[test]
    fn an_answer_past_its_limits_is_refused() {
        let limits = Limits {
            max_bytes: 64,
            max_depth: 3,
            max_elements: 5,
        };
        assert!(parse_within(b"<a><b><c>x</c></b></a>", limits).is_ok());
        assert!(refusal("<a><b><c><d/></c></b></a>", limits).contains("deeper"));
        assert!(parse_within(b"<a><b/><b/><b/><b/></a>", limits).is_ok());
        assert!(refusal("<a><b/><b/><b/><b/><b/></a>", limits).contains("more elements"));
        let long = format!("<a>{}</a>", "x".repeat(64));
        assert!(refusal(&long, limits).contains("longer"));
    }

    #[test]
    fn a_failure_repeats_nothing_of_the_answer() {
        for body in [
            "<Credentials><SecretAccessKey>s3cret</SecretAccessKey></Wrong>",
            "<Credentials><s3cret>val</other></Credentials>",
            "<Credentials>&#xs3cret;</Credentials>",
            "<Credentials>&#s3cret;</Credentials>",
        ] {
            let error = parse(body.as_bytes()).unwrap_err();
            assert!(std::error::Error::source(&error).is_none(), "{error:?}");
            let shown = format!("{error} {error:?}");
            assert!(!shown.contains("s3cret"), "{shown}");
        }
    }

    #[test]
    fn text_is_escaped_for_the_xml_a_driver_writes() {
        assert_eq!(escape("a<b>&'\"c"), "a&lt;b&gt;&amp;&apos;&quot;c");
        assert_eq!(escape("plain/key.txt"), "plain/key.txt");
        let root = parse(format!("<Key>{}</Key>", escape("a<b>&'\"c ")).as_bytes()).unwrap();
        assert_eq!(root.text(), "a<b>&'\"c ");
    }
}
