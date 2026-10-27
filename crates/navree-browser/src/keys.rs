//! Keyboard events: named keys, characters and combinations such as
//! `Control+K`, as `Input.dispatchKeyEvent` parameters.

use serde_json::{Value, json};

/// One key on a US keyboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    pub(crate) key: String,
    pub(crate) code: String,
    pub(crate) key_code: u32,
    /// The text the key types (none for Escape, arrows and modifiers).
    pub(crate) text: Option<String>,
    /// The modifier bit when the key is a modifier (Alt 1, Control 2,
    /// Meta 4, Shift 8).
    pub(crate) modifier: u32,
}

fn named(key: &str, code: &str, key_code: u32, text: Option<&str>, modifier: u32) -> Key {
    Key {
        key: key.to_owned(),
        code: code.to_owned(),
        key_code,
        text: text.map(str::to_owned),
        modifier,
    }
}

/// The key named `name` (`Enter`, `Escape`, `ArrowDown`, `F5`, `Control`,
/// `a`, `7`, `/`, ...).
pub(crate) fn key(name: &str) -> Option<Key> {
    let k = match name {
        "Enter" | "Return" => named("Enter", "Enter", 13, Some("\r"), 0),
        "Tab" => named("Tab", "Tab", 9, None, 0),
        "Escape" | "Esc" => named("Escape", "Escape", 27, None, 0),
        "Backspace" => named("Backspace", "Backspace", 8, None, 0),
        "Delete" | "Del" => named("Delete", "Delete", 46, None, 0),
        "Space" | " " => named(" ", "Space", 32, Some(" "), 0),
        "ArrowUp" | "Up" => named("ArrowUp", "ArrowUp", 38, None, 0),
        "ArrowDown" | "Down" => named("ArrowDown", "ArrowDown", 40, None, 0),
        "ArrowLeft" | "Left" => named("ArrowLeft", "ArrowLeft", 37, None, 0),
        "ArrowRight" | "Right" => named("ArrowRight", "ArrowRight", 39, None, 0),
        "Home" => named("Home", "Home", 36, None, 0),
        "End" => named("End", "End", 35, None, 0),
        "PageUp" => named("PageUp", "PageUp", 33, None, 0),
        "PageDown" => named("PageDown", "PageDown", 34, None, 0),
        "Insert" => named("Insert", "Insert", 45, None, 0),
        "Alt" | "Option" => named("Alt", "AltLeft", 18, None, 1),
        "Control" | "Ctrl" => named("Control", "ControlLeft", 17, None, 2),
        "Meta" | "Cmd" | "Command" | "Super" => named("Meta", "MetaLeft", 91, None, 4),
        "Shift" => named("Shift", "ShiftLeft", 16, None, 8),
        _ => {
            if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok())
                && (1..=12).contains(&n)
            {
                return Some(named(name, name, 111 + n, None, 0));
            }
            let mut chars = name.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            return character(c);
        }
    };
    Some(k)
}

/// The key that types `c` on a US keyboard (`None` for characters it has
/// no key for, which are inserted as text instead).
pub(crate) fn character(c: char) -> Option<Key> {
    let text = Some(c.to_string());
    let (code, key_code) = match c {
        'a'..='z' => (
            format!("Key{}", c.to_ascii_uppercase()),
            c.to_ascii_uppercase() as u32,
        ),
        'A'..='Z' => (format!("Key{c}"), c as u32),
        '0'..='9' => (format!("Digit{c}"), c as u32),
        ' ' => ("Space".to_owned(), 32),
        '\n' | '\r' => return key("Enter"),
        '\t' => return key("Tab"),
        _ => {
            let (code, key_code) = match c {
                '!' => ("Digit1", 49),
                '@' => ("Digit2", 50),
                '#' => ("Digit3", 51),
                '$' => ("Digit4", 52),
                '%' => ("Digit5", 53),
                '^' => ("Digit6", 54),
                '&' => ("Digit7", 55),
                '*' => ("Digit8", 56),
                '(' => ("Digit9", 57),
                ')' => ("Digit0", 48),
                '-' | '_' => ("Minus", 189),
                '=' | '+' => ("Equal", 187),
                '[' | '{' => ("BracketLeft", 219),
                ']' | '}' => ("BracketRight", 221),
                '\\' | '|' => ("Backslash", 220),
                ';' | ':' => ("Semicolon", 186),
                '\'' | '"' => ("Quote", 222),
                ',' | '<' => ("Comma", 188),
                '.' | '>' => ("Period", 190),
                '/' | '?' => ("Slash", 191),
                '`' | '~' => ("Backquote", 192),
                _ => return None,
            };
            (code.to_owned(), key_code)
        }
    };
    Some(Key {
        key: c.to_string(),
        code,
        key_code,
        text,
        modifier: 0,
    })
}

/// The `Input.dispatchKeyEvent` parameters pressing and releasing `key`
/// while `modifiers` are held.
pub(crate) fn press(key: &Key, modifiers: u32) -> Vec<Value> {
    // Control, Alt and Meta combinations type nothing.
    let text = if modifiers & 0b0111 == 0 {
        key.text.clone()
    } else {
        None
    };
    let mut down = json!({
        "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
        "key": key.key,
        "code": key.code,
        "windowsVirtualKeyCode": key.key_code,
        "nativeVirtualKeyCode": key.key_code,
        "modifiers": modifiers,
    });
    if let Some(text) = &text {
        down["text"] = json!(text);
        down["unmodifiedText"] = json!(text);
    }
    let up = json!({
        "type": "keyUp",
        "key": key.key,
        "code": key.code,
        "windowsVirtualKeyCode": key.key_code,
        "nativeVirtualKeyCode": key.key_code,
        "modifiers": modifiers,
    });
    vec![down, up]
}

/// The events of a combination such as `Escape`, `Enter`, `Control+K` or
/// `Shift+Tab`: the modifiers go down in order, the key is pressed, the
/// modifiers come up in reverse.
pub(crate) fn combination(combo: &str) -> Result<Vec<Value>, String> {
    let mut parts: Vec<&str> = combo.split('+').collect();
    // `Control++` presses `+`.
    if parts.len() >= 2 && parts[parts.len() - 1].is_empty() && parts[parts.len() - 2].is_empty() {
        parts.truncate(parts.len() - 2);
        parts.push("+");
    }
    let unknown = |name: &str| format!("{name:?} is not a key name (in {combo:?})");
    let Some((last, held)) = parts.split_last() else {
        return Err(format!("{combo:?} names no key"));
    };
    let mut events = Vec::new();
    let mut modifiers = 0;
    let mut down = Vec::new();
    for name in held {
        let k = key(name.trim()).ok_or_else(|| unknown(name))?;
        if k.modifier == 0 {
            return Err(format!(
                "{name:?} is not a modifier (Control, Shift, Alt or Meta) in {combo:?}"
            ));
        }
        modifiers |= k.modifier;
        let mut event = press(&k, modifiers).remove(0);
        event["type"] = json!("rawKeyDown");
        events.push(event);
        down.push(k);
    }
    let name = if last.trim().is_empty() {
        *last
    } else {
        last.trim()
    };
    let mut k = key(name).ok_or_else(|| unknown(last))?;
    // A letter's `key` is lower-case unless Shift is held: `Control+K`
    // reports `k`, as a keyboard does.
    if k.key.chars().count() == 1 && k.key.chars().all(|c| c.is_ascii_alphabetic()) {
        let case: fn(&str) -> String = if modifiers & 8 != 0 {
            str::to_uppercase
        } else {
            str::to_lowercase
        };
        k.key = case(&k.key);
        k.text = k.text.as_deref().map(case);
    }
    events.extend(press(&k, modifiers));
    for k in down.iter().rev() {
        modifiers &= !k.modifier;
        let mut event = press(k, modifiers).remove(1);
        event["modifiers"] = json!(modifiers);
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_and_characters() {
        let enter = key("Enter").unwrap();
        assert_eq!((enter.key_code, enter.text.as_deref()), (13, Some("\r")));
        assert_eq!(key("Escape").unwrap().text, None);
        assert_eq!(key("F5").unwrap().key_code, 116);
        assert_eq!(key("a").unwrap().code, "KeyA");
        assert_eq!(key("Z").unwrap().key_code, 90);
        assert_eq!(key("7").unwrap().code, "Digit7");
        assert_eq!(key("/").unwrap().code, "Slash");
        assert_eq!(key("Escapee"), None);
        assert_eq!(character('é'), None);
        assert_eq!(character('\n'), key("Enter"));
    }

    #[test]
    fn a_plain_key_types_its_text() {
        let events = press(&key("a").unwrap(), 0);
        assert_eq!(events[0]["type"], "keyDown");
        assert_eq!(events[0]["text"], "a");
        assert_eq!(events[1]["type"], "keyUp");
        let escape = press(&key("Escape").unwrap(), 0);
        assert_eq!(escape[0]["type"], "rawKeyDown");
        assert!(escape[0].get("text").is_none());
    }

    #[test]
    fn combinations_hold_their_modifiers() {
        let events = combination("Control+K").unwrap();
        let kinds: Vec<(&str, &str, u64)> = events
            .iter()
            .map(|e| {
                (
                    e["type"].as_str().unwrap(),
                    e["key"].as_str().unwrap(),
                    e["modifiers"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                ("rawKeyDown", "Control", 2),
                ("rawKeyDown", "k", 2),
                ("keyUp", "k", 2),
                ("keyUp", "Control", 0),
            ]
        );
        assert!(events[1].get("text").is_none(), "Control+K types nothing");

        let shift = combination("Shift+a").unwrap();
        assert_eq!(shift[1]["key"], "A");
        assert_eq!(shift[1]["text"], "A");
        assert_eq!(shift[1]["modifiers"], 8);

        let plus = combination("Control++").unwrap();
        assert_eq!(plus[1]["key"], "+");
        assert_eq!(combination("Enter").unwrap().len(), 2);
    }

    #[test]
    fn unknown_keys_and_non_modifiers_are_errors() {
        assert!(combination("Control+Nope").unwrap_err().contains("Nope"));
        assert!(combination("a+b").unwrap_err().contains("not a modifier"));
    }
}
