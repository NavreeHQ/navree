//! Validation outside a request: [`validate_json`], with the same messages
//! as the extractors (for input that comes from elsewhere, such as a
//! language model's tool arguments). [`validate`](crate::validate) checks a
//! value that is already deserialized.

use garde::Validate;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::extract::{check, from_json};
use crate::failure;
use crate::{Messages, ValidationErrors};

/// The field name of failures about the input as a whole (`input must be
/// an object`).
const ROOT: &str = "input";

/// Deserializes `input` into `T` and runs `T`'s garde rules, reporting
/// failures exactly as [`Valid<Json<T>>`](crate::Valid) does (`"subject
/// should not be empty"`, `"kind must be one of the following values: note,
/// call"`), rendered with `messages`. Failures about the input as a whole
/// are reported under the field `input`. Values that read the
/// [`InputContext`](crate::InputContext) (a `Phone` written without a
/// country code) get the defaults, unless the call runs in an
/// [`InputContext::scope`](crate::InputContext::scope).
///
/// ```
/// use garde::Validate;
/// use navree_validation::{Messages, validate_json};
/// use serde::Deserialize;
/// use serde_json::json;
///
/// #[derive(Debug, Deserialize, Validate)]
/// struct LogCall {
///     #[garde(length(chars, min = 1, max = 255))]
///     subject: String,
/// }
///
/// let errors = validate_json::<LogCall>(&json!({}), &Messages::default()).unwrap_err();
/// assert_eq!(errors.get("subject").unwrap()[0], "subject should not be empty");
///
/// let errors = validate_json::<LogCall>(&json!({ "subject": "" }), &Messages::default())
///     .unwrap_err();
/// assert_eq!(
///     errors.get("subject").unwrap()[0],
///     "subject must be longer than or equal to 1 characters"
/// );
///
/// let call = validate_json::<LogCall>(&json!({ "subject": "Call Dana" }), &Messages::default())
///     .unwrap();
/// assert_eq!(call.subject, "Call Dana");
/// ```
pub fn validate_json<T>(input: &Value, messages: &Messages) -> Result<T, ValidationErrors>
where
    T: DeserializeOwned + Validate<Context = ()>,
{
    let value = match from_json::<T>(input) {
        Ok(value) => value,
        Err(failure) => {
            return Err(failure::render(
                &[failure],
                messages,
                None,
                Some(input),
                ROOT,
            ));
        }
    };
    match check(&value) {
        Ok(()) => Ok(value),
        Err(failures) => Err(failure::render(
            &failures,
            messages,
            None,
            Some(input),
            ROOT,
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Debug, Deserialize, Validate)]
    struct Args {
        #[garde(length(chars, min = 1, max = 5))]
        subject: String,
        #[garde(skip)]
        kind: Kind,
        #[garde(range(min = 1))]
        #[serde(default = "one")]
        count: u32,
    }

    fn one() -> u32 {
        1
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Kind {
        Note,
        Call,
    }

    #[test]
    fn valid_input_deserializes() {
        let args: Args = validate_json(
            &json!({ "subject": "hi", "kind": "call" }),
            &Messages::default(),
        )
        .unwrap();
        assert_eq!(args.subject, "hi");
        assert!(matches!(args.kind, Kind::Call));
        assert_eq!(args.count, 1);
    }

    #[test]
    fn rule_failures_use_the_api_messages() {
        let errors = validate_json::<Args>(
            &json!({ "subject": "", "kind": "note", "count": 0 }),
            &Messages::default(),
        )
        .unwrap_err();
        assert_eq!(
            errors.get("subject").unwrap(),
            ["subject must be longer than or equal to 1 characters"]
        );
        assert_eq!(
            errors.get("count").unwrap(),
            ["count must not be less than 1"]
        );
    }

    #[test]
    fn serde_failures_name_the_field() {
        let errors = validate_json::<Args>(
            &json!({ "subject": "x", "kind": "email" }),
            &Messages::default(),
        )
        .unwrap_err();
        assert_eq!(
            errors.get("kind").unwrap(),
            ["kind must be one of the following values: note, call"]
        );
        let errors =
            validate_json::<Args>(&json!({ "kind": "note" }), &Messages::default()).unwrap_err();
        assert_eq!(
            errors.get("subject").unwrap(),
            ["subject should not be empty"]
        );
    }

    #[test]
    fn the_whole_input_is_named_input() {
        let errors = validate_json::<Args>(&json!("text"), &Messages::default()).unwrap_err();
        assert!(errors.get("input").is_some(), "{errors:?}");
    }

    #[test]
    fn validate_checks_a_value() {
        let args = Args {
            subject: "toolong".into(),
            kind: Kind::Note,
            count: 1,
        };
        let errors = crate::validate(&args, None, &Messages::default(), ROOT).unwrap_err();
        assert_eq!(
            errors.get("subject").unwrap(),
            ["subject must be shorter than or equal to 5 characters"]
        );
        let messages = Messages::default().with_attribute("subject", "the subject");
        let errors = crate::validate(&args, None, &messages, ROOT).unwrap_err();
        assert!(errors.get("subject").unwrap()[0].starts_with("the subject"));
    }
}
