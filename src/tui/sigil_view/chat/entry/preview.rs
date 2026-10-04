const SHOWN: usize = 3;

pub(super) fn summary(output: &str, failed: bool) -> String {
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if failed {
        return lines
            .iter()
            .find(|line| {
                let lower = line.to_lowercase();
                lower.contains("error") || lower.contains("fail")
            })
            .or_else(|| lines.iter().find(|line| !line.starts_with("exit ")))
            .copied()
            .unwrap_or("")
            .to_string();
    }
    crate::prefs::ToolOutput::Compact.text(output).to_string()
}

pub(super) fn plain(output: &str) -> String {
    let mut out = String::with_capacity(output.len());
    let mut chars = output.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(next) = chars.next() {
                    if next == '\u{7}' || (next == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub(super) fn text(full: bool, name: &str, output: &str, failed: bool, running: bool) -> String {
    if full {
        return output.to_string();
    }
    if crate::tools::Builtin::is(name, crate::tools::Builtin::Todo) && !failed {
        return String::new();
    }
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .skip_while(|line| *line == "exit 0")
        .collect();
    if failed {
        let error = summary(output, true);
        return match lines.len() {
            0 | 1 => error,
            count => format!("{error}\n… +{} lines", count - 1),
        };
    }
    if lines.len() <= SHOWN {
        return lines.join("\n");
    }
    if running {
        let tail = lines[lines.len() - SHOWN..].join("\n");
        return format!("… +{} lines\n{tail}", lines.len() - SHOWN);
    }
    let tail = lines[lines.len() - (SHOWN - 1)..].join("\n");
    format!("{}\n… +{} lines\n{tail}", lines[0], lines.len() - SHOWN)
}

pub(super) fn exit(output: &str) -> Option<&str> {
    output
        .lines()
        .next()?
        .strip_prefix("exit ")
        .filter(|code| code.parse::<i32>().is_ok())
}
