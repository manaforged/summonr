use super::TAIL_LINES;

pub(super) fn digest_is_lossy(raw: &str) -> bool {
    raw.lines().count() > TAIL_LINES
}

pub(crate) fn digest(command: &str, raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    let out = if lines.len() <= TAIL_LINES {
        clip_lines(&lines)
    } else if crate::language::toolchain(command)
        && let Some(output) = toolchain_output(&lines)
    {
        output
    } else if searchish(command) {
        search_output(&lines)
    } else {
        plain_output(&lines)
    };
    if out.len() > RESULT_BYTES {
        let mut out = out[..out.floor_char_boundary(RESULT_BYTES)].to_string();
        out.push_str(&format!(
            "\n[output capped at {RESULT_BYTES} bytes — read the full output at the path below]"
        ));
        return out;
    }
    out
}

const RESULT_BYTES: usize = 50 * 1024;
const LINE_MAX: usize = 500;

fn clip_lines(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| {
            if line.len() > LINE_MAX {
                format!(
                    "{} [line clipped]",
                    &line[..line.floor_char_boundary(LINE_MAX)]
                )
            } else {
                (*line).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

pub(crate) fn searchish(command: &str) -> bool {
    super::rewrite::segment_verbs(command)
        .any(|verb| matches!(verb.as_str(), "rg" | "grep" | "ag" | "ack" | "find" | "fd"))
}

fn search_output(lines: &[&str]) -> String {
    const PER_FILE: usize = 20;
    let mut counts = std::collections::HashMap::new();
    let mut kept: Vec<String> = Vec::new();
    let mut capped = 0_usize;
    for line in lines {
        let key = line
            .split_once(':')
            .filter(|(path, rest)| {
                !path.is_empty()
                    && !path.bytes().all(|b| b.is_ascii_digit())
                    && rest.bytes().next().is_some_and(|b| b.is_ascii_digit())
            })
            .map(|(path, _)| path.to_string());
        if let Some(key) = key {
            let count = counts.entry(key).or_insert(0_usize);
            *count += 1;
            if *count > PER_FILE {
                capped += 1;
                continue;
            }
        }
        if line.len() > LINE_MAX {
            kept.push(format!(
                "{} [line clipped]",
                &line[..line.floor_char_boundary(LINE_MAX)]
            ));
        } else {
            kept.push((*line).to_string());
        }
    }
    let mut out = if kept.len() > TAIL_LINES {
        let refs: Vec<&str> = kept.iter().map(String::as_str).collect();
        plain_output(&refs)
    } else {
        kept.join("\n")
    };
    if capped > 0 {
        out.push_str(&format!(
            "\n[{capped} lines omitted — over the {PER_FILE}-per-file cap; narrow the pattern or use read op=search]"
        ));
    }
    out
}

fn toolchain_output(lines: &[&str]) -> Option<String> {
    let picked = toolchain_digest(lines);
    if picked.is_empty() {
        return None;
    }
    let mut out = picked.join("\n");
    out.push_str(&format!(
        "\n[{} lines captured; {} shown — errors, failures, and summaries]",
        lines.len(),
        picked.len()
    ));
    Some(out)
}

fn plain_output(lines: &[&str]) -> String {
    let head = &lines[..10];
    let tail = &lines[lines.len() - (TAIL_LINES - 10)..];
    format!(
        "{}\n[{} middle lines omitted — read the full output at the path below]\n{}",
        head.join("\n"),
        lines.len() - TAIL_LINES,
        tail.join("\n")
    )
}

fn toolchain_digest(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut carry = 0_usize;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let starts_block = trimmed.starts_with("error")
            || trimmed.starts_with("thread '")
            || trimmed.starts_with("failures:")
            || trimmed.starts_with("---- ")
            || trimmed.starts_with("FAIL")
            || trimmed.starts_with("not ok")
            || trimmed.starts_with("✗")
            || trimmed.contains("panicked at");
        let summary = trimmed.starts_with("test result:")
            || trimmed.starts_with("warning: unused")
            || trimmed.starts_with("warning: `")
            || trimmed.starts_with("Finished")
            || trimmed.starts_with("could not compile")
            || trimmed.starts_with("Tests:")
            || trimmed.starts_with("# tests")
            || trimmed.starts_with("# pass")
            || trimmed.starts_with("# fail")
            || trimmed.ends_with(" FAILED")
            || trimmed.ends_with(" ... FAILED");
        if starts_block {
            carry = 12;
        }
        if carry > 0 {
            out.push((*line).to_string());
            carry -= 1;
            if trimmed.is_empty()
                && index + 1 < lines.len()
                && !lines[index + 1].trim_start().starts_with(' ')
            {
                carry = 0;
            }
        } else if summary {
            out.push((*line).to_string());
        }
        if out.len() > 400 {
            out.push("[digest truncated]".to_string());
            break;
        }
    }
    out
}
