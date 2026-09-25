use crate::prefs::ToolOutput;

const SHOWN: usize = 3;

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

pub(super) fn text(tools: ToolOutput, name: &str, output: &str, failed: bool) -> String {
    if tools == ToolOutput::Full {
        return output.to_string();
    }
    if name == "todo" && !failed {
        return String::new();
    }
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .skip_while(|line| *line == "exit 0")
        .collect();
    if failed {
        let error = lines
            .iter()
            .find(|line| {
                let lower = line.to_lowercase();
                lower.contains("error") || lower.contains("fail")
            })
            .or_else(|| lines.iter().find(|line| !line.starts_with("exit ")))
            .copied()
            .unwrap_or("");
        return match lines.len() {
            0 | 1 => error.to_string(),
            count => format!("{error}\n… +{} lines", count - 1),
        };
    }
    if lines.len() <= SHOWN {
        return lines.join("\n");
    }
    let tail = lines[lines.len() - (SHOWN - 1)..].join("\n");
    format!("{}\n… +{} lines\n{tail}", lines[0], lines.len() - SHOWN)
}
