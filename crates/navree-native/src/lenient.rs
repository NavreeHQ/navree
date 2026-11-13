//! Reading a closed list from a newer server.
//!
//! A role, a tone or a text style is one word of a closed list. A server newer
//! than the client may send a word the client's list does not have. That is
//! not a reason to refuse the whole answer: a field that holds such a word is
//! read as the field's default, as a field the server left out, which is what
//! the generated Kotlin and Swift models do. In a list of such words, the ones
//! this build does not know are left out.
//!
//! Only the word is forgiven. A value of another shape (a number where a word
//! belongs) is still an error.

use std::fmt;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::value::{Error as Plain, StrDeserializer};
use serde::de::{DeserializeOwned, Deserializer, Error, IntoDeserializer, Visitor};

/// Reads one word: the value it names, or nothing for a word the list lacks.
struct Word<T>(PhantomData<T>);

impl<'de, T: DeserializeOwned> Visitor<'de> for Word<T> {
    type Value = Option<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a word of a closed list")
    }

    fn visit_str<E: Error>(self, word: &str) -> Result<Self::Value, E> {
        let word: StrDeserializer<'_, Plain> = word.into_deserializer();
        Ok(T::deserialize(word).ok())
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

/// A word that may be one this build does not know.
struct Maybe<T>(Option<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for Maybe<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Word(PhantomData)).map(Self)
    }
}

/// A field that holds a word of a closed list: a word this build does not
/// know is read as the list's default.
pub(crate) fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    Maybe::deserialize(deserializer).map(|word| word.0.unwrap_or_default())
}

/// A field that may hold a word of a closed list: a word this build does not
/// know is read as none.
pub(crate) fn or_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Maybe::deserialize(deserializer).map(|word| word.0)
}

/// A field that holds several words of a closed list: the ones this build
/// does not know are left out.
pub(crate) fn known<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let words = Vec::<Maybe<T>>::deserialize(deserializer)?;
    Ok(words.into_iter().filter_map(|word| word.0).collect())
}

#[cfg(test)]
mod tests {
    use crate::action::Present;
    use crate::input::Input;
    use crate::node::{Node, Role, TextStyle, Tone};
    use crate::{Action, Envelope, Op};

    #[test]
    fn a_word_a_closed_list_lacks_is_the_field_s_default() {
        let node: Node = serde_json::from_str(
            r#"{"t":"button","label":"Share","role":"share","prominence":"loudest","action":{"t":"navigate","href":"/x","present":"hologram"}}"#,
        )
        .unwrap();
        let Node::Button(button) = node else {
            panic!("not a button");
        };
        assert_eq!(button.role, Role::None);
        assert_eq!(button.prominence, crate::Prominence::Medium);
        assert_eq!(
            button.action,
            Some(Action::Navigate {
                href: "/x".to_owned(),
                present: Present::Auto
            })
        );

        // A field that may hold a word holds none; a word the list has is read as ever.
        let node: Node = serde_json::from_str(
            r#"{"t":"text","value":"Paid","tone":"celebration","style":"marquee"}"#,
        )
        .unwrap();
        let Node::Text(text) = node else {
            panic!("not a text");
        };
        assert_eq!((text.tone, text.style), (None, TextStyle::Body));
        let node: Node =
            serde_json::from_str(r#"{"t":"text","value":"Paid","tone":"success","style":"title"}"#)
                .unwrap();
        let Node::Text(text) = node else {
            panic!("not a text");
        };
        assert_eq!(
            (text.tone, text.style),
            (Some(Tone::Success), TextStyle::Title)
        );
        // `null` is a field left out.
        assert!(serde_json::from_str::<Node>(r#"{"t":"badge","text":"x","tone":null}"#).is_ok());
    }

    #[test]
    fn the_rest_of_the_answer_is_read() {
        let envelope: Envelope = serde_json::from_str(
            r#"{"navree":1,"build":"b","vocab":0,"ops":[
                {"op":"toast","tone":"festive","text":"Saved"},
                {"op":"url","href":"/x","mode":"teleport"},
                {"op":"morph","target":"f","node":{"t":"field","name":"d","input":{"t":"date","kind":"fortnight","off_weekdays":["sunday","funday"]}}}]}"#,
        )
        .unwrap();
        let Op::Toast(toast) = &envelope.ops[0] else {
            panic!("not a toast");
        };
        assert_eq!(toast.tone, Tone::Neutral);
        let Op::Morph {
            node: Node::Field(field),
            ..
        } = &envelope.ops[2]
        else {
            panic!("not a field");
        };
        let Input::Date(date) = &field.input else {
            panic!("not a date");
        };
        // The words of a list that this build does not know are left out.
        assert_eq!(date.off_weekdays.len(), 1);
    }

    #[test]
    fn a_value_of_another_shape_is_still_refused() {
        assert!(serde_json::from_str::<Node>(r#"{"t":"badge","text":"x","tone":7}"#).is_err());
        assert!(
            serde_json::from_str::<Node>(r#"{"t":"badge","text":"x","tone":{"t":"success"}}"#)
                .is_err()
        );
    }
}
