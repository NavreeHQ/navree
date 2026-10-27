//! A small, strict XML tree for SAML messages and metadata.
//!
//! [`Document::parse`] reads UTF-8 XML 1.0 with quick-xml into a tree that
//! keeps what XML signatures need: namespace declarations as written,
//! resolved namespaces, attributes, text, comments and processing
//! instructions. It refuses what a SAML message never needs and an attacker
//! would like to use: a document type declaration (so no entity expansion
//! and no external entities), entities other than the five predefined ones,
//! unbound prefixes, duplicate attributes, more than one root, deep nesting
//! and huge node counts. Line ends are normalized first, and attribute
//! values as the XML specification says, so the canonical form
//! ([`c14n`]) matches what the signer saw.

pub(crate) mod c14n;

use std::collections::BTreeMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

/// The `xml` prefix's namespace.
pub(crate) const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Why a document was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum XmlError {
    /// The document is not well-formed XML.
    #[error("malformed XML: {0}")]
    Malformed(String),
    /// The document has a document type declaration (refused: entity
    /// expansion and external entities).
    #[error("XML with a document type declaration is refused")]
    Doctype,
    /// The document uses an entity other than the five predefined ones.
    #[error("undefined XML entity `{0}`")]
    UndefinedEntity(String),
    /// The document declares an encoding other than UTF-8.
    #[error("unsupported XML encoding `{0}` (UTF-8 only)")]
    Encoding(String),
    /// A prefix is used without a declaration.
    #[error("undeclared XML namespace prefix `{0}`")]
    UnboundPrefix(String),
    /// The document is larger, deeper or has more nodes than allowed.
    #[error("the XML document is too large ({0})")]
    TooLarge(&'static str),
}

/// Size limits for [`Document::parse`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// The longest input, in bytes.
    pub(crate) max_bytes: usize,
    /// The deepest element nesting.
    pub(crate) max_depth: usize,
    /// The most nodes (elements, text, comments, instructions).
    pub(crate) max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bytes: 2 * 1024 * 1024,
            max_depth: 64,
            max_nodes: 100_000,
        }
    }
}

/// A node's index in its [`Document`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct NodeId(usize);

/// An attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Attr {
    /// The prefix as written, if any.
    pub(crate) prefix: Option<String>,
    /// The local name.
    pub(crate) local: String,
    /// The namespace (only for prefixed attributes).
    pub(crate) ns: Option<String>,
    /// The normalized value.
    pub(crate) value: String,
}

impl Attr {
    /// `prefix:local`, or `local`.
    pub(crate) fn qname(&self) -> String {
        qname(self.prefix.as_deref(), &self.local)
    }
}

/// An element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Element {
    /// The prefix as written, if any.
    pub(crate) prefix: Option<String>,
    /// The local name.
    pub(crate) local: String,
    /// The resolved namespace, if any.
    pub(crate) ns: Option<String>,
    /// The namespace declarations written on this element, in order:
    /// `(None, uri)` is `xmlns="uri"` (`uri` may be empty: `xmlns=""`).
    pub(crate) ns_decls: Vec<(Option<String>, String)>,
    /// The other attributes, in order.
    pub(crate) attrs: Vec<Attr>,
    /// The children, in order.
    pub(crate) children: Vec<NodeId>,
}

impl Element {
    /// `prefix:local`, or `local`.
    pub(crate) fn qname(&self) -> String {
        qname(self.prefix.as_deref(), &self.local)
    }
}

fn qname(prefix: Option<&str>, local: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}:{local}"),
        None => local.to_owned(),
    }
}

/// A node of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    /// An element.
    Element(Element),
    /// Character data (text, references and CDATA sections, merged).
    Text(String),
    /// A comment.
    Comment(String),
    /// A processing instruction.
    Pi {
        /// The target.
        target: String,
        /// The data after the target, without the separating whitespace.
        data: String,
    },
}

/// A parsed document: its nodes and the root element.
#[derive(Clone, Debug)]
pub(crate) struct Document {
    nodes: Vec<Node>,
    parents: Vec<Option<NodeId>>,
    root: NodeId,
}

/// Replaces `\r\n` and lone `\r` with `\n` (XML 1.0 §2.11).
fn normalize_line_ends(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('\r') {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
}

fn malformed(error: impl std::fmt::Display) -> XmlError {
    XmlError::Malformed(error.to_string())
}

fn utf8(bytes: &[u8]) -> Result<&str, XmlError> {
    std::str::from_utf8(bytes).map_err(|_| XmlError::Malformed("invalid UTF-8".to_owned()))
}

/// Whether `c` may appear in an XML 1.0 document.
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

fn split_qname(name: &str) -> Result<(Option<&str>, &str), XmlError> {
    match name.split_once(':') {
        Some((prefix, local)) if !prefix.is_empty() && !local.is_empty() && !local.contains(':') => {
            Ok((Some(prefix), local))
        }
        Some(_) => Err(XmlError::Malformed(format!("invalid name `{name}`"))),
        None if name.is_empty() => Err(XmlError::Malformed("empty name".to_owned())),
        None => Ok((None, name)),
    }
}

/// The namespace scopes of the open elements.
struct Scopes(Vec<Vec<(Option<String>, String)>>);

impl Scopes {
    fn lookup(&self, prefix: Option<&str>) -> Option<&str> {
        if prefix == Some("xml") {
            return Some(XML_NS);
        }
        for scope in self.0.iter().rev() {
            for (declared, uri) in scope.iter().rev() {
                if declared.as_deref() == prefix {
                    return Some(uri.as_str());
                }
            }
        }
        None
    }
}

struct Builder {
    nodes: Vec<Node>,
    parents: Vec<Option<NodeId>>,
    stack: Vec<NodeId>,
    scopes: Scopes,
    root: Option<NodeId>,
    limits: Limits,
}

impl Builder {
    fn push(&mut self, node: Node) -> Result<NodeId, XmlError> {
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(XmlError::TooLarge("too many nodes"));
        }
        let id = NodeId(self.nodes.len());
        let parent = self.stack.last().copied();
        self.nodes.push(node);
        self.parents.push(parent);
        if let Some(parent) = parent
            && let Node::Element(element) = &mut self.nodes[parent.0]
        {
            element.children.push(id);
        }
        Ok(id)
    }

    /// Appends character data to the open element (merged with a preceding
    /// text node); outside the root only whitespace is allowed.
    fn text(&mut self, text: &str) -> Result<(), XmlError> {
        if let Some(c) = text.chars().find(|c| !is_xml_char(*c)) {
            return Err(XmlError::Malformed(format!(
                "invalid character U+{:04X}",
                u32::from(c)
            )));
        }
        let Some(&parent) = self.stack.last() else {
            if text.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
                return Ok(());
            }
            return Err(XmlError::Malformed(
                "text outside the root element".to_owned(),
            ));
        };
        if text.is_empty() {
            return Ok(());
        }
        let last = match &self.nodes[parent.0] {
            Node::Element(element) => element.children.last().copied(),
            _ => None,
        };
        if let Some(last) = last
            && let Node::Text(existing) = &mut self.nodes[last.0]
        {
            existing.push_str(text);
            return Ok(());
        }
        self.push(Node::Text(text.to_owned())).map(drop)
    }

    fn start(&mut self, start: &BytesStart<'_>) -> Result<NodeId, XmlError> {
        if self.stack.is_empty() && self.root.is_some() {
            return Err(XmlError::Malformed(
                "more than one root element".to_owned(),
            ));
        }
        if self.stack.len() >= self.limits.max_depth {
            return Err(XmlError::TooLarge("nested too deeply"));
        }
        let name = utf8(start.name().as_ref())?.to_owned();
        let mut decls: Vec<(Option<String>, String)> = Vec::new();
        let mut raw_attrs: Vec<(String, String)> = Vec::new();
        let mut attributes = start.attributes();
        attributes.with_checks(true);
        for attr in attributes {
            let attr = attr.map_err(malformed)?;
            let key = utf8(attr.key.as_ref())?.to_owned();
            let value = attr
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|error| match &error {
                    quick_xml::Error::Escape(quick_xml::escape::EscapeError::UnrecognizedEntity(
                        _,
                        entity,
                    )) => XmlError::UndefinedEntity(entity.clone()),
                    _ => malformed(error),
                })?
                .into_owned();
            if let Some(c) = value.chars().find(|c| !is_xml_char(*c)) {
                return Err(XmlError::Malformed(format!(
                    "invalid character U+{:04X}",
                    u32::from(c)
                )));
            }
            if key == "xmlns" {
                decls.push((None, value));
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                if value.is_empty() {
                    return Err(XmlError::Malformed(format!(
                        "the prefix `{prefix}` is declared empty"
                    )));
                }
                if prefix == "xmlns" || (prefix == "xml" && value != XML_NS) {
                    return Err(XmlError::Malformed(format!(
                        "the prefix `{prefix}` cannot be declared"
                    )));
                }
                decls.push((Some(prefix.to_owned()), value));
            } else {
                raw_attrs.push((key, value));
            }
        }
        self.scopes.0.push(decls.clone());
        let (prefix, local) = split_qname(&name)?;
        let ns = match self.scopes.lookup(prefix) {
            Some("") => None,
            Some(uri) => Some(uri.to_owned()),
            None if prefix.is_none() => None,
            None => return Err(XmlError::UnboundPrefix(prefix.unwrap_or_default().to_owned())),
        };
        let mut attrs: Vec<Attr> = Vec::with_capacity(raw_attrs.len());
        for (key, value) in raw_attrs {
            let (prefix, local) = split_qname(&key)?;
            let ns = match prefix {
                None => None,
                Some(prefix) => Some(
                    self.scopes
                        .lookup(Some(prefix))
                        .ok_or_else(|| XmlError::UnboundPrefix(prefix.to_owned()))?
                        .to_owned(),
                ),
            };
            if attrs.iter().any(|a| a.ns == ns && a.local == local) {
                return Err(XmlError::Malformed(format!("duplicate attribute `{key}`")));
            }
            attrs.push(Attr {
                prefix: prefix.map(str::to_owned),
                local: local.to_owned(),
                ns,
                value,
            });
        }
        let element = Element {
            prefix: prefix.map(str::to_owned),
            local: local.to_owned(),
            ns,
            ns_decls: decls,
            attrs,
            children: Vec::new(),
        };
        let id = self.push(Node::Element(element))?;
        if self.stack.is_empty() {
            self.root = Some(id);
        }
        self.stack.push(id);
        Ok(id)
    }

    fn end(&mut self) {
        self.stack.pop();
        self.scopes.0.pop();
    }
}

impl Document {
    /// Parses `text` with the default [`Limits`].
    pub(crate) fn parse(text: &str) -> Result<Document, XmlError> {
        Self::parse_with(text, Limits::default())
    }

    /// Parses `text` within `limits`.
    pub(crate) fn parse_with(text: &str, limits: Limits) -> Result<Document, XmlError> {
        if text.len() > limits.max_bytes {
            return Err(XmlError::TooLarge("too many bytes"));
        }
        let text = normalize_line_ends(text);
        let mut reader = Reader::from_str(&text);
        {
            let config = reader.config_mut();
            config.check_end_names = true;
            config.check_comments = true;
            config.expand_empty_elements = false;
            config.trim_text(false);
        }
        let mut builder = Builder {
            nodes: Vec::new(),
            parents: Vec::new(),
            stack: Vec::new(),
            scopes: Scopes(Vec::new()),
            root: None,
            limits,
        };
        loop {
            match reader.read_event().map_err(malformed)? {
                Event::Decl(decl) => {
                    if let Some(encoding) = decl.encoding() {
                        let encoding = encoding.map_err(malformed)?;
                        let encoding = utf8(&encoding)?.to_owned();
                        if !encoding.eq_ignore_ascii_case("utf-8") {
                            return Err(XmlError::Encoding(encoding));
                        }
                    }
                }
                Event::DocType(_) => return Err(XmlError::Doctype),
                Event::Start(start) => {
                    builder.start(&start)?;
                }
                Event::Empty(start) => {
                    builder.start(&start)?;
                    builder.end();
                }
                Event::End(_) => builder.end(),
                Event::Text(text) => {
                    let text = text.decode().map_err(malformed)?;
                    if text.contains('&') {
                        return Err(XmlError::Malformed("a bare `&`".to_owned()));
                    }
                    builder.text(&text)?;
                }
                Event::CData(data) => {
                    if builder.stack.is_empty() {
                        return Err(XmlError::Malformed(
                            "CDATA outside the root element".to_owned(),
                        ));
                    }
                    builder.text(&data.decode().map_err(malformed)?)?;
                }
                Event::GeneralRef(reference) => {
                    let c = match reference.resolve_char_ref().map_err(malformed)? {
                        Some(c) => c,
                        None => {
                            let name = reference.decode().map_err(malformed)?;
                            match name.as_ref() {
                                "lt" => '<',
                                "gt" => '>',
                                "amp" => '&',
                                "apos" => '\'',
                                "quot" => '"',
                                other => return Err(XmlError::UndefinedEntity(other.to_owned())),
                            }
                        }
                    };
                    builder.text(c.encode_utf8(&mut [0; 4]))?;
                }
                Event::Comment(comment) => {
                    if !builder.stack.is_empty() {
                        builder.push(Node::Comment(comment.decode().map_err(malformed)?.into_owned()))?;
                    }
                }
                Event::PI(pi) => {
                    if !builder.stack.is_empty() {
                        let target = utf8(pi.target())?.to_owned();
                        let data = utf8(pi.content())?.trim_start().to_owned();
                        builder.push(Node::Pi { target, data })?;
                    }
                }
                Event::Eof => break,
            }
        }
        if !builder.stack.is_empty() {
            return Err(XmlError::Malformed("unclosed element".to_owned()));
        }
        let root = builder
            .root
            .ok_or_else(|| XmlError::Malformed("no root element".to_owned()))?;
        Ok(Document {
            nodes: builder.nodes,
            parents: builder.parents,
            root,
        })
    }

    /// The root element.
    pub(crate) fn root(&self) -> NodeId {
        self.root
    }

    /// The node `id`.
    pub(crate) fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    /// The element `id`, if it is one.
    pub(crate) fn element(&self, id: NodeId) -> Option<&Element> {
        match self.node(id) {
            Node::Element(element) => Some(element),
            _ => None,
        }
    }

    /// The parent of `id` (`None` for the root).
    pub(crate) fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.parents[id.0]
    }

    /// Whether `id` is the element `{ns}local`.
    pub(crate) fn is(&self, id: NodeId, ns: &str, local: &str) -> bool {
        self.element(id)
            .is_some_and(|e| e.local == local && e.ns.as_deref() == Some(ns))
    }

    /// The child elements of `id`, in order.
    pub(crate) fn child_elements(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.element(id)
            .map(|e| e.children.as_slice())
            .unwrap_or_default()
            .iter()
            .copied()
            .filter(|child| self.element(*child).is_some())
    }

    /// The child elements of `id` named `{ns}local`.
    pub(crate) fn children_named<'a>(
        &'a self,
        id: NodeId,
        ns: &'a str,
        local: &'a str,
    ) -> impl Iterator<Item = NodeId> + 'a {
        self.child_elements(id)
            .filter(move |child| self.is(*child, ns, local))
    }

    /// The first child element of `id` named `{ns}local`.
    pub(crate) fn child(&self, id: NodeId, ns: &str, local: &str) -> Option<NodeId> {
        self.children_named(id, ns, local).next()
    }

    /// `id` and the elements below it, in document order.
    pub(crate) fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut pending = vec![id];
        while let Some(next) = pending.pop() {
            if let Some(element) = self.element(next) {
                out.push(next);
                pending.extend(element.children.iter().rev().copied());
            }
        }
        out
    }

    /// Every element of the document, in document order.
    pub(crate) fn elements(&self) -> Vec<NodeId> {
        self.descendants(self.root)
    }

    /// The value of the unprefixed attribute `name` of `id`.
    pub(crate) fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        self.element(id)?
            .attrs
            .iter()
            .find(|a| a.ns.is_none() && a.local == name)
            .map(|a| a.value.as_str())
    }

    /// The text of `id` and its descendants, concatenated.
    pub(crate) fn text(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        match self.node(id) {
            Node::Text(text) => out.push_str(text),
            Node::Element(element) => {
                for child in &element.children {
                    self.collect_text(*child, out);
                }
            }
            Node::Comment(_) | Node::Pi { .. } => {}
        }
    }

    /// The trimmed text of `id`, `None` when empty.
    pub(crate) fn trimmed_text(&self, id: NodeId) -> Option<String> {
        let text = self.text(id);
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    /// The namespaces in scope at `id`: prefix (`""` for the default
    /// namespace) to URI. An undeclared default namespace maps to `""`.
    pub(crate) fn in_scope_namespaces(&self, id: NodeId) -> BTreeMap<String, String> {
        let mut chain = vec![id];
        let mut current = id;
        while let Some(parent) = self.parent(current) {
            chain.push(parent);
            current = parent;
        }
        let mut scope = BTreeMap::new();
        scope.insert("xml".to_owned(), XML_NS.to_owned());
        for node in chain.into_iter().rev() {
            if let Some(element) = self.element(node) {
                apply_decls(&mut scope, element);
            }
        }
        scope
    }
}

/// Applies `element`'s declarations to `scope`.
pub(crate) fn apply_decls(scope: &mut BTreeMap<String, String>, element: &Element) {
    for (prefix, uri) in &element.ns_decls {
        scope.insert(prefix.clone().unwrap_or_default(), uri.clone());
    }
}

/// Escapes text for an XML document we write (element content and
/// double-quoted attribute values).
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\r' => out.push_str("&#xD;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(doc: &Document, ids: &[NodeId]) -> Vec<String> {
        ids.iter()
            .map(|id| doc.element(*id).unwrap().qname())
            .collect()
    }

    #[test]
    fn elements_attributes_and_namespaces_are_resolved() {
        let doc = Document::parse(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<p:a xmlns:p="urn:p" xmlns="urn:d" x="1" p:y="2"><b><c xmlns=""/></b></p:a>"#,
        )
        .unwrap();
        let root = doc.root();
        let a = doc.element(root).unwrap();
        assert_eq!(a.ns.as_deref(), Some("urn:p"));
        assert_eq!(a.ns_decls.len(), 2);
        assert_eq!(doc.attr(root, "x"), Some("1"));
        assert_eq!(a.attrs[1].ns.as_deref(), Some("urn:p"));
        let b = doc.child_elements(root).next().unwrap();
        assert!(doc.is(b, "urn:d", "b"));
        let c = doc.child_elements(b).next().unwrap();
        assert_eq!(doc.element(c).unwrap().ns, None);
        assert_eq!(names(&doc, &doc.elements()), ["p:a", "b", "c"]);
        let scope = doc.in_scope_namespaces(c);
        assert_eq!(scope.get(""), Some(&String::new()));
        assert_eq!(scope.get("p").map(String::as_str), Some("urn:p"));
    }

    #[test]
    fn text_references_and_cdata_merge_into_one_text_node() {
        let doc =
            Document::parse("<a>x &amp; &lt;y&gt; &#65;&#x42; <![CDATA[<z>&]]> \r\nend</a>").unwrap();
        let root = doc.root();
        assert_eq!(doc.element(root).unwrap().children.len(), 1);
        assert_eq!(doc.text(root), "x & <y> AB <z>& \nend");
    }

    #[test]
    fn attribute_values_are_normalized() {
        let doc = Document::parse("<a v=\"a\tb\nc\r\nd&#9;e&#10;f&amp;&quot;\"/>").unwrap();
        assert_eq!(doc.attr(doc.root(), "v"), Some("a b c d\te\nf&\""));
    }

    #[test]
    fn dangerous_or_broken_documents_are_refused() {
        let cases: &[(&str, XmlError)] = &[
            (
                "<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>",
                XmlError::Doctype,
            ),
            ("<a>&x;</a>", XmlError::UndefinedEntity("x".to_owned())),
            ("<a v=\"&x;\"/>", XmlError::UndefinedEntity("x".to_owned())),
            ("<p:a/>", XmlError::UnboundPrefix("p".to_owned())),
            (
                "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a/>",
                XmlError::Encoding("ISO-8859-1".to_owned()),
            ),
        ];
        for (xml, expected) in cases {
            assert_eq!(Document::parse(xml).unwrap_err(), *expected, "{xml}");
        }
        for xml in [
            "<a><b></a>",
            "<a/><b/>",
            "<a></a>text",
            "<a x=\"1\" x=\"2\"/>",
            "<a xmlns:p=\"urn:1\" xmlns:q=\"urn:1\" p:x=\"1\" q:x=\"2\"/>",
            "<a xmlns:p=\"\"/>",
            "",
            "<a>\u{1}</a>",
        ] {
            assert!(
                matches!(Document::parse(xml), Err(XmlError::Malformed(_))),
                "{xml:?}"
            );
        }
    }

    #[test]
    fn limits_are_enforced() {
        let deep = format!("{}{}", "<a>".repeat(10), "</a>".repeat(10));
        let limits = Limits {
            max_depth: 5,
            ..Limits::default()
        };
        assert_eq!(
            Document::parse_with(&deep, limits).unwrap_err(),
            XmlError::TooLarge("nested too deeply")
        );
        let wide = format!("<a>{}</a>", "<b/>".repeat(20));
        let limits = Limits {
            max_nodes: 10,
            ..Limits::default()
        };
        assert_eq!(
            Document::parse_with(&wide, limits).unwrap_err(),
            XmlError::TooLarge("too many nodes")
        );
        let limits = Limits {
            max_bytes: 4,
            ..Limits::default()
        };
        assert_eq!(
            Document::parse_with("<abc/>", limits).unwrap_err(),
            XmlError::TooLarge("too many bytes")
        );
    }

    #[test]
    fn comments_and_instructions_inside_the_root_are_kept() {
        let doc = Document::parse("<!-- out --><a><!-- in --><?pi  data ?></a>").unwrap();
        let children = &doc.element(doc.root()).unwrap().children;
        assert_eq!(doc.node(children[0]), &Node::Comment(" in ".to_owned()));
        assert_eq!(
            doc.node(children[1]),
            &Node::Pi {
                target: "pi".to_owned(),
                data: "data ".to_owned()
            }
        );
    }
}
