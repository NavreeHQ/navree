//! Canonical XML of an element subtree: Exclusive XML Canonicalization 1.0
//! (with its `InclusiveNamespaces` prefix list) and Canonical XML 1.0, each
//! with or without comments.
//!
//! The output is the octet stream an XML signature digests or signs:
//! namespace declarations rendered only where the method says (sorted by
//! prefix, the default namespace first), attributes sorted by namespace URI
//! and local name, empty elements written as start and end tags, and the
//! method's escaping of text and attribute values. One node can be left
//! out: the `Signature` element of an enveloped signature.

use std::collections::{BTreeMap, BTreeSet};

use super::{Document, Element, Node, NodeId, XML_NS, apply_decls};

/// `http://www.w3.org/2001/10/xml-exc-c14n#`
pub(crate) const EXCLUSIVE: &str = "http://www.w3.org/2001/10/xml-exc-c14n#";
/// `http://www.w3.org/2001/10/xml-exc-c14n#WithComments`
pub(crate) const EXCLUSIVE_WITH_COMMENTS: &str =
    "http://www.w3.org/2001/10/xml-exc-c14n#WithComments";
/// `http://www.w3.org/TR/2001/REC-xml-c14n-20010315`
pub(crate) const INCLUSIVE: &str = "http://www.w3.org/TR/2001/REC-xml-c14n-20010315";
/// `http://www.w3.org/TR/2001/REC-xml-c14n-20010315#WithComments`
pub(crate) const INCLUSIVE_WITH_COMMENTS: &str =
    "http://www.w3.org/TR/2001/REC-xml-c14n-20010315#WithComments";

/// A canonicalization method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Method {
    /// Exclusive (`true`) or inclusive canonicalization.
    pub(crate) exclusive: bool,
    /// Whether comments are kept.
    pub(crate) comments: bool,
}

impl Method {
    /// Exclusive canonicalization without comments, what SAML uses.
    pub(crate) const EXCLUSIVE: Method = Method {
        exclusive: true,
        comments: false,
    };

    /// The method named by an algorithm URI.
    pub(crate) fn from_uri(uri: &str) -> Option<Method> {
        let (exclusive, comments) = match uri {
            EXCLUSIVE => (true, false),
            EXCLUSIVE_WITH_COMMENTS => (true, true),
            INCLUSIVE => (false, false),
            INCLUSIVE_WITH_COMMENTS => (false, true),
            _ => return None,
        };
        Some(Method {
            exclusive,
            comments,
        })
    }
}

/// The canonical form of the element `apex` and its subtree, without
/// `exclude` (and its subtree). `inclusive_prefixes` is the exclusive
/// method's `InclusiveNamespaces PrefixList` (`#default` for the default
/// namespace); the inclusive method ignores it.
pub(crate) fn canonicalize(
    doc: &Document,
    apex: NodeId,
    method: Method,
    inclusive_prefixes: &[String],
    exclude: Option<NodeId>,
) -> String {
    // The namespaces in scope at the apex's parent: the walk applies each
    // element's own declarations.
    let scope = match doc.parent(apex) {
        Some(parent) => doc.in_scope_namespaces(parent),
        None => BTreeMap::from([("xml".to_owned(), XML_NS.to_owned())]),
    };
    let inclusive: BTreeSet<String> = inclusive_prefixes
        .iter()
        .map(|p| {
            if p == "#default" {
                String::new()
            } else {
                p.clone()
            }
        })
        .collect();
    let mut out = String::new();
    let mut walker = Walker {
        doc,
        method,
        inclusive: &inclusive,
        exclude,
        apex,
        out: &mut out,
    };
    walker.node(apex, &scope, &BTreeMap::new());
    out
}

struct Walker<'a> {
    doc: &'a Document,
    method: Method,
    inclusive: &'a BTreeSet<String>,
    exclude: Option<NodeId>,
    apex: NodeId,
    out: &'a mut String,
}

impl Walker<'_> {
    /// Writes `id`; `parent_scope` is the namespaces in scope at its parent,
    /// `rendered` the declarations its output ancestors rendered.
    fn node(
        &mut self,
        id: NodeId,
        parent_scope: &BTreeMap<String, String>,
        rendered: &BTreeMap<String, String>,
    ) {
        if Some(id) == self.exclude {
            return;
        }
        match self.doc.node(id) {
            Node::Element(element) => self.element(id, element, parent_scope, rendered),
            Node::Text(text) => escape_text(text, self.out),
            Node::Comment(text) => {
                if self.method.comments {
                    self.out.push_str("<!--");
                    self.out.push_str(text);
                    self.out.push_str("-->");
                }
            }
            Node::Pi { target, data } => {
                self.out.push_str("<?");
                self.out.push_str(target);
                if !data.is_empty() {
                    self.out.push(' ');
                    self.out.push_str(data);
                }
                self.out.push_str("?>");
            }
        }
    }

    fn element(
        &mut self,
        id: NodeId,
        element: &Element,
        parent_scope: &BTreeMap<String, String>,
        rendered: &BTreeMap<String, String>,
    ) {
        let mut scope = parent_scope.clone();
        apply_decls(&mut scope, element);

        let candidates: BTreeSet<String> = if self.method.exclusive {
            let mut used = BTreeSet::new();
            used.insert(element.prefix.clone().unwrap_or_default());
            for attr in &element.attrs {
                if let Some(prefix) = &attr.prefix {
                    used.insert(prefix.clone());
                }
            }
            for prefix in self.inclusive {
                if scope.contains_key(prefix) {
                    used.insert(prefix.clone());
                }
            }
            used
        } else {
            scope.keys().cloned().collect()
        };

        let mut declare: BTreeMap<String, String> = BTreeMap::new();
        for prefix in candidates {
            if prefix == "xml" {
                continue;
            }
            let uri = scope.get(&prefix).cloned().unwrap_or_default();
            if prefix.is_empty() && uri.is_empty() {
                // `xmlns=""` only undoes a default namespace an output
                // ancestor rendered.
                if rendered.get("").is_some_and(|u| !u.is_empty()) {
                    declare.insert(String::new(), String::new());
                }
                continue;
            }
            if rendered.get(&prefix) != Some(&uri) {
                declare.insert(prefix, uri);
            }
        }

        let mut attrs: Vec<(String, String, String, String)> = element
            .attrs
            .iter()
            .map(|a| {
                (
                    a.ns.clone().unwrap_or_default(),
                    a.local.clone(),
                    a.qname(),
                    a.value.clone(),
                )
            })
            .collect();
        // Canonical XML 1.0: the apex inherits its ancestors' `xml:*`
        // attributes.
        if !self.method.exclusive && id == self.apex {
            let mut current = self.doc.parent(id);
            while let Some(ancestor) = current {
                if let Some(ancestor_element) = self.doc.element(ancestor) {
                    for attr in &ancestor_element.attrs {
                        if attr.ns.as_deref() == Some(XML_NS)
                            && !attrs.iter().any(|a| a.0 == XML_NS && a.1 == attr.local)
                        {
                            attrs.push((
                                XML_NS.to_owned(),
                                attr.local.clone(),
                                attr.qname(),
                                attr.value.clone(),
                            ));
                        }
                    }
                }
                current = self.doc.parent(ancestor);
            }
        }
        attrs.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));

        let name = element.qname();
        self.out.push('<');
        self.out.push_str(&name);
        for (prefix, uri) in &declare {
            if prefix.is_empty() {
                self.out.push_str(" xmlns=\"");
            } else {
                self.out.push_str(" xmlns:");
                self.out.push_str(prefix);
                self.out.push_str("=\"");
            }
            escape_attr(uri, self.out);
            self.out.push('"');
        }
        for (_, _, qname, value) in &attrs {
            self.out.push(' ');
            self.out.push_str(qname);
            self.out.push_str("=\"");
            escape_attr(value, self.out);
            self.out.push('"');
        }
        self.out.push('>');

        let mut child_rendered = rendered.clone();
        child_rendered.extend(declare);
        for child in &element.children {
            self.node(*child, &scope, &child_rendered);
        }
        self.out.push_str("</");
        self.out.push_str(&name);
        self.out.push('>');
    }
}

fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
}

fn escape_attr(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INCLUSIVE_METHOD: Method = Method {
        exclusive: false,
        comments: false,
    };

    fn exc(xml: &str) -> String {
        let doc = Document::parse(xml).unwrap();
        canonicalize(&doc, doc.root(), Method::EXCLUSIVE, &[], None)
    }

    /// The first element named `local` (any namespace).
    fn find(doc: &Document, local: &str) -> NodeId {
        doc.elements()
            .into_iter()
            .find(|id| doc.element(*id).unwrap().local == local)
            .unwrap()
    }

    #[test]
    fn empty_elements_attributes_and_escaping() {
        assert_eq!(
            exc("<a b=\"x\" a=\"y\"><e/>t&amp;&lt;&gt;\"'<f c='&quot;&#9;&#10;&#13;'/></a>"),
            "<a a=\"y\" b=\"x\"><e></e>t&amp;&lt;&gt;\"'<f c=\"&quot;&#x9;&#xA;&#xD;\"></f></a>"
        );
        assert_eq!(exc("<a>&#13;</a>"), "<a>&#xD;</a>");
    }

    #[test]
    fn comments_go_unless_asked_for_and_instructions_stay() {
        let xml = "<a><!--c--><?p d?></a>";
        assert_eq!(exc(xml), "<a><?p d?></a>");
        let doc = Document::parse(xml).unwrap();
        let with = Method::from_uri(EXCLUSIVE_WITH_COMMENTS).unwrap();
        assert_eq!(
            canonicalize(&doc, doc.root(), with, &[], None),
            "<a><!--c--><?p d?></a>"
        );
    }

    #[test]
    fn attributes_sort_by_namespace_then_local_name() {
        assert_eq!(
            exc(r#"<e xmlns:b="urn:b" xmlns:a="urn:a" b:x="1" a:y="2" z="3" a:x="4"/>"#),
            r#"<e xmlns:a="urn:a" xmlns:b="urn:b" z="3" a:x="4" a:y="2" b:x="1"></e>"#
        );
    }

    /// Exclusive canonicalization renders only the visibly used
    /// namespaces, where they are first used.
    #[test]
    fn exclusive_renders_visibly_used_namespaces_only() {
        let xml = r#"<n0:a xmlns:n0="urn:0" xmlns:n1="urn:1" xmlns="urn:d"><n1:b><c n0:x="1"/></n1:b></n0:a>"#;
        assert_eq!(
            exc(xml),
            r#"<n0:a xmlns:n0="urn:0"><n1:b xmlns:n1="urn:1"><c xmlns="urn:d" n0:x="1"></c></n1:b></n0:a>"#
        );
        // A subtree takes the namespaces it uses from its ancestors.
        let doc = Document::parse(xml).unwrap();
        let b = find(&doc, "b");
        assert_eq!(
            canonicalize(&doc, b, Method::EXCLUSIVE, &[], None),
            r#"<n1:b xmlns:n1="urn:1"><c xmlns="urn:d" xmlns:n0="urn:0" n0:x="1"></c></n1:b>"#
        );
        // The prefix list renders listed prefixes in scope, like the
        // inclusive method.
        assert_eq!(
            canonicalize(
                &doc,
                b,
                Method::EXCLUSIVE,
                &["n0".to_owned(), "#default".to_owned(), "zz".to_owned()],
                None
            ),
            r#"<n1:b xmlns="urn:d" xmlns:n0="urn:0" xmlns:n1="urn:1"><c n0:x="1"></c></n1:b>"#
        );
    }

    #[test]
    fn redeclarations_and_undeclared_defaults() {
        assert_eq!(
            exc(r#"<a xmlns="urn:a"><b xmlns="urn:a"><c xmlns=""/></b></a>"#),
            r#"<a xmlns="urn:a"><b><c xmlns=""></c></b></a>"#
        );
        // Nothing rendered a default namespace: no `xmlns=""`.
        assert_eq!(exc(r#"<a><b xmlns=""/></a>"#), "<a><b></b></a>");
        // The same prefix bound to another URI below is rendered again.
        assert_eq!(
            exc(r#"<p:a xmlns:p="urn:1"><p:b xmlns:p="urn:2"/></p:a>"#),
            r#"<p:a xmlns:p="urn:1"><p:b xmlns:p="urn:2"></p:b></p:a>"#
        );
    }

    #[test]
    fn inclusive_renders_every_namespace_in_scope_and_xml_attributes() {
        let doc = Document::parse(
            r#"<a xmlns:x="urn:x" xml:lang="en"><b xmlns:y="urn:y"><c/></b></a>"#,
        )
        .unwrap();
        let b = find(&doc, "b");
        assert_eq!(
            canonicalize(&doc, b, INCLUSIVE_METHOD, &[], None),
            r#"<b xmlns:x="urn:x" xmlns:y="urn:y" xml:lang="en"><c></c></b>"#
        );
        assert_eq!(
            canonicalize(&doc, b, Method::EXCLUSIVE, &[], None),
            "<b><c></c></b>"
        );
    }

    #[test]
    fn the_excluded_node_and_its_subtree_are_left_out() {
        let doc = Document::parse("<a> <s><t/></s> <b/></a>").unwrap();
        let s = find(&doc, "s");
        assert_eq!(
            canonicalize(&doc, doc.root(), Method::EXCLUSIVE, &[], Some(s)),
            "<a>  <b></b></a>"
        );
    }

    /// The example of the Exclusive XML Canonicalization recommendation,
    /// §2.2 (with the namespace prefix list empty).
    #[test]
    fn recommendation_example() {
        let doc = Document::parse(
            r#"<n0:local xmlns:n0="foo:bar" xmlns:n3="ftp://example.org"><n1:elem2 xmlns:n1="http://example.net" xml:lang="en"><n3:stuff xmlns:n3="ftp://example.org"/></n1:elem2></n0:local>"#,
        )
        .unwrap();
        let elem2 = find(&doc, "elem2");
        assert_eq!(
            canonicalize(&doc, elem2, Method::EXCLUSIVE, &[], None),
            r#"<n1:elem2 xmlns:n1="http://example.net" xml:lang="en"><n3:stuff xmlns:n3="ftp://example.org"></n3:stuff></n1:elem2>"#
        );
        assert_eq!(
            canonicalize(&doc, elem2, INCLUSIVE_METHOD, &[], None),
            r#"<n1:elem2 xmlns:n0="foo:bar" xmlns:n1="http://example.net" xmlns:n3="ftp://example.org" xml:lang="en"><n3:stuff></n3:stuff></n1:elem2>"#
        );
    }
}
