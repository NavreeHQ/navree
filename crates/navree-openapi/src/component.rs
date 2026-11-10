//! [`ComponentProperties`]: properties a module adds to a component schema.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::Schema;

/// Properties a module adds at run time to the values of one component
/// schema, so the OpenAPI document lists them too. navree-auth's user
/// resource extensions add fields to `UserResource` this way (an app's
/// `roles`, say).
///
/// A module contributes it (`ModuleBuilder::contribute`), and navree-api
/// [applies](Self::apply) every one to the document's components: each
/// property goes after the type's own, and one of the same name replaces the
/// type's, as an extension may overwrite a field. A document without the
/// component is left as it is.
#[derive(Clone, Debug)]
pub struct ComponentProperties {
    /// The component's name (`UserResource`).
    pub component: Cow<'static, str>,
    /// Each property's name and schema, and whether every value has it.
    pub properties: fn() -> Vec<(String, Schema, bool)>,
}

impl ComponentProperties {
    /// Adds the properties to the schema named [`component`](Self::component)
    /// in `components`, when there is one.
    pub fn apply(&self, components: &mut BTreeMap<String, Schema>) {
        let Some(schema) = components.get_mut(self.component.as_ref()) else {
            return;
        };
        for (name, property, required) in (self.properties)() {
            if required && !schema.required.contains(&name) {
                schema.required.push(name.clone());
            }
            schema.properties.insert(name, property);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn roles() -> Vec<(String, Schema, bool)> {
        vec![
            ("roles".to_owned(), Schema::array(Schema::string()), true),
            (
                "name".to_owned(),
                Schema::string().description("Shown."),
                false,
            ),
        ]
    }

    #[test]
    fn properties_join_the_component_and_replace_one_of_the_same_name() {
        let mut components = BTreeMap::from([(
            "UserResource".to_owned(),
            Schema::object()
                .property("id", Schema::string(), true)
                .property("name", Schema::string(), true),
        )]);
        let extension = ComponentProperties {
            component: Cow::Borrowed("UserResource"),
            properties: roles,
        };
        extension.apply(&mut components);
        let schema = serde_json::to_value(&components["UserResource"]).unwrap();
        assert_eq!(schema["required"], json!(["id", "name", "roles"]));
        assert_eq!(schema["properties"]["roles"]["items"]["type"], "string");
        assert_eq!(schema["properties"]["name"]["description"], "Shown.");
        let names: Vec<&String> = components["UserResource"].properties.keys().collect();
        assert_eq!(names, ["id", "name", "roles"]);
        // Applying again changes nothing.
        extension.apply(&mut components);
        assert_eq!(
            serde_json::to_value(&components["UserResource"]).unwrap(),
            schema
        );
    }

    #[test]
    fn a_document_without_the_component_is_left_alone() {
        let mut components = BTreeMap::from([("Deal".to_owned(), Schema::object())]);
        ComponentProperties {
            component: Cow::Borrowed("UserResource"),
            properties: roles,
        }
        .apply(&mut components);
        assert_eq!(components.len(), 1);
        assert!(components["Deal"].properties.is_empty());
    }
}
