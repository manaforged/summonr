use std::collections::BTreeSet;

use serde_json::Value;

use super::catalog;

fn named(text: &str) -> BTreeSet<String> {
    let identifier =
        |word: &str| !word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    let mut keys = BTreeSet::new();
    for line in text.lines() {
        if let Some(bullet) = line.strip_prefix("- ") {
            for clause in bullet.split("; ") {
                let Some((head, _)) = clause.split_once(':') else {
                    continue;
                };
                for part in head.split('+') {
                    let key = part.trim_end_matches("[]");
                    let key = key.split_once('=').map_or(key, |(key, _)| key);
                    if identifier(key) {
                        keys.insert(key.to_string());
                    }
                }
            }
        }
    }
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')'))
        .collect();
    for pair in words.windows(2) {
        if matches!(pair[1], "true" | "N")
            && identifier(pair[0])
            && !matches!(pair[0], "is" | "are")
        {
            keys.insert(pair[0].to_string());
        }
    }
    keys
}

fn declared(schema: &Value) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, nested) in properties {
            keys.insert(key.clone());
            if let Some(items) = nested.get("items") {
                keys.extend(declared(items));
            }
        }
    }
    keys
}

#[test]
fn every_option_a_tool_help_names_is_in_its_schema() {
    for tool in &catalog().tool {
        let Some(definition) = &tool.definition else {
            continue;
        };
        let text = format!(
            "{}\n{}",
            definition.function.description,
            tool.help.as_deref().unwrap_or_default()
        );
        let missing: Vec<_> = named(&text)
            .difference(&declared(&definition.function.parameters))
            .cloned()
            .collect();
        assert!(missing.is_empty(), "{}: {missing:?}", tool.name);
    }
}
