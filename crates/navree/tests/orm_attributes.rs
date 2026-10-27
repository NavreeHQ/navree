//! The ORM's attribute layer through the facade: mass assignment, casts,
//! accessors, visibility and serialization with `navree::prelude` alone
//! (the macros name `::navree::orm`).

use navree::prelude::*;
use serde::Serialize;

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(
    table = "badges",
    fillable(label, level),
    hidden(secret),
    appends(shout)
)]
struct Badge {
    id: i64,
    label: String,
    #[model(cast = "decimal(1)")]
    level: navree::orm::Decimal,
    #[model(set_with = trimmed)]
    secret: String,
}

fn trimmed(badge: &mut Badge, secret: String) {
    badge.secret = secret.trim().to_owned();
}

#[accessors]
impl Badge {
    #[accessor]
    fn shout(&self) -> String {
        self.label.to_uppercase()
    }
}

#[test]
fn the_attribute_layer_works_through_the_facade() {
    let mut badge = Record::new(Badge::default());
    let attributes: Attributes = attrs! { "label" => "gold", "level" => "2.25", "secret" => "x" };
    badge.fill(attributes).unwrap();
    assert_eq!(badge.label, "gold");
    assert_eq!(badge.secret, "", "not fillable");
    badge.set_secret("  shh ");
    assert_eq!(badge.secret, "shh");
    assert_eq!(
        badge.to_value().unwrap(),
        serde_json::json!({"id": 0, "label": "gold", "level": "2.3", "shout": "GOLD"})
    );
    assert_eq!(Badge::fillable(), ["label", "level"]);
}
