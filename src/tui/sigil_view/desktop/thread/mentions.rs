use super::*;
use crate::tui::sigil_view::suggest::mention;

fn spans(draft: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut found = Vec::new();
    for (at, _) in draft.match_indices('@') {
        let rest = &draft[at + 1..];
        let (caret, end) = match rest.strip_prefix('"') {
            Some(quoted) => match quoted.find('"') {
                Some(close) => (at + 2 + close, at + 3 + close),
                None => continue,
            },
            None => {
                let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
                (at + 1 + len, at + 1 + len)
            }
        };
        if let Some((start, path)) = mention(draft, caret)
            && start == at
            && !path.is_empty()
        {
            found.push((at..end, path.to_string()));
        }
    }
    found
}

fn without(draft: &str, span: std::ops::Range<usize>) -> String {
    let tail = draft[span.end..]
        .strip_prefix(' ')
        .unwrap_or(&draft[span.end..]);
    format!("{}{tail}", &draft[..span.start])
}

pub(super) fn chips(app: App, editing: bool) -> NodeBuilder {
    let draft = app.draft;
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(XS)
                .align_items(AlignItems::Center),
        )
        .children_reactive(move || {
            if editing {
                return Vec::new();
            }
            let text = draft.get();
            spans(&text)
                .into_iter()
                .map(|(span, path)| {
                    let remove = app.clone();
                    let label = format!("Remove {path}");
                    button(format!("@{path} ×"))
                        .ghost()
                        .size(ControlSize::Compact)
                        .accessible(label)
                        .activate(move || {
                            let draft = remove.draft.peek();
                            if draft.get(span.clone()).is_some() {
                                remove.edit(&without(&draft, span.clone()));
                            }
                        })
                })
                .collect()
        })
}
