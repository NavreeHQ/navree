//! [`Pricing`]: plans side by side, with a monthly or yearly switch that needs no script.

use std::borrow::Cow;

use crate::components::icon;
use crate::icons;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// What a plan costs in one billing period: the amount as the reader reads it (`₹299`)
/// and the period after it (`a month`), and a line under them.
#[derive(Clone, Debug)]
struct Price {
    amount: Node,
    period: Option<Node>,
    note: Option<Node>,
}

/// One plan of a [`Pricing`] table: its name (`h3`), who it is for, its price monthly and
/// yearly or once for both, what it includes and its call to action.
///
/// Write amounts as the reader reads them, in their grouping: `₹2,990`, `₹1,00,000`.
///
/// ```
/// use navree_ui::marketing::Plan;
/// use navree_ui::Button;
///
/// let plus = Plan::new("Plus")
///     .description("For one person who wants the whole picture.")
///     .monthly("₹299", "a month")
///     .yearly("₹2,990", "a year")
///     .yearly_note("₹249 a month, billed once a year")
///     .feature("Trends over a year")
///     .feature("Export to your doctor as a PDF")
///     .action(Button::primary("Start Plus").href("/signup?plan=plus"))
///     .featured("Most popular");
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Plan {
    el: Element,
    name: Node,
    description: Option<Node>,
    once: Option<Price>,
    monthly: Option<Price>,
    yearly: Option<Price>,
    features: Vec<Node>,
    action: Option<Node>,
    yearly_action: Option<Node>,
    badge: Option<Node>,
}

impl Plan {
    /// A plan called `name`.
    pub fn new(name: impl Into<Node>) -> Self {
        Self {
            el: el("li").slot("plan"),
            name: name.into(),
            description: None,
            once: None,
            monthly: None,
            yearly: None,
            features: Vec::new(),
            action: None,
            yearly_action: None,
            badge: None,
        }
    }

    /// Who it is for, in a line.
    pub fn description(mut self, text: impl Into<Node>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// One price whatever the period: `₹0`, `for ever`; or `Talk to us`, with no period.
    pub fn price(mut self, amount: impl Into<Node>, period: impl Into<Node>) -> Self {
        self.once = Some(Price {
            amount: amount.into(),
            period: Some(period.into()),
            note: None,
        });
        self
    }

    /// The price when billed monthly.
    pub fn monthly(mut self, amount: impl Into<Node>, period: impl Into<Node>) -> Self {
        self.monthly = Some(Price {
            amount: amount.into(),
            period: Some(period.into()),
            note: self.monthly.and_then(|price| price.note),
        });
        self
    }

    /// The price when billed yearly.
    pub fn yearly(mut self, amount: impl Into<Node>, period: impl Into<Node>) -> Self {
        self.yearly = Some(Price {
            amount: amount.into(),
            period: Some(period.into()),
            note: self.yearly.and_then(|price| price.note),
        });
        self
    }

    /// A line under the monthly price.
    pub fn monthly_note(mut self, note: impl Into<Node>) -> Self {
        if let Some(price) = &mut self.monthly {
            price.note = Some(note.into());
        }
        self
    }

    /// A line under the yearly price: what it comes to a month.
    pub fn yearly_note(mut self, note: impl Into<Node>) -> Self {
        if let Some(price) = &mut self.yearly {
            price.note = Some(note.into());
        }
        self
    }

    /// Adds a thing the plan includes, with a tick.
    pub fn feature(mut self, feature: impl Into<Node>) -> Self {
        self.features.push(feature.into());
        self
    }

    /// Adds things the plan includes.
    pub fn features<T: Into<Node>>(mut self, features: impl IntoIterator<Item = T>) -> Self {
        self.features.extend(features.into_iter().map(Into::into));
        self
    }

    /// Its call to action: a [`Button`](crate::Button) with an `href`.
    pub fn action(mut self, action: impl Into<Node>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// Its call to action while yearly billing is chosen, when it differs: a link that
    /// carries the period.
    pub fn yearly_action(mut self, action: impl Into<Node>) -> Self {
        self.yearly_action = Some(action.into());
        self
    }

    /// Sets it apart from the others, with `badge` beside its name: "Most popular".
    pub fn featured(mut self, badge: impl Into<Node>) -> Self {
        self.badge = Some(badge.into());
        self.el = self.el.flag("data-featured", true);
        self
    }

    /// Whether it has a monthly and a yearly price.
    fn has_cycles(&self) -> bool {
        self.monthly.is_some() || self.yearly.is_some()
    }

    fn price_node(price: Price, cycle: Option<&'static str>) -> Element {
        el("div")
            .slot("plan-price")
            .attr_opt("data-cycle", cycle)
            .children((
                el("p").slot("plan-amount").children((
                    el("span").slot("plan-figure").child(price.amount),
                    price
                        .period
                        .map(|period| el("span").slot("plan-period").child(period)),
                )),
                price.note.map(|note| el("p").slot("plan-note").child(note)),
            ))
    }

    fn into_node(self) -> Node {
        let prices: Vec<Element> = match self.once {
            Some(once) => vec![Self::price_node(once, None)],
            None => [
                self.monthly
                    .map(|price| Self::price_node(price, Some("monthly"))),
                self.yearly
                    .map(|price| Self::price_node(price, Some("yearly"))),
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        let action = match (self.action, self.yearly_action) {
            (Some(monthly), Some(yearly)) => Some(Node::list((
                el("div")
                    .slot("plan-action")
                    .attr("data-cycle", "monthly")
                    .child(monthly),
                el("div")
                    .slot("plan-action")
                    .attr("data-cycle", "yearly")
                    .child(yearly),
            ))),
            (Some(one), None) | (None, Some(one)) => {
                Some(el("div").slot("plan-action").child(one).into())
            }
            (None, None) => None,
        };
        let features = (!self.features.is_empty()).then(|| {
            el("ul").slot("plan-features").child(
                self.features
                    .into_iter()
                    .map(|feature| el("li").children((icon(icons::CHECK), feature)))
                    .collect::<Vec<_>>(),
            )
        });
        self.el
            .children((
                el("div").slot("plan-head").children((
                    el("h3").slot("plan-name").child(self.name),
                    self.badge
                        .map(|badge| el("span").slot("plan-badge").child(badge)),
                )),
                self.description
                    .map(|text| el("p").slot("plan-description").child(text)),
                prices,
                action,
                features,
            ))
            .into()
    }
}

/// Plans side by side, with a switch between monthly and yearly prices when a plan has
/// both. The switch is a pair of radio buttons and the stylesheet shows the prices of the
/// one chosen, so it works with no script, by keyboard (the arrow keys move between the
/// two) and by touch, and the server can draw either period first.
///
/// ```
/// use navree_ui::marketing::{Plan, Pricing};
/// use navree_ui::render;
///
/// let pricing = Pricing::new("plans")
///     .yearly_badge("2 months free")
///     .plan(Plan::new("Free").price("₹0", "for ever"))
///     .plan(Plan::new("Plus").monthly("₹299", "a month").yearly("₹2,990", "a year"));
/// let html = render(&pricing.into());
/// assert!(html.contains(r#"<input type="radio" name="plans-cycle" value="monthly" checked>"#));
/// assert!(html.contains(r#"data-cycle="yearly""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Pricing {
    el: Element,
    id: String,
    plans: Vec<Plan>,
    yearly_first: bool,
    legend: Cow<'static, str>,
    monthly_label: Cow<'static, str>,
    yearly_label: Cow<'static, str>,
    yearly_badge: Option<Node>,
    footnote: Option<Node>,
}

impl Pricing {
    /// A pricing table named `id`: the switch's radio buttons are named `{id}-cycle`.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            el: el("div").slot("pricing").uses(Uses::MARKETING),
            id: id.into(),
            plans: Vec::new(),
            yearly_first: false,
            legend: Cow::Borrowed("Billing period"),
            monthly_label: Cow::Borrowed("Monthly"),
            yearly_label: Cow::Borrowed("Yearly"),
            yearly_badge: None,
            footnote: None,
        }
    }

    /// Adds a plan.
    pub fn plan(mut self, plan: Plan) -> Self {
        self.plans.push(plan);
        self
    }

    /// Shows the yearly prices first: for a link that carries `?billing=yearly`.
    pub fn yearly(mut self) -> Self {
        self.yearly_first = true;
        self
    }

    /// The switch's words, in the page's language: its name for assistive technology and
    /// its two choices.
    pub fn labels(
        mut self,
        legend: impl Into<Cow<'static, str>>,
        monthly: impl Into<Cow<'static, str>>,
        yearly: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.legend = legend.into();
        self.monthly_label = monthly.into();
        self.yearly_label = yearly.into();
        self
    }

    /// A short line beside the yearly choice: "2 months free".
    pub fn yearly_badge(mut self, badge: impl Into<Node>) -> Self {
        self.yearly_badge = Some(badge.into());
        self
    }

    /// A line under the plans: taxes, the currency, how to cancel.
    pub fn footnote(mut self, note: impl IntoNodes) -> Self {
        let mut nodes = Vec::new();
        note.push_into(&mut nodes);
        self.footnote = Some(Node::list(nodes));
        self
    }

    fn into_node(self) -> Node {
        let cycles = self.plans.iter().any(Plan::has_cycles);
        let name = format!("{}-cycle", self.id);
        let choice =
            |value: &'static str, label: Cow<'static, str>, checked: bool, badge: Option<Node>| {
                el("label").slot("pricing-choice").children((
                    el("input")
                        .attr("type", "radio")
                        .attr("name", name.clone())
                        .attr("value", value)
                        .flag("checked", checked),
                    el("span").child(label),
                    badge.map(|badge| el("span").slot("pricing-save").child(badge)),
                ))
            };
        let switch = cycles.then(|| {
            el("fieldset").slot("pricing-cycle").children((
                el("legend").flag("data-ui-sr", true).child(self.legend),
                choice("monthly", self.monthly_label, !self.yearly_first, None),
                choice(
                    "yearly",
                    self.yearly_label,
                    self.yearly_first,
                    self.yearly_badge,
                ),
            ))
        });
        let count = self.plans.len().clamp(1, 4).to_string();
        self.el
            .id(self.id)
            .children((
                switch,
                el("ul")
                    .slot("pricing-plans")
                    .attr("data-count", count)
                    .child(self.plans.into_iter().map(Node::from).collect::<Vec<_>>()),
                self.footnote
                    .map(|note| el("p").slot("pricing-footnote").child(note)),
            ))
            .into()
    }
}

into_node!(Plan, Pricing);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Button, render};

    #[test]
    fn a_table_with_one_price_per_plan_has_no_switch() {
        let html = render(
            &Pricing::new("p")
                .plan(Plan::new("Free").price("₹0", "for ever"))
                .into(),
        );
        assert!(!html.contains("<fieldset"), "{html}");
        assert!(!html.contains("data-cycle"), "{html}");
    }

    #[test]
    fn the_switch_starts_on_the_period_asked_for() {
        let html = render(
            &Pricing::new("p")
                .yearly()
                .labels("बिलिंग", "मासिक", "वार्षिक")
                .plan(
                    Plan::new("Plus")
                        .monthly("₹299", "a month")
                        .yearly("₹2,990", "a year"),
                )
                .into(),
        );
        assert!(html.contains(r#"value="yearly" checked"#), "{html}");
        assert!(!html.contains(r#"value="monthly" checked"#), "{html}");
        assert!(html.contains("<legend data-ui-sr>बिलिंग</legend>"), "{html}");
    }

    #[test]
    fn a_plan_can_link_each_period_to_its_own_page() {
        let html = render(
            &Plan::new("Plus")
                .monthly("₹299", "a month")
                .monthly_note("Cancel any time")
                .yearly("₹2,990", "a year")
                .yearly_note("₹249 a month")
                .action(Button::primary("Start").href("/s?b=m"))
                .yearly_action(Button::primary("Start").href("/s?b=y"))
                .featured("Most popular")
                .into(),
        );
        assert!(html.contains("data-featured"), "{html}");
        assert!(
            html.contains("Cancel any time") && html.contains("₹249 a month"),
            "{html}"
        );
        let monthly = html
            .find(r#"data-slot="plan-action" data-cycle="monthly""#)
            .unwrap();
        let yearly = html
            .find(r#"data-slot="plan-action" data-cycle="yearly""#)
            .unwrap();
        assert!(monthly < yearly, "{html}");
        assert!(
            html.contains("<h3 data-slot=\"plan-name\">Plus</h3>"),
            "{html}"
        );
    }
}
