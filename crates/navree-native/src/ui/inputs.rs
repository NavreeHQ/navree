//! Builders for the controls of a form's fields: a control is built on its
//! own and given to [`Field::new`](crate::node::Field::new) with the name its
//! value is sent under.

use crate::input::{
    ChoiceCreate, ChoiceInput, ChoiceKind, ChoiceOption, ChoiceSearch, ChoicesInput, ColourInput,
    DateInput, DateKind, FileInput, Input, PhoneCountry, RangeEnd, RangeInput, SliderInput,
    TextInput, TextKind, ToggleInput, ToggleKind, Weekday,
};

macro_rules! inputs {
    ($($variant:ident($ty:ty),)+) => {
        $(
            impl From<$ty> for Input {
                #[inline]
                fn from(input: $ty) -> Self {
                    Input::$variant(input)
                }
            }
        )+
    };
}

inputs! {
    Text(TextInput), Toggle(ToggleInput), Choice(ChoiceInput), Choices(ChoicesInput),
    Date(DateInput), Slider(SliderInput), Colour(ColourInput), File(FileInput),
    Range(RangeInput),
}

impl TextInput {
    /// Typed text of the kind `kind`: a client shows the keyboard and the
    /// help that go with it.
    pub fn new(kind: TextKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    /// Any text.
    pub fn plain() -> Self {
        Self::new(TextKind::Plain)
    }

    /// An email address.
    pub fn email() -> Self {
        Self::new(TextKind::Email)
    }

    /// A password, hidden as it is typed.
    pub fn password() -> Self {
        Self::new(TextKind::Password)
    }

    /// A whole number, sent as plain digits.
    pub fn number() -> Self {
        Self::new(TextKind::Number)
    }

    /// A number with a fraction, sent as plain digits with `.` for the
    /// decimal point.
    pub fn decimal() -> Self {
        Self::new(TextKind::Decimal)
    }

    /// A phone number.
    pub fn phone() -> Self {
        Self::new(TextKind::Phone)
    }

    /// A web address.
    pub fn url() -> Self {
        Self::new(TextKind::Url)
    }

    /// A search.
    pub fn search() -> Self {
        Self::new(TextKind::Search)
    }

    /// Several lines.
    pub fn multiline() -> Self {
        Self::new(TextKind::Multiline)
    }

    /// A code sent to the person, which a system can fill in.
    pub fn one_time_code() -> Self {
        Self::new(TextKind::OneTimeCode)
    }

    /// What it holds now. A number is plain digits with `.` for the decimal
    /// point and no grouping (`100000.50`), whatever the person's locale
    /// writes.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// What it shows while empty.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// The most characters it takes.
    #[must_use]
    pub fn max_length(mut self, characters: u32) -> Self {
        self.max_length = Some(characters);
        self
    }

    /// The least and the most a number may be.
    #[must_use]
    pub fn between(mut self, min: impl ToString, max: impl ToString) -> Self {
        self.min = Some(min.to_string());
        self.max = Some(max.to_string());
        self
    }

    /// The step between numbers.
    #[must_use]
    pub fn step(mut self, step: impl ToString) -> Self {
        self.step = Some(step.to_string());
        self
    }

    /// What a system may fill it with, by the web's own names: `email`,
    /// `current-password`, `one-time-code`.
    #[must_use]
    pub fn autofill(mut self, name: impl Into<String>) -> Self {
        self.autofill = Some(name.into());
        self
    }

    /// It can be read and not changed. Its value is still sent.
    #[must_use]
    pub fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }

    /// What stands before the typed text, and is not sent with it: `₹`.
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// What stands after the typed text, and is not sent with it: `kg`.
    #[must_use]
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// For a phone number: the country it is read in, sent as a second field
    /// named `name` beside the number. `value` is the country now, as its
    /// two-letter code in capitals: `IN`.
    #[must_use]
    pub fn country(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.country = Some(PhoneCountry {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

impl ToggleInput {
    /// A switch: it takes effect as it flips.
    pub fn switch() -> Self {
        Self {
            kind: ToggleKind::Switch,
            ..Self::default()
        }
    }

    /// A checkbox: it is sent with its form.
    pub fn checkbox() -> Self {
        Self {
            kind: ToggleKind::Checkbox,
            ..Self::default()
        }
    }

    /// It is on.
    #[must_use]
    pub fn on(mut self) -> Self {
        self.on = true;
        self
    }

    /// It is on when `on` is true.
    #[must_use]
    pub fn set(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// What is sent when it is on, when it is not `on`.
    #[must_use]
    pub fn sends(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
}

impl ChoiceOption {
    /// An option: the value it sends and the words it shows.
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            ..Self::default()
        }
    }

    /// A quieter line beside its words.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// It can't be chosen now.
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

impl<V: Into<String>, L: Into<String>> From<(V, L)> for ChoiceOption {
    fn from((value, label): (V, L)) -> Self {
        Self::new(value, label)
    }
}

/// The options of `options`, each an option or a value with its words.
fn options_of<O: Into<ChoiceOption>>(options: impl IntoIterator<Item = O>) -> Vec<ChoiceOption> {
    options.into_iter().map(Into::into).collect()
}

impl ChoiceInput {
    /// One of `options`, drawn as the system draws a choice of `kind`. Each
    /// option is a [`ChoiceOption`] or a value with its words.
    pub fn of<O: Into<ChoiceOption>>(
        kind: ChoiceKind,
        options: impl IntoIterator<Item = O>,
    ) -> Self {
        Self {
            kind,
            options: options_of(options),
            ..Self::default()
        }
    }

    /// The value chosen now.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// What it shows while nothing is chosen.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// The options are searched on the server: `href` is asked with the
    /// typed words as the query value `name`, once `min` characters are
    /// typed.
    #[must_use]
    pub fn search(mut self, href: impl Into<String>, name: impl Into<String>, min: u8) -> Self {
        self.search = Some(ChoiceSearch {
            href: href.into(),
            name: name.into(),
            min,
        });
        self
    }

    /// The person may add an option that is not there: `label` is what the
    /// client offers, and `href`, when there is one, is where the typed words
    /// are posted.
    #[must_use]
    pub fn create(mut self, label: impl Into<String>, href: Option<String>) -> Self {
        self.create = Some(ChoiceCreate {
            label: label.into(),
            href,
        });
        self
    }

    /// Where the next options are, by cursor.
    #[must_use]
    pub fn more(mut self, href: impl Into<String>) -> Self {
        self.more = Some(href.into());
        self
    }
}

impl ChoicesInput {
    /// Any of `options`. Each is a [`ChoiceOption`] or a value with its
    /// words.
    pub fn of<O: Into<ChoiceOption>>(options: impl IntoIterator<Item = O>) -> Self {
        Self {
            options: options_of(options),
            ..Self::default()
        }
    }

    /// The values chosen now.
    #[must_use]
    pub fn values<V: Into<String>>(mut self, values: impl IntoIterator<Item = V>) -> Self {
        self.values = values.into_iter().map(Into::into).collect();
        self
    }

    /// The options are searched on the server, as a choice of one's are.
    #[must_use]
    pub fn search(mut self, href: impl Into<String>, name: impl Into<String>, min: u8) -> Self {
        self.search = Some(ChoiceSearch {
            href: href.into(),
            name: name.into(),
            min,
        });
        self
    }

    /// The person may add an option that is not there, as in a choice of one.
    #[must_use]
    pub fn create(mut self, label: impl Into<String>, href: Option<String>) -> Self {
        self.create = Some(ChoiceCreate {
            label: label.into(),
            href,
        });
        self
    }

    /// Where the next options are, by cursor.
    #[must_use]
    pub fn more(mut self, href: impl Into<String>) -> Self {
        self.more = Some(href.into());
        self
    }
}

impl DateInput {
    /// A date: `2026-11-12`.
    pub fn date() -> Self {
        Self {
            kind: DateKind::Date,
            ..Self::default()
        }
    }

    /// A time of day: `09:30`.
    pub fn time() -> Self {
        Self {
            kind: DateKind::Time,
            ..Self::default()
        }
    }

    /// A date and a time: `2026-11-12T09:30`.
    pub fn datetime() -> Self {
        Self {
            kind: DateKind::Datetime,
            ..Self::default()
        }
    }

    /// What it holds now, in the form a date control sends.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// The earliest it takes.
    #[must_use]
    pub fn min(mut self, min: impl Into<String>) -> Self {
        self.min = Some(min.into());
        self
    }

    /// The latest it takes.
    #[must_use]
    pub fn max(mut self, max: impl Into<String>) -> Self {
        self.max = Some(max.into());
        self
    }

    /// Days that can't be picked, each `YYYY-MM-DD`.
    #[must_use]
    pub fn off_days<D: Into<String>>(mut self, days: impl IntoIterator<Item = D>) -> Self {
        self.off_days.extend(days.into_iter().map(Into::into));
        self
    }

    /// Days of the week that can't be picked.
    #[must_use]
    pub fn off_weekdays(mut self, days: impl IntoIterator<Item = Weekday>) -> Self {
        self.off_weekdays.extend(days);
        self
    }
}

impl RangeInput {
    /// Two dates picked as one, the start sent as `from` and the end as
    /// `to`.
    pub fn dates(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self::new(DateKind::Date, from, to)
    }

    /// Two ends of the kind `kind` picked as one, the start sent as `from`
    /// and the end as `to`.
    pub fn new(kind: DateKind, from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            kind,
            from: RangeEnd {
                name: from.into(),
                value: String::new(),
            },
            to: RangeEnd {
                name: to.into(),
                value: String::new(),
            },
            ..Self::default()
        }
    }

    /// What the two ends hold now.
    #[must_use]
    pub fn values(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.from.value = from.into();
        self.to.value = to.into();
        self
    }

    /// The earliest either end takes.
    #[must_use]
    pub fn min(mut self, min: impl Into<String>) -> Self {
        self.min = Some(min.into());
        self
    }

    /// The latest either end takes.
    #[must_use]
    pub fn max(mut self, max: impl Into<String>) -> Self {
        self.max = Some(max.into());
        self
    }

    /// Days that can't be either end, each `YYYY-MM-DD`.
    #[must_use]
    pub fn off_days<D: Into<String>>(mut self, days: impl IntoIterator<Item = D>) -> Self {
        self.off_days.extend(days.into_iter().map(Into::into));
        self
    }

    /// Days of the week that can't be either end.
    #[must_use]
    pub fn off_weekdays(mut self, days: impl IntoIterator<Item = Weekday>) -> Self {
        self.off_weekdays.extend(days);
        self
    }
}

impl SliderInput {
    /// A number from `min` to `max`, picked by dragging. It starts at `min`.
    pub fn between(min: impl ToString, max: impl ToString) -> Self {
        let min = min.to_string();
        Self {
            value: min.clone(),
            min,
            max: max.to_string(),
            step: None,
        }
    }

    /// The step between values.
    #[must_use]
    pub fn step(mut self, step: impl ToString) -> Self {
        self.step = Some(step.to_string());
        self
    }

    /// What it holds now.
    #[must_use]
    pub fn value(mut self, value: impl ToString) -> Self {
        self.value = value.to_string();
        self
    }
}

impl ColourInput {
    /// A colour, which starts as `value`: `#22c55e`.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

impl FileInput {
    /// Files from the device. A field of this kind makes its form a
    /// multipart post.
    pub fn new() -> Self {
        Self::default()
    }

    /// The kinds of file it takes, by the web's own list: `image/*,.pdf`.
    #[must_use]
    pub fn accept(mut self, kinds: impl Into<String>) -> Self {
        self.accept = Some(kinds.into());
        self
    }

    /// It takes more than one.
    #[must_use]
    pub fn multiple(mut self) -> Self {
        self.multiple = true;
        self
    }

    /// The camera it asks for, by the web's own names: `user`,
    /// `environment`.
    #[must_use]
    pub fn capture(mut self, camera: impl Into<String>) -> Self {
        self.capture = Some(camera.into());
        self
    }

    /// A photo keeps the place it was taken. Without this a client takes the
    /// place out of every photo before it is sent.
    #[must_use]
    pub fn keep_location(mut self) -> Self {
        self.keep_location = true;
        self
    }
}
