//! [`PhoneInput`]: a phone number field with a country picker (feature
//! `phone`).
//!
//! With JavaScript off it is a `<select>` of countries and a `tel` input, and
//! the server reads the number in the chosen country. With the `phone`
//! module the select becomes a searchable list with flags and dial codes,
//! the number is formatted as it is typed, a pasted international number
//! picks its own country, and a hidden field carries the number in E.164.
//! The server stays the authority: it parses what was typed in the chosen
//! country, and writes the number back in that country's format.

use navree_phone::{PhoneNumber, Region};
use navree_validation::Phone;

use crate::components::icon;
use crate::icons;
use crate::node::{AttrValue, Element, Node, Uses, el};

/// The suffix of the field that posts the chosen country (`phone_country`).
pub const COUNTRY_SUFFIX: &str = "_country";

/// The suffix of the hidden field that posts the browser's reading of the
/// number in E.164 when the browser module runs (`phone_e164`). It is a hint
/// for code that reads the post without Navree: the server never reads it,
/// and it can be wrong (the browser checks lengths only) or forged.
pub const E164_SUFFIX: &str = "_e164";

/// What a [`PhoneInput`] says, for apps in other languages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhoneLabels {
    /// The country picker's name: `Country`.
    pub country: String,
    /// The select's empty choice, when no country is chosen: `Country`.
    pub choose: String,
    /// The group of countries listed first: `Suggested`.
    pub preferred: String,
    /// The search box in the list: `Search countries`.
    pub search: String,
    /// Shown when the search finds nothing: `No country found.`
    pub empty: String,
    /// Announced when a pasted or typed number picks a country:
    /// `Country set to {country}`.
    pub country_set: String,
    /// Announced as the search narrows the list: `{n} countries`.
    pub count: String,
}

impl Default for PhoneLabels {
    fn default() -> Self {
        Self {
            country: "Country".into(),
            choose: "Country".into(),
            preferred: "Suggested".into(),
            search: "Search countries".into(),
            empty: "No country found.".into(),
            country_set: "Country set to {country}".into(),
            count: "{n} countries".into(),
        }
    }
}

/// The flag of `region` as text: its two regional indicator symbols, which
/// systems with flag emoji draw as the flag. Pass your own drawing to
/// [`PhoneInput::flags`].
pub fn text_flag(region: Region) -> Node {
    let flag: String = region
        .as_str()
        .bytes()
        .filter_map(|letter| char::from_u32(0x1F1E6 + u32::from(letter.saturating_sub(b'A'))))
        .collect();
    Node::from(flag)
}

/// A phone number input with a country picker.
///
/// ```
/// use navree_phone::Region;
/// use navree_ui::{PhoneInput, render};
///
/// let html = render(&PhoneInput::new("phone").region(Region::parse("GB")).into());
/// assert!(html.contains(r#"name="phone_country""#));
/// assert!(html.contains(r#"<option value="GB" selected>United Kingdom +44</option>"#));
/// assert!(html.contains(r#"type="tel" name="phone""#));
/// assert!(html.contains(r#"placeholder="07400 123456""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct PhoneInput {
    el: Element,
    name: String,
    region: Option<Region>,
    text: Option<String>,
    number: Option<PhoneNumber>,
    placeholder: Option<String>,
    required: bool,
    disabled: bool,
    readonly: bool,
    autofocus: bool,
    invalid: bool,
    preferred: Vec<Region>,
    only: Vec<Region>,
    labels: PhoneLabels,
    flags: Option<fn(Region) -> Node>,
}

impl PhoneInput {
    /// An input posting the number as `name` and the country as
    /// `{name}_country`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            el: el("div").slot("phone-input"),
            name: name.into(),
            region: None,
            text: None,
            number: None,
            placeholder: None,
            required: false,
            disabled: false,
            readonly: false,
            autofocus: false,
            invalid: false,
            preferred: Vec::new(),
            only: Vec::new(),
            labels: PhoneLabels::default(),
            flags: None,
        }
    }

    /// The country chosen when the field has no number of its own: the
    /// workspace's or the user's.
    pub fn region(mut self, region: Option<Region>) -> Self {
        self.region = region;
        self
    }

    /// The country chosen when the field has no number of its own, by its
    /// code (`"GB"`); an unknown code chooses none.
    pub fn country(self, code: &str) -> Self {
        self.region(Region::parse(code))
    }

    /// Shows `number`, written the way its country writes it, with its
    /// country chosen.
    pub fn number(mut self, number: &PhoneNumber) -> Self {
        self.number = Some(*number);
        self
    }

    /// Shows `phone`, written the way its country writes it.
    pub fn phone(self, phone: &Phone) -> Self {
        self.number(phone.number())
    }

    /// Shows what was typed, as typed (a rejected form shows it again).
    /// Text that is a valid number in the chosen country is written in that
    /// country's format.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// The hint while empty. Without one, the chosen country's example
    /// mobile number.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Requires a number.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Shows the number and its country without letting them change; both
    /// still post.
    pub fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }

    /// Takes focus when the page loads.
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }

    /// Marks it as failing validation.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Countries listed first, above the rest.
    pub fn preferred(mut self, regions: impl IntoIterator<Item = Region>) -> Self {
        self.preferred = regions.into_iter().collect();
        self
    }

    /// Lists only these countries. It limits the list, not what can be
    /// typed: a number with another country's code still reads, so check the
    /// country on the server if it matters.
    pub fn countries(mut self, regions: impl IntoIterator<Item = Region>) -> Self {
        self.only = regions.into_iter().collect();
        self
    }

    /// Its words, for other languages.
    pub fn labels(mut self, labels: PhoneLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Draws each country's flag with `flag` (an SVG flag set) in place of
    /// the flag emoji. Every flag goes into the page once, in a template the
    /// list copies from.
    pub fn flags(mut self, flag: fn(Region) -> Node) -> Self {
        self.flags = Some(flag);
        self
    }

    /// Sets the root element's `id`.
    pub fn id(mut self, id: impl Into<AttrValue>) -> Self {
        self.el = self.el.id(id);
        self
    }

    /// Sets an attribute on the root element.
    pub fn attr(mut self, name: &'static str, value: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr(name, value);
        self
    }

    /// The number shown, the country chosen and the text in the box.
    fn resolve(&self) -> (Option<Region>, String, Option<PhoneNumber>) {
        if let Some(number) = self.number {
            let region = number.region().or(self.region);
            return (region, written(&number, region), Some(number));
        }
        let Some(text) = self.text.as_deref().filter(|text| !text.trim().is_empty()) else {
            return (self.region, String::new(), None);
        };
        match PhoneNumber::parse(text, self.region) {
            Ok(number) => {
                let region = number.region().or(self.region);
                (region, written(&number, region), Some(number))
            }
            Err(_) => (self.region, text.to_owned(), None),
        }
    }

    fn into_node(self) -> Node {
        let (region, text, number) = self.resolve();
        let flag = self.flags.unwrap_or(text_flag);
        let base = slug(&self.name);
        let list_id = format!("{base}-countries");
        let placeholder = self.placeholder.clone().or_else(|| {
            region
                .and_then(Region::placeholder_example)
                .map(|example| written(&example, region))
        });

        let mut select = el("select")
            .slot("phone-country")
            .attr("name", format!("{}{COUNTRY_SUFFIX}", self.name))
            .attr("aria-label", self.labels.country.clone())
            .attr("autocomplete", "tel-country-code")
            .flag("data-field-skip", true)
            .flag("disabled", self.disabled || self.readonly);
        let mut options = Vec::with_capacity(Region::all().count() + self.preferred.len() + 2);
        options.push(
            el("option")
                .attr("value", "")
                .flag("selected", region.is_none())
                .child(self.labels.choose.clone()),
        );
        let chosen = |candidate: Region| region == Some(candidate);
        let option = |candidate: Region, selected: bool| {
            el("option")
                .attr("value", candidate.as_str())
                .flag("selected", selected)
                .child(format!(
                    "{} +{}",
                    candidate.name(),
                    candidate.calling_code()
                ))
        };
        if !self.preferred.is_empty() {
            let preferred: Vec<Element> = self
                .preferred
                .iter()
                .map(|candidate| option(*candidate, chosen(*candidate)))
                .collect();
            options.push(
                el("optgroup")
                    .attr("label", self.labels.preferred.clone())
                    .child(preferred),
            );
        }
        let mut regions: Vec<Region> = if self.only.is_empty() {
            Region::all().collect()
        } else {
            self.only.clone()
        };
        regions.sort_by_cached_key(|region| sort_key(region.name()));
        let in_preferred = |candidate: &Region| self.preferred.contains(candidate);
        options.extend(
            regions.iter().map(|candidate| {
                option(*candidate, chosen(*candidate) && !in_preferred(candidate))
            }),
        );
        select = select.child(options);

        let trigger_label = match region {
            Some(region) => format!(
                "{}: {} +{}",
                self.labels.country,
                region.name(),
                region.calling_code()
            ),
            None => self.labels.choose.clone(),
        };
        let trigger = el("button")
            .slot("phone-country-trigger")
            .attr("type", "button")
            .attr("popovertarget", list_id.clone())
            .attr("aria-haspopup", "listbox")
            .attr("aria-expanded", "false")
            .attr("aria-label", trigger_label)
            .flag("data-field-skip", true)
            .flag("disabled", self.disabled || self.readonly)
            .children((
                el("span")
                    .slot("phone-flag")
                    .attr("aria-hidden", "true")
                    .child(region.map(flag)),
                el("span")
                    .slot("phone-dial")
                    .child(region.map(|region| format!("+{}", region.calling_code()))),
                icon(icons::CHEVRON_DOWN),
            ));
        let list = el("div")
            .slot("phone-countries")
            .attr("id", list_id)
            .attr("popover", "auto")
            .attr("data-side", "bottom")
            .attr("data-align", "start")
            .flag("data-ui-place", true)
            .flag("data-ui-keep", true)
            .attr("data-label-search", self.labels.search.clone())
            .attr("data-label-empty", self.labels.empty.clone())
            .attr("data-label-count", self.labels.count.clone())
            .attr(
                "data-preferred",
                self.preferred
                    .iter()
                    .map(|region| region.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            );

        let mut input = el("input")
            .slot("input")
            .attr("type", "tel")
            .attr("name", self.name.clone())
            .attr("autocomplete", "tel")
            .attr("inputmode", "tel")
            .attr("dir", "ltr")
            .attr("maxlength", "40")
            .flag("required", self.required)
            .flag("disabled", self.disabled)
            .flag("readonly", self.readonly)
            .flag("autofocus", self.autofocus);
        if !text.is_empty() {
            input = input.attr("value", text);
        }
        if let Some(placeholder) = placeholder {
            input = input.attr("placeholder", placeholder);
        }
        // The app's own hint stays when the country changes.
        input = input.flag("data-own-placeholder", self.placeholder.is_some());
        if self.invalid {
            input = input.attr("aria-invalid", "true");
        }
        let e164 = el("input")
            .slot("phone-e164")
            .attr("type", "hidden")
            .attr("name", format!("{}{E164_SUFFIX}", self.name))
            .attr(
                "value",
                number
                    .map(|number| number.as_str().to_owned())
                    .unwrap_or_default(),
            )
            .flag("disabled", true)
            .attr("data-ui-keep-attrs", "disabled value");
        let status = el("span")
            .slot("phone-status")
            .attr("role", "status")
            .attr("aria-live", "polite")
            .attr("data-label-country-set", self.labels.country_set.clone());
        let flag_set = self.flags.map(|flag| {
            el("template").slot("phone-flags").child(
                Region::all()
                    .map(|region| {
                        el("span")
                            .attr("data-flag", region.as_str())
                            .child(flag(region))
                    })
                    .collect::<Vec<_>>(),
            )
        });

        // A select that can't change posts nothing: the country goes along
        // in a hidden field.
        let fixed_country = (self.readonly && !self.disabled)
            .then(|| {
                region.map(|region| {
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", format!("{}{COUNTRY_SUFFIX}", self.name))
                        .attr("value", region.as_str())
                })
            })
            .flatten();
        self.el
            .flag("data-ui-phone", true)
            .attr("data-ui-keep-attrs", "data-enhanced")
            .flag("data-disabled", self.disabled)
            .flag("data-readonly", self.readonly)
            .uses(Uses::PHONE.with(Uses::OVERLAY))
            .children((
                select,
                fixed_country,
                trigger,
                list,
                input,
                e164,
                status,
                flag_set,
            ))
            .into()
    }
}

/// `number` as people write it: nationally when it is the chosen region's
/// and the national form reads back to the same number there, else
/// internationally. A few national forms name another number (Gabon's
/// `05 84 17 52` is `+24105841752`, not `+2415841752`), and the box's text is
/// what the form posts back.
fn written(number: &PhoneNumber, region: Option<Region>) -> String {
    if region.is_some() && number.region() == region {
        let national = number.national();
        if PhoneNumber::parse(&national, region).as_ref() == Ok(number) {
            return national;
        }
    }
    number.international()
}

/// A country name for sorting: lower case, with the accents of Latin
/// letters dropped, so `Åland Islands` sorts with the A's.
fn sort_key(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'À'..='Å' | 'à'..='å' => 'a',
            'Ç' | 'ç' => 'c',
            'È'..='Ë' | 'è'..='ë' => 'e',
            'Ì'..='Ï' | 'ì'..='ï' => 'i',
            'Ñ' | 'ñ' => 'n',
            'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' => 'o',
            'Ù'..='Ü' | 'ù'..='ü' => 'u',
            'Ý' | 'ý' | 'ÿ' => 'y',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// `name` as an id: letters, digits, `-` and `_`.
fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if slug.is_empty() {
        "phone".into()
    } else {
        slug
    }
}

impl From<PhoneInput> for Node {
    fn from(input: PhoneInput) -> Self {
        input.into_node()
    }
}
