use super::scan::{Found, Row};
use super::walk::Report;
use super::{GREP_FILE_HITS, Mode, Query, relative};

#[derive(Default)]
struct Tally {
    hits: Vec<String>,
    matches: usize,
    truncated: bool,
}

pub(super) fn render(query: &Query, mut report: Report) -> String {
    if query.group {
        return groups(query, &report);
    }
    let page: Vec<Found> = std::mem::take(&mut report.found)
        .into_iter()
        .skip(query.offset)
        .collect();
    if query.matcher.is_none() {
        return file_list(query, &report, &page);
    }
    let mut tally = Tally::default();
    let mut seen = 0;
    for found in page {
        if tally.matches >= query.limit {
            tally.truncated = true;
            break;
        }
        seen += 1;
        let rel = relative(&query.shown, &found.path);
        match query.mode {
            Mode::Files => {
                tally.hits.push(rel);
                tally.matches += 1;
            }
            Mode::Count => {
                tally.hits.push(format!("{rel}:{}", found.hits.matches));
                tally.matches += 1;
            }
            Mode::Content => drain(found.hits.rows, query.limit, &mut tally),
        }
    }
    tally.truncated |= query.offset + seen < report.files;
    header(query, &report, &tally, query.offset + seen)
}

fn header(query: &Query, report: &Report, tally: &Tally, next: usize) -> String {
    let pattern = &query.pattern;
    if tally.hits.is_empty() {
        return missed(query, report);
    }
    let mut out = match query.mode {
        Mode::Files => format!(
            "{} with `{pattern}`, newest first ({} searched)",
            crate::text::plural(report.files, "file"),
            report.searched
        ),
        Mode::Count => format!(
            "{} in {} ({} searched)",
            crate::text::plural(report.match_total, "match"),
            crate::text::plural(report.files, "file"),
            report.searched
        ),
        Mode::Content => format!(
            "{} for `{pattern}`, newest file first ({} files searched)",
            crate::text::plural(tally.matches, "match"),
            report.searched
        ),
    };
    if tally.truncated {
        out.push_str(&format!(
            "; limit {} reached: offset {next} shows the next files, or narrow with path, name, or a more specific pattern",
            query.limit
        ));
    }
    if report.capped > 0 {
        out.push_str(&format!(
            "; {} had more than {GREP_FILE_HITS} matching lines",
            crate::text::plural(report.capped, "file"),
        ));
    }
    if report.skipped > 0 {
        out.push_str(&format!(
            "; {} skipped as binary, over 1 MiB, or unreadable",
            crate::text::plural(report.skipped, "file")
        ));
    }
    out.push_str(&coverage(report));
    for hit in &tally.hits {
        out.push('\n');
        out.push_str(hit);
    }
    out
}

fn missed(query: &Query, report: &Report) -> String {
    let mut out = if query.matcher.is_some() {
        format!("no matches for `{}`", query.pattern)
    } else {
        "no files match".to_string()
    };
    out.push_str(&format!(
        " ({} searched",
        crate::text::plural(report.searched.max(report.files), "file")
    ));
    if report.skipped > 0 {
        out.push_str(&format!(
            ", {} skipped as binary, over 1 MiB, or unreadable",
            report.skipped
        ));
    }
    out.push(')');
    let plain = query
        .pattern
        .chars()
        .all(|ch| ch.is_alphanumeric() || " _-.".contains(ch));
    if query.matcher.is_some() && !query.glob && query.name.is_empty() && plain {
        out.push_str("; to match file names instead, pass name");
    }
    if query.glob && query.name.is_empty() {
        out.push_str("; name matches words in file names without wildcards");
    }
    out.push_str(&skipped_note(query));
    out.push_str(&coverage(report));
    out
}

fn file_list(query: &Query, report: &Report, page: &[Found]) -> String {
    if page.is_empty() && query.offset == 0 {
        return missed(query, report);
    }
    let shown = page.len().min(query.limit);
    let mut out = format!(
        "{}, newest first",
        crate::text::plural(report.files, "file")
    );
    let end = query.offset + shown;
    if query.offset > 0 || end < report.files {
        out.push_str(&format!("; showing {}–{end}", query.offset + 1));
    }
    if end < report.files {
        out.push_str(&format!("; offset {end} shows the next files"));
    }
    out.push_str(&coverage(report));
    for found in page.iter().take(shown) {
        out.push('\n');
        out.push_str(&relative(&query.shown, &found.path));
    }
    out
}

fn groups(query: &Query, report: &Report) -> String {
    if report.groups.is_empty() {
        return missed(query, report);
    }
    let mut rows: Vec<(&String, &usize)> = report.groups.iter().collect();
    rows.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    let mut out = format!(
        "{} in {}, by folder",
        crate::text::plural(report.files, "file"),
        crate::text::plural(rows.len(), "folder")
    );
    if rows.len() > query.limit {
        out.push_str(&format!("; showing the {} largest", query.limit));
    }
    out.push_str(&coverage(report));
    for (folder, count) in rows.into_iter().take(query.limit) {
        out.push_str(&format!(
            "\n{count:>6}  {}",
            relative(&query.shown, std::path::Path::new(folder))
        ));
    }
    out
}

fn skipped_note(query: &Query) -> String {
    if query.no_ignore {
        return format!("; {} folders skipped", crate::STATE_DIRS.join(" and "));
    }
    format!(
        "; gitignored files and {} folders skipped (no_ignore true includes them)",
        crate::SKIP_DIRS.join(", ")
    )
}

fn coverage(report: &Report) -> String {
    let mut out = String::new();
    if let Some(built) = report.indexed {
        out.push_str(&format!(
            "; from the file index of {} ago, so files made since appear after the refresh this search started",
            crate::text::duration(crate::mail::now().saturating_sub(built))
        ));
    }
    if report.links > 0 {
        out.push_str(&format!(
            "; includes {}, which find -type f leaves out",
            crate::text::plural(report.links, "symbolic link")
        ));
    }
    if report.unreadable > 0 {
        out.push_str(&format!(
            "; {} could not be read",
            crate::text::plural(report.unreadable, "folder")
        ));
    }
    if report.stopped {
        out.push_str(&format!(
            "; stopped after {}s before the walk finished, so results cover only the files searched: narrow path, name, or depth for the rest",
            crate::limits::limits().search_secs
        ));
    }
    out
}

fn drain(rows: Vec<Row>, limit: usize, tally: &mut Tally) {
    let mut pending_break = false;
    for row in rows {
        match row {
            Row::Break => {
                pending_break = !tally.hits.is_empty();
            }
            Row::Ctx(line) => {
                if pending_break {
                    tally.hits.push("--".to_string());
                    pending_break = false;
                }
                tally.hits.push(line);
            }
            Row::Hit(line) => {
                if tally.matches >= limit {
                    tally.truncated = true;
                    break;
                }
                tally.matches += 1;
                if pending_break {
                    tally.hits.push("--".to_string());
                    pending_break = false;
                }
                tally.hits.push(line);
            }
        }
    }
}
