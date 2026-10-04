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

#[derive(Clone, Copy)]
enum Escape {
    Csi,
    Text,
    Single,
}

fn escape_of(next: char) -> Escape {
    if next == '[' {
        Escape::Csi
    } else if matches!(next, ']' | 'P' | '_' | '^' | 'X') {
        Escape::Text
    } else {
        Escape::Single
    }
}

fn c1(ch: char) -> Option<Escape> {
    if ch == '\u{9b}' {
        Some(Escape::Csi)
    } else if matches!(ch, '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}') {
        Some(Escape::Text)
    } else {
        ('\u{80}'..='\u{9f}')
            .contains(&ch)
            .then_some(Escape::Single)
    }
}

fn skip(kind: Escape, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match kind {
        Escape::Csi => {
            for next in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&next) {
                    break;
                }
            }
        }
        Escape::Text => {
            while let Some(next) = chars.next() {
                if matches!(next, '\u{7}' | '\u{9c}')
                    || (next == '\u{1b}' && chars.next_if_eq(&'\\').is_some())
                {
                    break;
                }
            }
        }
        Escape::Single => {}
    }
}

pub(super) fn plain(output: &str) -> String {
    let mut out = String::with_capacity(output.len());
    let mut chars = output.chars().peekable();
    while let Some(ch) = chars.next() {
        if matches!(ch, '\n' | '\t') || !ch.is_control() {
            out.push(ch);
        } else if ch == '\r' {
            if chars.peek() != Some(&'\n') {
                out.truncate(out.rfind('\n').map_or(0, |at| at + 1));
            }
        } else if ch == '\u{1b}' {
            if let Some(next) = chars.next() {
                skip(escape_of(next), &mut chars);
            }
        } else if let Some(kind) = c1(ch) {
            skip(kind, &mut chars);
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

#[cfg(test)]
mod tests {
    use super::plain;

    #[test]
    fn plain_drops_terminal_controls_and_string_payloads() {
        let raw = "a\u{1b}[31mred\u{1b}[0m \u{1b}]0;title\u{7}b\u{1b}Pdcs data\u{1b}\\c\u{1b}_apc\u{7}d\u{9b}2Je\u{9d}osc\u{9c}f\u{7}\u{8}g\u{0}h\tI\u{85}";
        assert_eq!(plain(raw), "ared bcdefgh\tI");
    }

    #[test]
    fn plain_keeps_the_text_after_a_carriage_return() {
        let raw = "start\n 10%\r 50%\r100% done\r\nnext\r\n";
        assert_eq!(plain(raw), "start\n100% done\nnext\n");
    }
}
