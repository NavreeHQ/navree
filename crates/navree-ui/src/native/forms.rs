//! A form a page draws, said to a native client.
//!
//! `#[derive(Form)]` gives a form its id, its fields and their rules, and a
//! [`FormView`] fills it for a request: where it posts, what it holds, what is
//! wrong with a field, the options a choice offers. [`form`] says that same
//! form to a native client, so a screen written for native clients repeats
//! none of it by hand: the form's id, each field's name, its label, its help,
//! its control and its value are the ones the page's form has, and the
//! handler that takes the page's form takes this one.

use navree_native::node::{self as n, Node as Part};

use super::lower::{Cx, id};
use crate::form::{Form, FormView};

/// The form `view` draws, as a native client's form: its id, where it posts,
/// what it sends unseen, and its fields in their order, each with the label,
/// the help, the control, the value and the message the page's field has.
///
/// The form's own buttons are left out: a screen puts what sends the form
/// where its design has it, a [`Button`](n::Button) inside the form that
/// [`submits`](n::Button::submits) it or one elsewhere that
/// [`submits_form`](n::Button::submits_form) by the form's id. The rows and
/// columns the page lays its fields out in are left out too: each system's
/// form lays out its own.
///
/// A control no native part says (a field drawn by markup of the app's own)
/// stands as an empty `web` part, and [`designed`](super::designed) names it.
///
/// ```
/// # // In navree-ui's own examples the derive names the crate `crate`: these are its paths.
/// # use navree_ui::{InputKind, form};
/// use navree_ui::form::FormView;
/// use navree_ui::native::{self, ui::*};
/// use navree_ui::{Form, Validate};
/// use serde::Deserialize;
///
/// /// What a customer is asked.
/// #[derive(Deserialize, Validate, Form)]
/// #[form(id = "customer")]
/// struct Customer {
///     /// Name
///     #[validate(length(min = 1))]
///     name: String,
///     /// Email
///     #[validate(email)]
///     #[form(widget = email, help = "Where the receipt goes.")]
///     email: String,
/// }
///
/// # fn main() {
/// let asked = FormView::<Customer>::new().post("/customers").of(&Customer {
///     name: "Simran Kaur".to_owned(),
///     email: String::new(),
/// });
/// // The page draws `asked`; a native screen says the same form.
/// let sheet = Sheet::new("new-customer")
///     .title("New customer")
///     .task()
///     .parts(native::form(&asked))
///     .action(Button::new("Save").role(Role::Confirm).submits_form("customer"));
/// let Node::Form(form) = &sheet.children[0] else { panic!("the sheet holds the form") };
/// assert_eq!((form.id.as_deref(), form.action.as_str()), (Some("customer"), "/customers"));
/// let [Node::Field(name), Node::Field(email)] = form.children.as_slice() else {
///     panic!("the form holds its two fields");
/// };
/// assert_eq!((name.name.as_str(), name.label.as_deref()), ("name", Some("Name")));
/// assert!(matches!(&name.input, Input::Text(text) if text.value == "Simran Kaur"));
/// assert!(matches!(&email.input, Input::Text(text) if text.kind == TextKind::Email));
/// assert_eq!(email.help.as_deref(), Some("Where the receipt goes."));
/// # }
/// ```
pub fn form<T: Form>(view: &FormView<T>) -> n::Form {
    let element = view.without_actions();
    let mut cx = Cx::new(false);
    let mut form = match cx.form(&element) {
        Ok(form) => form,
        // A form that can't be said stands as one part that is not native.
        Err(fail) => {
            let what = fail.what.unwrap_or_else(|| "slot:form".to_owned());
            n::Form {
                id: id(&element),
                method: n::Method::Post,
                action: String::new(),
                hidden: Vec::new(),
                children: vec![cx.miss(what, fail.why, &element)],
            }
        }
    };
    form.children = flat(form.children);
    form
}

/// The fields of the form `view` draws, as a native client's fields, in their
/// order: for a form whose address and whose id the screen writes itself. See
/// [`form`].
pub fn fields<T: Form>(view: &FormView<T>) -> Vec<Part> {
    form(view).children
}

/// The parts of `nodes` without the stacks and grids a page groups its fields
/// in. A group with an id, or one that is hidden, is something an answer or a
/// command names, and stays.
fn flat(nodes: Vec<Part>) -> Vec<Part> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            Part::Stack(stack) if stack.id.is_none() && !stack.hidden => {
                out.extend(flat(stack.children));
            }
            Part::Grid(grid) if grid.id.is_none() => out.extend(flat(grid.children)),
            other => out.push(other),
        }
    }
    out
}
