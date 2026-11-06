//! Every problem with input that does not deserialize, at once: each value
//! serde cannot read, and the rules of the fields it can.
//!
//! serde stops at the first value it cannot read (`"size": "huge"` for a
//! number), so on its own it reports that one and no rule runs. Here the
//! value it stopped at gets a stand-in of the kind serde expected there (`0`
//! for a number, `""` for text, `null` for an option, the first of an enum's
//! variants, ...): a missing field gets one too, and an unknown field is
//! taken out. The input is read again, until it reads; then the rules run,
//! and what they say of the stand-ins is dropped, since the client never
//! sent them. When no stand-in gets serde past a value, what was found so far
//! is the answer.
//!
//! Each read costs a pass over the input, so this runs only on input under
//! the length the extractors read on the async worker (16 KiB: the caller
//! decides), for at most [`STOPS_AT_MOST`] values and [`READS_AT_MOST`] reads;
//! longer input reports the first value only, as serde does.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::Validate;
use crate::extract::check;
use crate::failure::{self, Failure, Rule};

/// The most values that get a stand-in.
pub(crate) const STOPS_AT_MOST: usize = 16;

/// The most times the input is read again.
pub(crate) const READS_AT_MOST: usize = 48;

/// One step into the input: a key of an object, or an index of an array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Key(String),
    Index(usize),
}

/// What serde stopped at: the failure, and where in the input it is, when
/// it can be told.
#[derive(Clone, Debug)]
pub(crate) struct Stop {
    pub(crate) failure: Failure,
    at: Option<Vec<Step>>,
    mend: Mend,
}

/// How the input is changed to get serde past a [`Stop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mend {
    /// The value there gets a stand-in.
    Replace,
    /// The missing field gets one.
    Insert,
    /// The unknown field is taken out.
    Remove,
}

impl Stop {
    /// The stop of a serde error with `path` and `message`.
    pub(crate) fn of(path: &serde_path_to_error::Path, message: &str) -> Self {
        let failure = failure::from_serde(path, message);
        let steps = steps(path);
        let (at, mend) = if let Some(field) = backticked(message, "missing field `") {
            (
                steps.map(|mut steps| {
                    steps.push(Step::Key(field.to_owned()));
                    steps
                }),
                Mend::Insert,
            )
        } else if let Some(field) = backticked(message, "unknown field `") {
            let at = steps.map(|mut steps| {
                if steps.last() != Some(&Step::Key(field.to_owned())) {
                    steps.push(Step::Key(field.to_owned()));
                }
                steps
            });
            (at, Mend::Remove)
        } else {
            (steps, Mend::Replace)
        };
        Self { failure, at, mend }
    }

    /// The failure alone.
    pub(crate) fn into_failure(self) -> Failure {
        self.failure
    }
}

fn steps(path: &serde_path_to_error::Path) -> Option<Vec<Step>> {
    use serde_path_to_error::Segment;
    path.iter()
        .map(|segment| match segment {
            Segment::Seq { index } => Some(Step::Index(*index)),
            Segment::Map { key } => Some(Step::Key(key.clone())),
            Segment::Enum { variant } => Some(Step::Key(variant.clone())),
            Segment::Unknown => None,
        })
        .collect()
}

fn backticked<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = message.strip_prefix(prefix)?;
    rest.split_once('`').map(|(field, _)| field)
}

/// The dotted path of `steps`, as failures name fields.
fn dotted(steps: &[Step]) -> String {
    steps
        .iter()
        .map(|step| match step {
            Step::Key(key) => key.clone(),
            Step::Index(index) => index.to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Whether the failure at `path` is about a value at or below `at`.
fn under(path: &str, at: &str) -> bool {
    path == at
        || path
            .strip_prefix(at)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// Input that can be read into a `T` and changed at a [`Stop`].
pub(crate) trait Mendable: Sized {
    /// The input with `stand_in` (or nothing, for [`Mend::Remove`]) at
    /// `at`; `None` when `at` is not in it.
    fn mended(&self, at: &[Step], mend: Mend, stand_in: &Value) -> Option<Self>;

    /// The input read into a `T`.
    fn read<T: DeserializeOwned>(&self) -> Result<T, Stop>;
}

/// Every failure of `input`, which serde stopped reading at `first`: one for
/// each value it cannot read, then those of the rules of `T` on the rest.
pub(crate) fn every_failure<T, I>(input: &I, first: Stop) -> Vec<Failure>
where
    T: DeserializeOwned + Validate,
    I: Mendable,
{
    let mut failures = Vec::new();
    // Where stand-ins are: what serde or the rules say there or below is
    // about them, not about the client's input.
    let mut mended: Vec<String> = Vec::new();
    let mut current: Option<I> = None;
    let mut reads = 0;
    let mut stop = first;
    'stops: loop {
        let place = stop.at.as_deref().map(dotted);
        let in_a_stand_in = place
            .as_deref()
            .is_some_and(|place| mended.iter().any(|stand_in| under(place, stand_in)));
        if !in_a_stand_in {
            failures.push(stop.failure.clone());
            if failures.len() >= STOPS_AT_MOST {
                return failures;
            }
        }
        let (Some(at), Some(place)) = (stop.at.clone().filter(|at| !at.is_empty()), place) else {
            return failures;
        };
        for stand_in in stand_ins(&stop) {
            if reads >= READS_AT_MOST {
                return failures;
            }
            let base = current.as_ref().unwrap_or(input);
            let Some(attempt) = base.mended(&at, stop.mend, &stand_in) else {
                return failures;
            };
            reads += 1;
            match attempt.read::<T>() {
                Ok(value) => {
                    mended.push(place);
                    if let Err(rules) = check(&value) {
                        failures.extend(rules.into_iter().filter(|failure| {
                            !mended.iter().any(|place| under(&failure.path, place))
                        }));
                    }
                    return failures;
                }
                // Still stopped at this value: the next stand-in.
                Err(next) if next.at.as_deref() == Some(at.as_slice()) => {}
                // Past it, or into the stand-in itself (an empty object's
                // missing fields, which get stand-ins of their own).
                Err(next) => {
                    mended.push(place);
                    current = Some(attempt);
                    stop = next;
                    continue 'stops;
                }
            }
        }
        return failures;
    }
}

/// What may stand in for the value of `stop`, most likely first: the kind
/// serde expected, then every other kind.
fn stand_ins(stop: &Stop) -> Vec<Value> {
    if stop.mend == Mend::Remove {
        return vec![Value::Null];
    }
    let nil = || Value::from("00000000-0000-0000-0000-000000000000");
    let dates = || {
        vec![
            Value::from("1970-01-01T00:00:00Z"),
            Value::from("1970-01-01"),
            Value::from("00:00:00"),
        ]
    };
    let mut first = match &stop.failure.rule {
        Rule::Keyed { key, params } => match key.as_str() {
            "integer" | "numeric" => vec![Value::from(0)],
            "boolean" => vec![Value::Bool(false)],
            "string" => vec![Value::from("")],
            "array" => vec![Value::Array(Vec::new())],
            "object" => vec![Value::Object(Map::new())],
            "uuid" => vec![nil()],
            "date" => dates(),
            "in" => params
                .iter()
                .find(|(name, _)| name == "values")
                .and_then(|(_, values)| values.split(", ").next())
                .filter(|variant| !variant.is_empty())
                .map(|variant| vec![Value::from(variant)])
                .unwrap_or_default(),
            _ => Vec::new(),
        },
        Rule::Text(_) => Vec::new(),
    };
    let every = [
        vec![
            Value::Null,
            Value::from(0),
            Value::from(""),
            Value::Bool(false),
            Value::Array(Vec::new()),
            Value::Object(Map::new()),
            nil(),
        ],
        dates(),
    ]
    .concat();
    for stand_in in every {
        if !first.contains(&stand_in) {
            first.push(stand_in);
        }
    }
    first
}

impl Mendable for Value {
    fn mended(&self, at: &[Step], mend: Mend, stand_in: &Value) -> Option<Self> {
        let (last, parents) = at.split_last()?;
        let mut mended = self.clone();
        let mut place = &mut mended;
        for step in parents {
            place = match step {
                Step::Key(key) => place.as_object_mut()?.get_mut(key)?,
                Step::Index(index) => place.as_array_mut()?.get_mut(*index)?,
            };
        }
        match (last, mend) {
            (Step::Key(key), Mend::Remove) => {
                place.as_object_mut()?.remove(key)?;
            }
            (Step::Key(key), Mend::Insert) => {
                place.as_object_mut()?.insert(key.clone(), stand_in.clone());
            }
            (Step::Key(key), Mend::Replace) => {
                *place.as_object_mut()?.get_mut(key)? = stand_in.clone();
            }
            (Step::Index(index), Mend::Replace) => {
                *place.as_array_mut()?.get_mut(*index)? = stand_in.clone();
            }
            (Step::Index(_), _) => return None,
        }
        Some(mended)
    }

    fn read<T: DeserializeOwned>(&self) -> Result<T, Stop> {
        failure::encoding(|| serde_path_to_error::deserialize::<_, T>(self))
            .map_err(|error| Stop::of(error.path(), &error.inner().to_string()))
    }
}

/// URL-encoded pairs (a query string or a form): flat, so a stop is at a
/// key, and every value is text.
#[derive(Clone, Debug)]
pub(crate) struct Pairs(Vec<(String, String)>);

impl Pairs {
    pub(crate) fn parse(input: &[u8]) -> Self {
        Self(
            form_urlencoded::parse(input)
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect(),
        )
    }

    fn encoded(&self) -> String {
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&self.0)
            .finish()
    }
}

impl Mendable for Pairs {
    fn mended(&self, at: &[Step], mend: Mend, stand_in: &Value) -> Option<Self> {
        let [Step::Key(key)] = at else {
            return None;
        };
        // A pair's value is text: `null` stands for no pair at all.
        let text = match stand_in {
            Value::Null => None,
            Value::String(text) => Some(text.clone()),
            Value::Bool(_) | Value::Number(_) => Some(stand_in.to_string()),
            Value::Array(_) | Value::Object(_) => return None,
        };
        let mut pairs: Vec<(String, String)> = self
            .0
            .iter()
            .filter(|(name, _)| name != key)
            .cloned()
            .collect();
        if mend != Mend::Remove
            && let Some(text) = text
        {
            pairs.push((key.clone(), text));
        }
        Some(Self(pairs))
    }

    fn read<T: DeserializeOwned>(&self) -> Result<T, Stop> {
        let encoded = self.encoded();
        let deserializer =
            serde_urlencoded::Deserializer::new(form_urlencoded::parse(encoded.as_bytes()));
        failure::encoding(|| serde_path_to_error::deserialize(deserializer))
            .map_err(|error| Stop::of(error.path(), &error.inner().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    // Fields serde reads and the rules do not.
    #[allow(dead_code)]
    #[derive(Debug, Deserialize, Validate)]
    struct Product {
        #[validate(length(min = 1))]
        name: String,
        #[validate(range(min = 1))]
        size: u32,
        #[validate(length(min = 1))]
        sku: Option<String>,
        #[validate(skip)]
        kind: Kind,
        #[validate(nested)]
        dimensions: Dimensions,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Kind {
        Saree,
        Kurta,
    }

    #[derive(Debug, Deserialize, Validate)]
    struct Dimensions {
        #[validate(range(min = 1))]
        width: u32,
    }

    fn keys(failures: &[Failure]) -> Vec<(String, String)> {
        failures
            .iter()
            .map(|failure| {
                let rule = match &failure.rule {
                    Rule::Keyed { key, .. } => key.clone(),
                    Rule::Text(text) => text.clone(),
                };
                (failure.path.clone(), rule)
            })
            .collect()
    }

    /// The failures, sorted: serde reads a JSON object's keys in an order
    /// of its own.
    fn sorted(failures: &[Failure]) -> Vec<(String, String)> {
        let mut keys = keys(failures);
        keys.sort();
        keys
    }

    fn json_failures(input: &Value) -> Vec<(String, String)> {
        let stop = input.read::<Product>().unwrap_err();
        sorted(&every_failure::<Product, _>(input, stop))
    }

    fn pairs(text: &str) -> Vec<(String, String)> {
        let input = Pairs::parse(text.as_bytes());
        let stop = input.read::<Flat>().unwrap_err();
        sorted(&every_failure::<Flat, _>(&input, stop))
    }

    #[allow(dead_code)]
    #[derive(Debug, Deserialize, Validate)]
    struct Flat {
        #[validate(length(min = 1))]
        name: String,
        #[validate(range(min = 1))]
        size: u32,
        #[validate(range(min = 1))]
        count: Option<u32>,
        #[validate(skip)]
        active: bool,
    }

    fn pair(path: &str, rule: &str) -> (String, String) {
        (path.to_owned(), rule.to_owned())
    }

    #[test]
    fn a_wrong_type_and_a_failed_rule_are_reported_together() {
        let failures = json_failures(&json!({
            "name": "",
            "size": "huge",
            "kind": "saree",
            "dimensions": { "width": 3 },
        }));
        assert_eq!(
            failures,
            [pair("name", "min.string"), pair("size", "integer")]
        );
    }

    #[test]
    fn every_value_that_cannot_be_read_is_reported_once() {
        let failures = json_failures(&json!({
            "name": 7,
            "size": "huge",
            "sku": 3,
            "kind": "lehenga",
            "dimensions": { "width": "wide" },
        }));
        assert_eq!(
            failures,
            [
                pair("dimensions.width", "integer"),
                pair("kind", "in"),
                pair("name", "string"),
                pair("size", "integer"),
                pair("sku", "string"),
            ]
        );
    }

    #[test]
    fn missing_fields_are_required_and_the_rest_is_still_checked() {
        let failures = json_failures(&json!({
            "name": "",
            "kind": "kurta",
            "dimensions": {},
        }));
        assert_eq!(
            failures,
            [
                pair("dimensions.width", "required"),
                pair("name", "min.string"),
                pair("size", "required"),
            ]
        );
    }

    #[test]
    fn what_is_wrong_with_a_stand_in_is_no_failure_of_the_input() {
        // Text where an object goes: `{}` stands in, its own missing `width`
        // gets `0`, and the `range` failure of that `0` is dropped.
        let failures = json_failures(&json!({
            "name": "Phulkari dupatta",
            "size": { "value": 3 },
            "kind": "saree",
            "dimensions": "large",
        }));
        assert_eq!(
            failures,
            [pair("dimensions", "object"), pair("size", "integer")]
        );
    }

    #[test]
    fn a_body_that_is_not_an_object_is_one_failure() {
        assert_eq!(json_failures(&json!([1, 2])).len(), 1);
    }

    #[test]
    fn the_stops_are_bounded() {
        #[allow(dead_code)]
        #[derive(Debug, Deserialize, Validate)]
        struct Many {
            #[validate(skip)]
            values: Vec<u32>,
        }
        let input = json!({ "values": vec!["x"; 100] });
        let stop = input.read::<Many>().unwrap_err();
        let failures = every_failure::<Many, _>(&input, stop);
        assert_eq!(failures.len(), STOPS_AT_MOST);
        assert!(
            failures
                .iter()
                .all(|failure| failure.path.starts_with("values."))
        );
    }

    #[test]
    fn a_form_reports_its_wrong_values_with_its_rules() {
        assert_eq!(
            pairs("name=&size=huge&count=lots&active=maybe"),
            [
                pair("active", "boolean"),
                pair("count", "integer"),
                pair("name", "min.string"),
                pair("size", "integer"),
            ]
        );
        assert_eq!(
            pairs("size=0&active=true"),
            [pair("name", "required"), pair("size", "min.numeric")]
        );
    }
}
