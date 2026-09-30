use crate::tools::budget::{Digest, shaped};

const PER_FILE: usize = 20;
const BLOCK_LINES: usize = 12;
const DIGEST_ROWS: usize = 400;
const BLOCK_STARTS: &[&str] = &[
    "error",
    "thread '",
    "failures:",
    "---- ",
    "FAIL",
    "not ok",
    "✗",
];
const BLOCK_CONTAINS: &str = "panicked at";
const SUMMARY_STARTS: &[&str] = &[
    "test result:",
    "warning: unused",
    "warning: `",
    "Finished",
    "could not compile",
    "Tests:",
    "# tests",
    "# pass",
    "# fail",
];
const SUMMARY_ENDS: &str = " FAILED";

pub(super) fn digest(command: &str, raw: &str) -> Digest {
    shaped(raw, |lines| {
        select(
            command,
            lines
                .into_iter()
                .map(std::borrow::Cow::into_owned)
                .collect(),
        )
    })
}

fn select(command: &str, lines: Vec<String>) -> Vec<String> {
    if crate::language::toolchain(command)
        && let Some(picked) = toolchain_lines(&lines)
    {
        return picked;
    }
    if searchish(command) {
        return search_lines(&lines);
    }
    lines
}

pub(crate) fn searchish(command: &str) -> bool {
    super::rewrite::segment_verbs(command)
        .any(|verb| crate::policy::verb_row(&verb).is_some_and(|row| row.search))
}

fn search_lines(lines: &[String]) -> Vec<String> {
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
        kept.push(line.clone());
    }
    if capped > 0 {
        kept.push(format!(
            "[{capped} lines omitted — over the {PER_FILE}-per-file cap; narrow the pattern or use read op=search]"
        ));
    }
    kept
}

fn toolchain_lines(lines: &[String]) -> Option<Vec<String>> {
    let mut picked = toolchain_digest(lines);
    if picked.is_empty() {
        return None;
    }
    picked.push(format!(
        "[{} lines captured; {} shown — errors, failures, and summaries]",
        lines.len(),
        picked.len()
    ));
    Some(picked)
}

fn toolchain_digest(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut carry = 0_usize;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if opens_block(trimmed) {
            carry = BLOCK_LINES;
        }
        if carry > 0 {
            out.push(line.clone());
            carry -= 1;
            if ends_block(lines, index) {
                carry = 0;
            }
        } else if is_summary(trimmed) {
            out.push(line.clone());
        }
        if out.len() > DIGEST_ROWS {
            out.push("[digest truncated]".to_string());
            break;
        }
    }
    out
}

fn opens_block(line: &str) -> bool {
    BLOCK_STARTS.iter().any(|prefix| line.starts_with(prefix)) || line.contains(BLOCK_CONTAINS)
}

fn is_summary(line: &str) -> bool {
    SUMMARY_STARTS.iter().any(|prefix| line.starts_with(prefix)) || line.ends_with(SUMMARY_ENDS)
}

fn ends_block(lines: &[String], index: usize) -> bool {
    lines[index].trim_start().is_empty()
        && index + 1 < lines.len()
        && !lines[index + 1].trim_start().starts_with(' ')
}
