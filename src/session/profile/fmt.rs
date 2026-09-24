pub(super) fn line(out: &mut String, text: String) {
    out.push_str(&text);
    out.push('\n');
}

pub(super) fn section(out: &mut String, title: &str, lines: impl Iterator<Item = String>) {
    let lines: Vec<String> = lines.collect();
    if lines.is_empty() {
        return;
    }
    line(out, format!("\n{title}"));
    for text in lines {
        line(out, text);
    }
}
pub(super) fn head_tail(command: &str) -> bool {
    command
        .split(['|', ';', '&'])
        .skip(1)
        .filter_map(|part| part.split_whitespace().next())
        .map(|verb| verb.rsplit('/').next().unwrap_or(verb))
        .any(|verb| matches!(verb, "head" | "tail" | "less" | "more"))
}

pub(super) fn brief(arguments: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    let text = value
        .get("command")
        .or_else(|| value.get("path"))
        .or_else(|| value.get("query"))
        .or_else(|| value.get("url"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| arguments.to_string(), str::to_string);
    crate::child::bounded(&text.split_whitespace().collect::<Vec<_>>().join(" "), 80)
}

pub(super) fn secs(millis: u64) -> String {
    format!("{:.1}s", millis as f64 / 1000.0)
}

pub(super) fn share(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "–".to_string();
    }
    format!("{:.0}%", part as f64 * 100.0 / whole as f64)
}

pub(super) fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}
