//! [`Newsletter`]: a band with a sign-up or contact form beside its words.

use super::container;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// A band with a form: a title (`h2`), a line, the form and a small note under it. The
/// form is a [`FormView`](crate::FormView), so its fields, its checks on the server, its
/// errors and its CSRF token are a form's like any other, and it posts with no script.
///
/// A sign-up of one field lays itself out in one line ([`inline`](Self::inline)): the
/// field's label is kept for assistive technology and the button sits beside the box. A
/// contact form of several fields keeps its labels and stacks beside the words.
///
/// ```
/// use navree_ui::marketing::Newsletter;
/// use navree_ui::{el, render};
///
/// // In an app: `FormView::<Subscribe>::new().post("/subscribe").submit("Subscribe")`,
/// // from a `#[derive(Form)]` struct with one `email` field.
/// let form = el("form").attr("method", "post").attr("action", "/subscribe");
/// let band = Newsletter::new("letter", "A calm letter, once a month")
///     .lead("What changed in Kesar, and one thing worth knowing about sleep.")
///     .form(form)
///     .note("No spam. Leave with one click.")
///     .inline();
/// let html = render(&band.into());
/// assert!(html.contains(r#"data-layout="inline""#));
/// assert!(html.contains(r#"action="/subscribe""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Newsletter {
    el: Element,
    id: String,
    title: Node,
    lead: Option<Node>,
    form: Option<Node>,
    note: Option<Node>,
    inline: bool,
}

impl Newsletter {
    /// A band whose `id` links name, titled `title`.
    pub fn new(id: impl Into<String>, title: impl IntoNodes) -> Self {
        Self {
            el: el("section").slot("newsletter").uses(Uses::MARKETING),
            id: id.into(),
            title: Node::list(title),
            lead: None,
            form: None,
            note: None,
            inline: false,
        }
    }

    /// The line under the title.
    pub fn lead(mut self, lead: impl Into<Node>) -> Self {
        self.lead = Some(lead.into());
        self
    }

    /// The form: a [`FormView`](crate::FormView), drawn with its values and errors when
    /// the server sent it back.
    pub fn form(mut self, form: impl Into<Node>) -> Self {
        self.form = Some(form.into());
        self
    }

    /// A small line under the form: how often, how to leave, what happens to the address.
    pub fn note(mut self, note: impl Into<Node>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Lays a form of one field out in one line, its button beside the box.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    fn into_node(self) -> Node {
        let title = format!("{}-title", self.id);
        self.el
            .id(self.id)
            .attr("aria-labelledby", title.clone())
            .attr(
                "data-layout",
                if self.inline { "inline" } else { "stacked" },
            )
            .child(
                container().child(
                    el("div").slot("newsletter-panel").children((
                        el("div").slot("newsletter-copy").children((
                            el("h2")
                                .slot("newsletter-title")
                                .id(title)
                                .child(self.title),
                            self.lead
                                .map(|lead| el("p").slot("newsletter-lead").child(lead)),
                        )),
                        el("div").slot("newsletter-form").children((
                            self.form,
                            self.note
                                .map(|note| el("p").slot("newsletter-note").child(note)),
                        )),
                    )),
                ),
            )
            .into()
    }
}

into_node!(Newsletter);
