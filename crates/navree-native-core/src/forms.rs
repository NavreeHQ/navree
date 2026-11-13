//! Forms: what the person typed, how a form is sent, and what comes back.
//!
//! - A form is sent as a browser sends it: its hidden values, then each field
//!   in document order, then the pair of the button that sent it. A switch
//!   that is off and a disabled field send nothing.
//! - A post always says it is a form, even with nothing in it.
//! - One send at a time. A second that says the same (a double tap) is
//!   dropped. One that says something new waits, the latest only, and goes
//!   when the first is answered and left the screen as it was; otherwise the
//!   person is told it was not saved.
//! - A `get` form asks, and the answer decides: a screen takes this screen's
//!   place, ops are applied to it. A table's query form asks by itself as its
//!   fields change, and the answer takes the table's place.
//! - A field may say what happens as it changes: its form is sent (at once
//!   for a tick or a pick, when typing pauses for text), or commands run with
//!   what it holds (a count of the characters typed).
//! - The rows chosen in a table go with the form the table names, each by its
//!   id, after the form's own fields and before the pair of the button.
//! - A choice whose options are searched on the server asks when typing
//!   pauses. The answer is matched to the field by the request that asked,
//!   never by a name in it, and an answer to an older search is dropped.
//! - A control that asks a question first is held until the person answers:
//!   yes runs what it was about to do, no lets it go.

use navree_native::action::Pair;
use navree_native::node::{Field, Form, Method, Node};
use navree_native::{Action, Input};

use crate::decode::Answer;
use crate::effect::{Body, Change, Part};
use crate::event::FieldValue;
use crate::origin::Target;
use crate::reduce::SEND_LIMIT;
use crate::state::{Content, FieldView, FormView, Held, Notice, Pending, Question, ScreenId, TimerFor, Waiting};
use crate::tree::{fields_of, find_in, form_of, has_id};
use crate::Core;

/// How long typing in a table's query waits for a pause, in milliseconds.
const TYPING_PAUSE: u32 = 100;

fn encode_into(out: &mut String, text: &str) {
    // Line breaks are sent as CR LF, as a browser's form does.
    let mut bytes = text.bytes().peekable();
    let push = |out: &mut String, byte: u8| match byte {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => out.push(char::from(byte)),
        b' ' => out.push('+'),
        other => {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[usize::from(other >> 4)]));
            out.push(char::from(b"0123456789ABCDEF"[usize::from(other & 15)]));
        }
    };
    while let Some(byte) = bytes.next() {
        match byte {
            b'\r' => {
                if bytes.peek() == Some(&b'\n') {
                    bytes.next();
                }
                push(out, b'\r');
                push(out, b'\n');
            }
            b'\n' => {
                push(out, b'\r');
                push(out, b'\n');
            }
            other => push(out, other),
        }
    }
}

/// `name=value&…` as `application/x-www-form-urlencoded`. A file sends its name.
pub(crate) fn encode(parts: &[Part]) -> String {
    let mut out = String::new();
    for part in parts {
        let (name, value) = match part {
            Part::Text { name, value } => (name, value),
            Part::File { name, filename, .. } => (name, filename),
        };
        if !out.is_empty() {
            out.push('&');
        }
        encode_into(&mut out, name);
        out.push('=');
        encode_into(&mut out, value);
    }
    out
}

fn text(name: &str, value: &str) -> Part {
    Part::Text {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

fn field_parts(field: &Field, value: &FieldValue, out: &mut Vec<Part>) {
    let name = field.name.as_str();
    match (&field.input, value) {
        (Input::Toggle(toggle), FieldValue::Flag { on }) => {
            if *on {
                out.push(text(name, toggle.value.as_deref().unwrap_or("on")));
            }
        }
        // A phone number names the country it is read in, as a second field
        // beside it: the one the person picked, or the one the page came with.
        (Input::Text(input), FieldValue::Phone { text: number, country }) => {
            out.push(text(name, number));
            if let Some(held) = &input.country {
                out.push(text(&held.name, country));
            }
        }
        (Input::Text(input), FieldValue::Text { text: number }) if input.kind == navree_native::input::TextKind::Phone && input.country.is_some() => {
            out.push(text(name, number));
            if let Some(held) = &input.country {
                out.push(text(&held.name, &held.value));
            }
        }
        // Each end of a range goes under its own name.
        (Input::Range(range), FieldValue::Range { from, to }) => {
            out.push(text(&range.from.name, from));
            out.push(text(&range.to.name, to));
        }
        (_, FieldValue::Text { text: value }) => out.push(text(name, value)),
        (_, FieldValue::Many { values }) => out.extend(values.iter().map(|value| text(name, value))),
        (_, FieldValue::Files { files }) => out.extend(files.iter().map(|file| Part::File {
            name: name.to_owned(),
            token: file.token.clone(),
            filename: file.name.clone(),
            media_type: if file.media_type.is_empty() {
                "application/octet-stream".to_owned()
            } else {
                file.media_type.clone()
            },
        })),
        _ => {}
    }
}

/// What a form sends, in the order a browser sends it.
pub(crate) fn parts(form: &Form, state: Option<&FormView>, with: Option<&Pair>) -> Vec<Part> {
    let mut out = Vec::new();
    for hidden in &form.hidden {
        if !hidden.name.is_empty() {
            out.push(text(&hidden.name, &hidden.value));
        }
    }
    for field in fields_of(form) {
        // A range is sent under the names of its ends, whatever its own is.
        if field.disabled || (field.name.is_empty() && !matches!(field.input, Input::Range(_))) {
            continue;
        }
        let held = field.id.as_deref().and_then(|id| state?.fields.get(id)).map(|field| &field.value);
        match held {
            Some(value) => field_parts(field, value, &mut out),
            None => field_parts(field, &crate::tree::value_of(&field.input), &mut out),
        }
    }
    if let Some(with) = with.filter(|with| !with.name.is_empty()) {
        out.push(text(&with.name, &with.value));
    }
    out
}

fn body_of(parts: Vec<Part>) -> Body {
    if parts.iter().any(|part| matches!(part, Part::File { .. })) {
        Body::Multipart { parts }
    } else {
        Body::Form { text: encode(&parts) }
    }
}

/// Whether two values are of one kind, so one can take the other's place.
fn same_kind(a: &FieldValue, b: &FieldValue) -> bool {
    use FieldValue::{Files, Flag, Many, None, Phone, Range, Text};
    matches!(
        (a, b),
        (Text { .. } | None | Phone { .. }, Text { .. } | None | Phone { .. }) | (Flag { .. }, Flag { .. }) | (Many { .. }, Many { .. }) | (Files { .. }, Files { .. }) | (Range { .. }, Range { .. })
    )
}

impl Core {
    /// The person changed a field.
    pub(crate) fn field_changed(&mut self, id: ScreenId, form: Option<&str>, field: &str, value: FieldValue) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        // The form the client named, or whichever holds the field.
        let named = form.filter(|form| view.forms.get(*form).is_some_and(|form| form.fields.contains_key(field)));
        let handle = match named {
            Some(form) => form.to_owned(),
            None => match view.forms.iter().find(|(_, form)| form.fields.contains_key(field)) {
                Some((handle, _)) => handle.clone(),
                None => return,
            },
        };
        let Some(held) = view.forms.get_mut(&handle).and_then(|form| form.fields.get_mut(field)) else {
            return;
        };
        if !same_kind(&held.value, &value) {
            return;
        }
        held.value = value;
        // What was wrong with it was wrong with what it held before.
        held.errors.clear();
        inner.dirty.insert(field.to_owned());
        self.changed.insert(Change::Form {
            screen: id,
            form: handle.clone(),
        });
        let (typed, changed) = match view.screen().and_then(|screen| find_in(screen, &|node| matches!(node, Node::Field(_)) && has_id(node, field))) {
            Some(Node::Field(found)) => (matches!(found.input, Input::Text(_)), found.changed.clone()),
            _ => (false, Option::None),
        };
        // A table's query asks by itself: at once for a choice, after a pause for typing.
        let asks = inner.forms.get(&handle).is_some_and(|meta| meta.query_of.is_some());
        let sends = matches!(changed, Some(Action::Submit { .. }));
        if asks || sends {
            let old = inner.query_timers.remove(&handle);
            if let Some(old) = old {
                self.cancel_timer(old);
            }
        }
        if asks {
            if typed {
                let timer = self.timer(TYPING_PAUSE, TimerFor::Query { screen: id, form: handle.clone() });
                if let Some(inner) = self.inner.screens.get_mut(&id) {
                    inner.query_timers.insert(handle, timer);
                }
            } else {
                self.query(id, &handle);
            }
            return;
        }
        // What the field itself says happens as it changes.
        match changed {
            Some(Action::Commands { list }) => {
                let held = self.view.screens.get(&id).and_then(|view| view.forms.get(&handle)).and_then(|form| form.fields.get(field)).and_then(|field| match &field.value {
                    FieldValue::Text { text } => Some(text.clone()),
                    _ => Option::None,
                });
                self.commands(id, list, held.as_deref());
            }
            Some(action @ Action::Submit { .. }) if typed => {
                let timer = self.timer(
                    TYPING_PAUSE,
                    TimerFor::Changed {
                        screen: id,
                        form: handle.clone(),
                        action: Box::new(action),
                    },
                );
                if let Some(inner) = self.inner.screens.get_mut(&id) {
                    inner.query_timers.insert(handle, timer);
                }
            }
            Some(action @ Action::Submit { .. }) => self.changed_fire(id, &handle, action),
            _ => {}
        }
    }

    /// Sends the form a field stands in, because the field changed and says so.
    pub(crate) fn changed_fire(&mut self, id: ScreenId, handle: &str, action: Action) {
        if let Action::Submit { form, href, with } = action {
            self.submit(id, form.as_deref().unwrap_or(handle), href, with);
        }
    }

    // A choice whose options are found on the server.

    /// The control of the field `field`, when it is a choice.
    fn choice_of(&self, id: ScreenId, field: &str) -> Option<(Option<navree_native::input::ChoiceSearch>, Option<String>)> {
        let screen = self.view.screens.get(&id)?.screen()?;
        match find_in(screen, &|node| matches!(node, Node::Field(_)) && has_id(node, field))? {
            Node::Field(found) => match &found.input {
                Input::Choice(choice) => Some((choice.search.clone(), choice.more.clone())),
                Input::Choices(choices) => Some((choices.search.clone(), choices.more.clone())),
                _ => Option::None,
            },
            _ => Option::None,
        }
    }

    fn field_state(&mut self, id: ScreenId, form: &str, field: &str) -> Option<&mut FieldView> {
        self.view.screens.get_mut(&id)?.forms.get_mut(form)?.fields.get_mut(field)
    }

    /// The person typed in a searched choice, or wants the options after
    /// those it shows.
    pub(crate) fn options(&mut self, id: ScreenId, form: Option<&str>, field: &str, words: Option<String>, more: bool) {
        let Some(view) = self.view.screens.get(&id) else {
            return;
        };
        // The form the client named, or whichever holds the field.
        let named = form.filter(|form| view.forms.get(*form).is_some_and(|form| form.fields.contains_key(field)));
        let Some(handle) = named.map(str::to_owned).or_else(|| view.forms.iter().find(|(_, form)| form.fields.contains_key(field)).map(|(handle, _)| handle.clone())) else {
            return;
        };
        let Some((search, own_more)) = self.choice_of(id, field) else {
            return;
        };
        let Some(inner) = self.inner.screens.get_mut(&id) else {
            return;
        };
        if more {
            // The options after the ones found; or after the control's own, before any search.
            let ask = inner.options.entry(field.to_owned()).or_default();
            let Some(next) = ask.more.clone().or(own_more.filter(|_| ask.turn == 0)) else {
                return;
            };
            return self.ask_options(id, &handle, field, &next, true);
        }
        let Some(search) = search else {
            return;
        };
        let words = words.unwrap_or_default();
        let ask = inner.options.entry(field.to_owned()).or_default();
        let old = ask.timer.take();
        ask.words.clone_from(&words);
        if let Some(old) = old {
            self.cancel_timer(old);
        }
        if words.chars().count() < usize::from(search.min) {
            // Too few characters to search with: the control shows its own options again,
            // and an answer on its way is for a search that is over.
            if let Some(ask) = self.inner.screens.get_mut(&id).and_then(|inner| inner.options.get_mut(field)) {
                ask.turn += 1;
                ask.more = Option::None;
            }
            if let Some(state) = self.field_state(id, &handle, field)
                && (state.options.is_some() || state.searching || state.more)
            {
                state.options = Option::None;
                state.searching = false;
                state.more = false;
                self.changed.insert(Change::Form { screen: id, form: handle });
            }
            return;
        }
        let timer = self.timer(
            TYPING_PAUSE,
            TimerFor::Options {
                screen: id,
                form: handle,
                field: field.to_owned(),
            },
        );
        if let Some(ask) = self.inner.screens.get_mut(&id).and_then(|inner| inner.options.get_mut(field)) {
            ask.timer = Some(timer);
        }
    }

    /// The typing paused: the search is asked.
    pub(crate) fn options_fire(&mut self, id: ScreenId, form: &str, field: &str) {
        let Some((Some(search), _)) = self.choice_of(id, field) else {
            return;
        };
        let Some(ask) = self.inner.screens.get_mut(&id).and_then(|inner| inner.options.get_mut(field)) else {
            return;
        };
        ask.timer = Option::None;
        let words = ask.words.clone();
        let mut href = search.href.clone();
        href.push(if href.contains('?') { '&' } else { '?' });
        href.push_str(&encode(&[text(&search.name, &words)]));
        self.ask_options(id, form, field, &href, false);
    }

    fn ask_options(&mut self, id: ScreenId, form: &str, field: &str, href: &str, more: bool) {
        let Target::Inside(url) = self.resolve(href, id) else {
            return;
        };
        let Some(inner) = self.inner.screens.get_mut(&id) else {
            return;
        };
        let ask = inner.options.entry(field.to_owned()).or_default();
        ask.turn += 1;
        let purpose = Pending::Options {
            screen: id,
            visit: inner.visit,
            form: form.to_owned(),
            field: field.to_owned(),
            turn: ask.turn,
            more,
        };
        // A search that began before this one is out of date.
        self.drop_pending(|purpose| matches!(purpose, Pending::Options { screen, field: other, .. } if *screen == id && other == field));
        self.fetch(purpose, Method::Get, &url, Option::None, SEND_LIMIT);
        if let Some(state) = self.field_state(id, form, field)
            && !state.searching
        {
            state.searching = true;
            self.changed.insert(Change::Form {
                screen: id,
                form: form.to_owned(),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn options_answered(&mut self, id: ScreenId, visit: u64, form: &str, field: &str, turn: u64, more: bool, answer: Answer) {
        let Some(inner) = self.inner.screens.get_mut(&id) else {
            return;
        };
        // The answer is this field's because this request asked for it. One
        // for an older search, or for a screen since asked again, is dropped.
        if inner.visit != visit || inner.options.get(field).is_none_or(|ask| ask.turn != turn) {
            return;
        }
        let mut found = Option::None;
        if let Answer::Tree { status, mut read, shell, .. } = answer {
            if self.note(&mut read, shell) {
                return;
            }
            if let Some(way_in) = Core::way_in(status, &read.envelope.ops) {
                return self.session_ended(id, &way_in);
            }
            // The first field in the answer holds the options, whatever its name.
            if let Some(screen) = read.envelope.screen.as_ref().filter(|_| status < 400)
                && let Some(Node::Field(holder)) = find_in(screen, &|node| matches!(node, Node::Field(_)))
            {
                found = match &holder.input {
                    Input::Choice(choice) => Some((choice.options.clone(), choice.more.clone())),
                    Input::Choices(choices) => Some((choices.options.clone(), choices.more.clone())),
                    _ => Option::None,
                };
            }
        }
        if let Some(ask) = self.inner.screens.get_mut(&id).and_then(|inner| inner.options.get_mut(field))
            && let Some((_, next)) = &found
        {
            ask.more.clone_from(next);
        }
        let Some(state) = self.field_state(id, form, field) else {
            return;
        };
        state.searching = false;
        if let Some((mut options, next)) = found {
            if more && let Some(held) = state.options.take() {
                // The next ones join those already found; an option is never there twice.
                options.retain(|option| !held.iter().any(|other| other.value == option.value));
                options.splice(0..0, held);
            }
            state.options = Some(options);
            state.more = next.is_some();
        }
        self.changed.insert(Change::Form {
            screen: id,
            form: form.to_owned(),
        });
    }

    // A control that asks first.

    /// Holds what a control was about to do, and shows its question.
    pub(crate) fn ask_first(&mut self, id: ScreenId, ask: navree_native::node::Question, mut held: Held) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        self.inner.next.question += 1;
        held.question = self.inner.next.question;
        view.question = Some(Question {
            id: held.question,
            title: ask.title,
            text: ask.text,
            confirm: ask.confirm,
            cancel: ask.cancel,
        });
        inner.held = Some(held);
        self.touch_screen(id);
    }

    /// The person answered a screen's question.
    pub(crate) fn answer(&mut self, id: ScreenId, question: u64, yes: bool) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        // An answer to a question that is no longer asked changes nothing.
        if view.question.as_ref().is_none_or(|asked| asked.id != question) {
            return;
        }
        view.question = Option::None;
        let held = inner.held.take();
        self.touch_screen(id);
        if let (true, Some(held)) = (yes, held) {
            self.act(id, held.action, held.form, held.overlay, held.text, held.now);
        }
    }

    /// Asks a table's query: the form's values as a query to its address,
    /// answered in the table's place.
    pub(crate) fn query(&mut self, id: ScreenId, handle: &str) {
        let (Some(view), Some(inner)) = (self.view.screens.get(&id), self.inner.screens.get(&id)) else {
            return;
        };
        let Some(region) = inner.forms.get(handle).and_then(|meta| meta.query_of.clone()) else {
            return;
        };
        let Some(form) = view.screen().and_then(|screen| form_of(screen, handle)) else {
            return;
        };
        // An empty control says nothing: it stays out of the address.
        let mut said = parts(form, view.forms.get(handle), None);
        said.retain(|part| !matches!(part, Part::Text { name, value } if value.is_empty() && name != "sort"));
        let action = if form.action.is_empty() { view.url.clone() } else { form.action.clone() };
        let Target::Inside(address) = self.resolve(&action, id) else {
            return;
        };
        let query = encode(&said);
        let url = if query.is_empty() {
            address.split('?').next().unwrap_or("/").to_owned()
        } else {
            format!("{}?{query}", address.split('?').next().unwrap_or("/"))
        };
        self.region_go(id, &region, url);
    }

    /// Asks a screen's own search: the words under the search's name, with
    /// what else the search carries, as a query to its address. The answer
    /// takes the screen's place.
    pub(crate) fn search(&mut self, id: ScreenId, words: &str) {
        let Some(view) = self.view.screens.get(&id) else {
            return;
        };
        let Some(search) = view.screen().and_then(|screen| screen.search.as_ref()) else {
            return;
        };
        let mut said: Vec<Part> = search.hidden.iter().map(|pair| text(&pair.name, &pair.value)).collect();
        if !words.is_empty() {
            said.push(text(&search.name, words));
        }
        let action = if search.action.is_empty() { view.url.clone() } else { search.action.clone() };
        let Target::Inside(address) = self.resolve(&action, id) else {
            return;
        };
        let path = address.split('?').next().unwrap_or("/");
        let query = encode(&said);
        let url = if query.is_empty() { path.to_owned() } else { format!("{path}?{query}") };
        if url != view.url {
            self.replace(id, &url);
        }
    }

    /// The form the server calls `id`, or the form that goes by it here.
    pub(crate) fn form_named(&self, screen: ScreenId, id: &str) -> Option<String> {
        let inner = self.inner.screens.get(&screen)?;
        let view = self.view.screens.get(&screen)?.screen()?;
        // The first in document order, as a lookup by id finds it.
        let mut found = None;
        for root in crate::tree::roots(view) {
            crate::tree::walk(root, &mut |node| {
                let handle = match node {
                    Node::Form(form) => form.id.as_deref(),
                    Node::Table(table) => table.query.as_ref().and_then(|query| query.id.as_deref()),
                    _ => None,
                };
                if let Some(handle) = handle
                    && found.is_none()
                    && inner.server_ids.get(handle).is_some_and(|server| server.as_deref() == Some(id))
                {
                    found = Some(handle.to_owned());
                }
            });
        }
        found.or_else(|| inner.forms.contains_key(id).then(|| id.to_owned()))
    }

    /// Whether anything may be sent from the screen. A kept tree may hold a
    /// token that is no longer good: it waits for the fresh one.
    pub(crate) fn may_send(&mut self, id: ScreenId) -> bool {
        let waits = self.view.screens.get(&id).is_some_and(|view| view.cached);
        if waits {
            self.notice(Notice::Loading, Some(id));
        }
        !waits
    }

    /// Sends a form. `named` is the id an action or an op knows it by.
    pub(crate) fn submit(&mut self, id: ScreenId, named: &str, href: Option<String>, with: Option<Pair>) {
        let Some(handle) = self.form_named(id, named) else {
            return;
        };
        if !self.may_send(id) {
            return;
        }
        let chosen = self.chosen(id, &handle);
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let Some(form) = view.screen().and_then(|screen| form_of(screen, &handle)) else {
            return;
        };
        if inner.forms.get(&handle).is_some_and(|meta| meta.query_of.is_some()) {
            return self.query(id, &handle);
        }
        let method = form.method;
        // The form's own fields, the rows chosen in the tables that go with
        // it, and last the pair of the button that sent it.
        let mut sent = parts(form, view.forms.get(&handle), Option::None);
        sent.extend(chosen);
        if let Some(with) = with.as_ref().filter(|with| !with.name.is_empty()) {
            sent.push(text(&with.name, &with.value));
        }
        let action = match &href {
            Some(href) => href.clone(),
            None if form.action.is_empty() => view.url.clone(),
            None => form.action.clone(),
        };
        let Target::Inside(address) = self.inner.origin.resolve(&action, &view.url) else {
            // Only this site's addresses are posted to.
            return self.notice(Notice::Failed, Some(id));
        };
        let said = format!("{method:?} {address} {}", encode(&sent));
        if let Some(out) = inner.sending.get(&handle) {
            // A send that can't be taken back is on its way. The same again is
            // one send; something new waits for it, the latest only.
            if *out == said {
                inner.waiting.remove(&handle);
            } else {
                inner.waiting.insert(handle, Waiting { said, with, href });
            }
            return;
        }
        inner.sending.insert(handle.clone(), said);
        if let Some(form) = view.forms.get_mut(&handle) {
            form.pending = true;
        }
        let visit = inner.visit;
        if method == Method::Get {
            // A form that asks may be answered with a screen: the screen says it is loading.
            view.loading = true;
            self.changed.insert(Change::Screen { id });
        }
        self.changed.insert(Change::Form {
            screen: id,
            form: handle.clone(),
        });
        let get = method == Method::Get;
        let purpose = Pending::Send {
            screen: id,
            visit,
            key: handle,
            write: !get,
            in_place: get,
        };
        if get {
            let query = encode(&sent);
            let path = address.split('?').next().unwrap_or("/");
            let url = if query.is_empty() { path.to_owned() } else { format!("{path}?{query}") };
            self.fetch(purpose, Method::Get, &url, None, SEND_LIMIT);
        } else {
            self.fetch(purpose, Method::Post, &address, Some(body_of(sent)), SEND_LIMIT);
        }
    }

    /// A button's own post. `action` is what the button holds, so the button
    /// shows that it waits and a second tap is not a second post.
    pub(crate) fn post(&mut self, id: ScreenId, href: &str, fields: Vec<Pair>, action: Option<Action>) {
        self.request(id, Method::Post, href, fields, action);
    }

    pub(crate) fn request(&mut self, id: ScreenId, method: Method, href: &str, fields: Vec<Pair>, action: Option<Action>) {
        let url = match self.resolve(href, id) {
            Target::Inside(url) => url,
            Target::Outside(url) => return self.effects.push(crate::Effect::OpenUrl { url, inside: false }),
            Target::Refused => return,
        };
        if !self.may_send(id) {
            return;
        }
        let Some(inner) = self.inner.screens.get_mut(&id) else {
            return;
        };
        let sent: Vec<Part> = fields.iter().map(|pair| text(&pair.name, &pair.value)).collect();
        let said = format!("{method:?} {url} {}", encode(&sent));
        let key = format!("request {said}");
        // A second tap while the first is on its way is not a second request.
        if inner.sending.contains_key(&key) {
            return;
        }
        inner.sending.insert(key.clone(), said);
        if let Some(action) = action {
            inner.busy.push((key.clone(), action));
        }
        let visit = inner.visit;
        let get = method == Method::Get;
        let purpose = Pending::Send {
            screen: id,
            visit,
            key,
            write: !get,
            in_place: false,
        };
        if get {
            let query = encode(&sent);
            let url = if query.is_empty() { url } else { format!("{}?{query}", url.split('?').next().unwrap_or("/")) };
            self.fetch(purpose, Method::Get, &url, None, SEND_LIMIT);
        } else {
            self.fetch(purpose, Method::Post, &url, Some(body_of(sent)), SEND_LIMIT);
        }
        self.mark_busy(id);
    }

    /// Marks the buttons whose own request is on its way as waiting, and the
    /// others as not.
    pub(crate) fn mark_busy(&mut self, id: ScreenId) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get(&id)) else {
            return;
        };
        let Content::Native { screen } = &mut view.content else {
            return;
        };
        let busy = &inner.busy;
        let mut changed = false;
        fn mark(node: &mut Node, busy: &[(String, Action)], changed: &mut bool) {
            if let Node::Button(button) = node
                && matches!(button.action, Some(Action::Request { .. }))
            {
                let waits = button.action.as_ref().is_some_and(|action| busy.iter().any(|(_, sent)| sent == action));
                if button.pending != waits {
                    button.pending = waits;
                    *changed = true;
                }
            }
            for slot in crate::tree::slots(node) {
                match slot {
                    crate::tree::Slot::Many(list) => list.iter_mut().for_each(|child| mark(child, busy, changed)),
                    crate::tree::Slot::Maybe(child) => {
                        if let Some(child) = child.as_deref_mut() {
                            mark(child, busy, changed);
                        }
                    }
                    crate::tree::Slot::One(child) => mark(child, busy, changed),
                }
            }
        }
        for action in &mut screen.actions {
            mark(action, busy, &mut changed);
        }
        mark(&mut screen.body, busy, &mut changed);
        if changed {
            self.touch_screen(id);
        }
    }

    /// The action of something the person tapped.
    pub(crate) fn act(&mut self, id: ScreenId, action: Action, form: Option<String>, overlay: Option<String>, text: Option<String>, now: Option<i64>) {
        if !self.view.holds(id) {
            return;
        }
        match action {
            Action::Navigate { href, present } => {
                self.navigate(Some(id), &href, present);
            }
            Action::Submit { form: named, href, with } => {
                // The form the button names, or the one it stands in.
                if let Some(named) = named.or(form) {
                    self.close_alert(id, overlay.as_deref());
                    self.submit(id, &named, href, with);
                }
            }
            Action::Request { ref method, ref href, ref fields } => {
                let (method, href, fields) = (*method, href.clone(), fields.clone());
                // A system alert goes away when one of its buttons is tapped.
                self.close_alert(id, overlay.as_deref());
                self.request(id, method, &href, fields, Some(action));
            }
            Action::Commands { list } => self.commands(id, list, text.as_deref()),
            Action::Back => self.back(),
            Action::Refresh => self.fetch_page(id, true),
            Action::Dismiss => self.dismiss(id, overlay.as_deref()),
            Action::Device(device) => self.device_call(id, device, now),
            // An action this build does not know does nothing.
            _ => {}
        }
    }

    /// Closes the alert a tapped button stands in.
    fn close_alert(&mut self, id: ScreenId, overlay: Option<&str>) {
        let Some(overlay) = overlay else {
            return;
        };
        let alert = self.view.screens.get(&id).and_then(|view| view.screen()).and_then(|screen| find_in(screen, &|node| has_id(node, overlay) && matches!(node, Node::Dialog(_)))).is_some_and(|node| matches!(node, Node::Dialog(dialog) if dialog.alert));
        if alert {
            self.set_open(id, overlay, false);
        }
    }

    /// Closes the sheet or dialog a control stands in; without one, the one
    /// opened last; without any, the sheet or cover the screen is presented in.
    fn dismiss(&mut self, id: ScreenId, overlay: Option<&str>) {
        let open = self.view.screens.get(&id).and_then(|view| match overlay {
            Some(overlay) => view.overlays.iter().find(|open| *open == overlay).cloned(),
            None => view.overlays.last().cloned(),
        });
        match open {
            Some(open) => self.set_open(id, &open, false),
            None if self.view.modals.iter().any(|modal| !modal.locked && modal.stack.contains(&id)) => self.dismissed(id),
            None => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn send_answered(&mut self, id: ScreenId, visit: u64, key: &str, write: bool, in_place: bool, asked: &str, answer: Answer) {
        // The send is over, whatever came of it.
        let mut waiting = None;
        let mut chrome = true;
        let mut same_visit = false;
        if let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) {
            inner.sending.remove(key);
            inner.busy.retain(|(sent, _)| sent != key);
            waiting = inner.waiting.remove(key);
            inner.replaced.clear();
            chrome = view.chrome;
            same_visit = inner.visit == visit;
            if let Some(form) = view.forms.get_mut(key) {
                form.pending = false;
                self.changed.insert(Change::Form {
                    screen: id,
                    form: key.to_owned(),
                });
            }
            if in_place {
                view.loading = false;
            }
        }
        self.mark_busy(id);
        let alive = self.view.holds(id);
        let front = self.front() == Some(id);
        let cover = self.view.modals.iter().find(|modal| modal.stack.contains(&id)).and_then(|modal| modal.stack.first()).and_then(|first| self.inner.screens.get(first)).and_then(|inner| inner.cover.clone());
        let mut told = false;
        // Whether the answer left the screen as it was: ops that lead nowhere.
        let mut kept = false;
        match answer {
            Answer::Failed(failure) => {
                self.failed(failure, Some(id));
                told = true;
            }
            Answer::Left(url) => self.effects.push(crate::Effect::OpenUrl { url, inside: false }),
            Answer::Empty => {
                if write {
                    self.wrote(id);
                }
                kept = true;
            }
            Answer::Html { url } => {
                if write {
                    self.wrote(id);
                }
                if alive && front {
                    let shown = crate::reduce::Shown::Web {
                        url: url.clone(),
                        title: None,
                        why: vec!["html".to_owned()],
                    };
                    self.arrive(id, &url, shown);
                }
            }
            Answer::Tree { url, status, mut read, body, shell } => {
                if self.note(&mut read, shell) {
                    return;
                }
                if let Some(way_in) = Core::way_in(status, &read.envelope.ops) {
                    // The session ended before this was sent. What the answer
                    // says is said; the way in is put up; what was typed stays
                    // where it is for when the person is back.
                    for op in std::mem::take(&mut read.envelope.ops) {
                        if let navree_native::Op::Toast(toast) = op {
                            self.toast(None, toast);
                        }
                    }
                    if let Some(under) = alive.then_some(id).or(self.front()) {
                        self.session_ended(under, &way_in);
                    }
                    return;
                }
                let worked = status < 400;
                if write && worked {
                    self.wrote(id);
                    // A screen with no chrome is a sign-in or the like: who is signed in may have changed.
                    if !chrome {
                        self.fetch_shell();
                    }
                }
                let reload = read.reload;
                let shown = self.shown(&url, status, &mut read, body);
                let ops = std::mem::take(&mut read.envelope.ops);
                let leads = shown.is_some() || ops.iter().any(|op| matches!(op, navree_native::Op::Navigate { .. } | navree_native::Op::NavigateExternal { .. }));
                if !alive || !front {
                    // The person has moved on: the answer changes its own screen's
                    // tree, says what it has to say, and takes them nowhere.
                    let mut quiet = Vec::new();
                    for op in ops {
                        match op {
                            navree_native::Op::Navigate { .. } | navree_native::Op::NavigateExternal { .. } | navree_native::Op::Reload | navree_native::Op::Open { .. } | navree_native::Op::Focus { .. } | navree_native::Op::Url { .. } => {}
                            other => quiet.push(other),
                        }
                    }
                    if alive {
                        self.apply_ops(id, quiet, false);
                        if let Some(inner) = self.inner.screens.get_mut(&id) {
                            inner.stale |= leads;
                        }
                    } else {
                        for op in quiet {
                            match op {
                                navree_native::Op::Toast(toast) => self.toast(None, toast),
                                // Signing out is no less so for having left the screen it was asked from.
                                navree_native::Op::SignedOut => self.signed_out_on_purpose(),
                                _ => {}
                            }
                        }
                    }
                } else if cover.as_ref().is_some_and(|route| write && worked && leaves(route, &url, shown.is_some(), &ops)) {
                    // The way in the session's end put up has done its work: the
                    // person is back where they were. Who is signed in may have
                    // changed, so the shell is asked for again.
                    for op in ops {
                        if let navree_native::Op::Toast(toast) = op {
                            self.toast(None, toast);
                        }
                    }
                    self.way_in_done(id);
                    // The page the sign-in led to is often the very one they
                    // were on: it is shown there. Otherwise that screen is
                    // fetched again as it comes to the front.
                    let under = self.front().filter(|under| self.view.screens.get(under).is_some_and(|view| view.url == url));
                    if let (Some(under), Some(shown)) = (under, shown) {
                        self.show(under, &url, shown);
                        if let Some(inner) = self.inner.screens.get_mut(&under) {
                            inner.stale = false;
                        }
                    }
                } else if let Some(route) = self.covers(asked, &url).filter(|_| shown.is_some()) {
                    self.cover(id, route, &url, shown);
                    self.apply_ops(self.front().unwrap_or(id), ops, false);
                } else {
                    let mut current = id;
                    let showed = shown.is_some();
                    if let Some(shown) = shown {
                        if in_place && same_visit {
                            // A query of the screen itself: the answer takes its place, and
                            // what was typed stays. A sheet the query was sent from has done its work.
                            self.close_overlays(id);
                            self.show(id, &url, shown);
                        } else {
                            current = self.arrive(id, &url, shown);
                        }
                    } else if !worked && ops.is_empty() {
                        self.notice(Notice::Failed, Some(id));
                        told = true;
                    }
                    let left = self.apply_ops(current, ops, true);
                    kept = !showed && !left;
                    if reload && !left {
                        // Something in the answer could not be read: the screen is asked for again.
                        self.fetch_page(current, false);
                    }
                }
            }
        }
        // What was typed meanwhile goes now, from the form as it is, only where
        // it was typed: after ops that left the screen alone, on the same visit,
        // to the same form.
        if let Some(waiting) = waiting {
            let there = self.inner.screens.get(&id).is_some_and(|inner| inner.visit == visit && inner.forms.contains_key(key) && !inner.replaced.contains(key));
            if kept && there {
                self.submit(id, key, waiting.href, waiting.with);
            } else if !told {
                self.notice(Notice::Unsaved, Some(id));
            }
        }
    }
}

/// Whether an answer leads away from the route `pattern`: it navigates to
/// an address the route does not name, or brings a screen from one.
fn leaves(pattern: &str, url: &str, showed: bool, ops: &[navree_native::Op]) -> bool {
    let away = ops.iter().any(|op| match op {
        navree_native::Op::Navigate { href, .. } => !crate::origin::route_matches(pattern, href),
        navree_native::Op::NavigateExternal { .. } => true,
        _ => false,
    });
    away || (showed && !crate::origin::route_matches(pattern, url))
}

/// What a field holds when a tree first says so.
pub(crate) fn field_view(field: &Field) -> FieldView {
    FieldView {
        name: field.name.clone(),
        value: crate::tree::value_of(&field.input),
        errors: field.error.iter().cloned().collect(),
        options: None,
        more: false,
        searching: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use navree_native::input::{ChoicesInput, TextInput, ToggleInput};
    use navree_native::node::Hidden;

    fn field(id: &str, name: &str, input: Input) -> Node {
        Node::Field(Field {
            id: Some(id.to_owned()),
            name: name.to_owned(),
            input,
            ..Field::default()
        })
    }

    fn typed(value: &str) -> Input {
        Input::Text(TextInput {
            value: value.to_owned(),
            ..TextInput::default()
        })
    }

    #[test]
    fn a_form_is_encoded_as_a_browser_would() {
        let form = Form {
            hidden: vec![Hidden {
                name: "_csrf".to_owned(),
                value: "t0k/en+".to_owned(),
                id: None,
            }],
            children: vec![
                field("a", "note", typed("चाय & coffee\nline two")),
                field("b", "gift", Input::Toggle(ToggleInput::default())),
                field(
                    "c",
                    "wrap",
                    Input::Toggle(ToggleInput {
                        on: true,
                        value: Some("paper".to_owned()),
                        ..ToggleInput::default()
                    }),
                ),
                field(
                    "d",
                    "tag[]",
                    Input::Choices(ChoicesInput {
                        values: vec!["new".to_owned(), "sale".to_owned()],
                        ..ChoicesInput::default()
                    }),
                ),
                Node::Field(Field {
                    id: Some("e".to_owned()),
                    name: "locked".to_owned(),
                    disabled: true,
                    input: typed("never sent"),
                    ..Field::default()
                }),
            ],
            ..Form::default()
        };
        let with = Pair {
            name: "intent".to_owned(),
            value: "save".to_owned(),
        };
        let sent = parts(&form, None, Some(&with));
        assert_eq!(
            encode(&sent),
            "_csrf=t0k%2Fen%2B&note=%E0%A4%9A%E0%A4%BE%E0%A4%AF+%26+coffee%0D%0Aline+two&wrap=paper&tag%5B%5D=new&tag%5B%5D=sale&intent=save"
        );
        // What the person typed is what is sent, not what the tree came with.
        let mut state = FormView::default();
        state.fields.insert(
            "a".to_owned(),
            FieldView {
                name: "note".to_owned(),
                value: FieldValue::Text { text: "a~b*c".to_owned() },
                errors: Vec::new(),
                options: None,
                more: false,
                searching: false,
            },
        );
        state.fields.insert(
            "b".to_owned(),
            FieldView {
                name: "gift".to_owned(),
                value: FieldValue::Flag { on: true },
                errors: Vec::new(),
                options: None,
                more: false,
                searching: false,
            },
        );
        let sent = parts(&form, Some(&state), None);
        assert_eq!(encode(&sent), "_csrf=t0k%2Fen%2B&note=a%7Eb*c&gift=on&wrap=paper&tag%5B%5D=new&tag%5B%5D=sale");
        assert_eq!(body_of(sent.clone()), Body::Form { text: encode(&sent) });
    }

    #[test]
    fn a_file_makes_the_post_multipart() {
        let form = Form {
            children: vec![field("p", "photo", Input::File(navree_native::input::FileInput::default()))],
            ..Form::default()
        };
        let mut state = FormView::default();
        state.fields.insert(
            "p".to_owned(),
            FieldView {
                name: "photo".to_owned(),
                value: FieldValue::Files {
                    files: vec![crate::FileRef {
                        token: "f1".to_owned(),
                        name: "bill.jpg".to_owned(),
                        media_type: String::new(),
                        size: 2048,
                    }],
                },
                errors: Vec::new(),
                options: None,
                more: false,
                searching: false,
            },
        );
        let Body::Multipart { parts } = body_of(parts(&form, Some(&state), None)) else {
            panic!("not multipart");
        };
        assert_eq!(
            parts,
            [Part::File {
                name: "photo".to_owned(),
                token: "f1".to_owned(),
                filename: "bill.jpg".to_owned(),
                media_type: "application/octet-stream".to_owned(),
            }]
        );
    }
}
