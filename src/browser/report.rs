const ESCAPED: &str =
    "blocked: the page reached a host summonr does not allow; the tab now shows about:blank";

pub(super) fn outcome(outcome: &charm::Outcome, escaped: bool, out_max: usize) -> String {
    let mut out = format!("url: {}\n", outcome.url_after);
    if outcome.url_before != outcome.url_after {
        out.push_str(&format!("from: {}", outcome.url_before));
        out.push('\n');
    }
    out.push_str(&format!("title: {}", outcome.title));
    out.push('\n');
    if !outcome.new_pages.is_empty() {
        let ids: Vec<&str> = outcome
            .new_pages
            .iter()
            .map(charm::PageId::as_str)
            .collect();
        out.push_str(&format!("new tabs: {}", ids.join(" ")));
        out.push('\n');
    }
    if let Some(dialogs) = outcome
        .dialogs
        .as_ref()
        .filter(|dialogs| !dialogs.is_empty())
    {
        out.push_str(&format!("dialogs answered: {}", dialogs.len()));
        out.push('\n');
        for dialog in dialogs {
            out.push_str(&format!("- {:?}: {}", dialog.kind, dialog.message));
            out.push('\n');
        }
    }
    if let Some(console) = &outcome.console {
        let errors = console
            .iter()
            .filter(|message| {
                matches!(
                    message.level,
                    charm::ConsoleLevel::Error | charm::ConsoleLevel::Exception
                )
            })
            .count();
        if errors > 0 {
            out.push_str(&format!("console errors: {errors}"));
            out.push('\n');
        }
    }
    if escaped {
        out.push_str(ESCAPED);
        out.push('\n');
    }
    if let Some(snapshot) = &outcome.snapshot {
        out.push_str(&crate::text::ellipsize(snapshot, out_max));
    }
    out
}

pub(super) fn tabs(pages: &[charm::PageInfo]) -> String {
    pages
        .iter()
        .map(|page| {
            let mark = if page.active { "*" } else { " " };
            format!("{mark} {} {:?} {}", page.id.as_str(), page.title, page.url)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
