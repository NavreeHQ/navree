//! `navree_cli::openapi_client`: the OpenAPI document `navree openapi:client`
//! reads, whatever a file or a teammate's export holds. It is JSON, then a
//! tree of schemas, paths, references and descriptions turned into
//! TypeScript; nesting, `$ref` chains and lists of types are where it could
//! run away.
//!
//! Invariants: no panic, no hang and no stack overflow for any text; a
//! document that is accepted gives the same file when run again, a file that
//! starts with the generated-file comment and holds its five declarations,
//! with braces that match, and has nothing the document wrote outside its
//! string literals and comments (no `declare`, no `global`); a refused
//! document gives a message.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_cli::openapi_client::generate;

/// Counts `{` and `}` outside string literals, as the file writes them.
fn braces_match(file: &str) -> bool {
    let mut depth = 0i64;
    let mut quoted = false;
    let mut escaped = false;
    let mut comment = false;
    let mut previous = '\0';
    for ch in file.chars() {
        if comment {
            if previous == '*' && ch == '/' {
                comment = false;
            }
            previous = ch;
            continue;
        }
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            previous = ch;
            continue;
        }
        match ch {
            '"' => quoted = true,
            '*' if previous == '/' => comment = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
        previous = ch;
    }
    depth == 0
}

/// The file's text without its string literals and comments.
fn code_of(file: &str) -> String {
    let mut code = String::new();
    let mut chars = file.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                let mut escaped = false;
                for next in chars.by_ref() {
                    if escaped {
                        escaped = false;
                    } else if next == '\\' {
                        escaped = true;
                    } else if next == '"' {
                        break;
                    }
                }
                code.push_str("\"\"");
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            other => code.push(other),
        }
    }
    code
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    match generate(text) {
        Ok(generated) => {
            assert!(generated.schema.starts_with("/**\n * This file was auto-generated"));
            for name in ["paths", "webhooks", "components", "$defs", "operations"] {
                assert!(
                    generated.schema.contains(&format!("\nexport type {name} = "))
                        || generated.schema.contains(&format!("\nexport interface {name} {{")),
                    "{name} is not declared"
                );
            }
            assert!(generated.schema.ends_with('\n'));
            assert!(braces_match(&generated.schema), "unbalanced braces");
            let code = code_of(&generated.schema);
            for word in ["declare", "global"] {
                assert!(!code.contains(word), "{word} outside a string or a comment");
            }
            assert!(generated.warnings.len() <= 50);
            let again = generate(text).expect("a document accepted once is accepted again");
            assert_eq!(generated, again);
        }
        Err(error) => assert!(!error.to_string().is_empty()),
    }
});
