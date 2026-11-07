use sigil::prelude::Widget;
use sigil::prelude::{LayoutStyle, NodeBuilder, Paint, Radius, Space, Token, container};

use crate::ToolState;
use crate::chat::entry::tools::workspace_relative;
use crate::chat::entry::tree::label;
use crate::chat::entry::{patch_of, preview};
use crate::model::Call;
use crate::text_style::TextStyle;
use summonr_agent::tools::Builtin;

const CHIP_COLS: usize = 48;
const META_COLS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StepVerb {
    Explored,
    Searched,
    Edited,
    Ran,
    Fetched,
    Inspected,
    Loaded,
    Planned,
    Delegated,
    Called,
}

impl StepVerb {
    fn of(name: &str) -> Self {
        match Builtin::of(name) {
            Some(Builtin::Read) => Self::Explored,
            Some(Builtin::Search) => Self::Searched,
            Some(Builtin::Browser) => Self::Fetched,
            Some(Builtin::Edit | Builtin::Write) => Self::Edited,
            Some(Builtin::RunCommand | Builtin::Monitor) => Self::Ran,
            Some(Builtin::Web) => Self::Fetched,
            Some(Builtin::Lsp) => Self::Inspected,
            Some(Builtin::Skill) => Self::Loaded,
            Some(Builtin::Todo) => Self::Planned,
            Some(Builtin::Task) => Self::Delegated,
            None => Self::Called,
        }
    }

    const fn row(self) -> (&'static str, &'static str) {
        match self {
            Self::Explored => ("Explored", "◎"),
            Self::Searched => ("Searched", "⌕"),
            Self::Edited => ("Edited", "✎"),
            Self::Ran => ("Ran", "▷"),
            Self::Fetched => ("Fetched", "↓"),
            Self::Inspected => ("Inspected", "◇"),
            Self::Loaded => ("Loaded", "◆"),
            Self::Planned => ("Planned", "☐"),
            Self::Delegated => ("Delegated", "⇢"),
            Self::Called => ("", "•"),
        }
    }

    pub(super) fn verb(self, name: &str) -> String {
        match self {
            Self::Called => name,
            kind => kind.row().0,
        }
        .to_string()
    }

    pub(super) const fn icon(self) -> &'static str {
        self.row().1
    }
}

pub(super) struct Step<'a> {
    pub(super) name: &'a str,
    pub(super) call: &'a Call,
    pub(super) index: usize,
    pub(super) at: usize,
}

impl Step<'_> {
    pub(super) fn kind(&self) -> StepVerb {
        StepVerb::of(self.name)
    }

    pub(super) fn chips(&self, roots: &[String]) -> Vec<String> {
        let shown = workspace_relative(&self.call.arg, roots);
        if shown.is_empty() {
            return Vec::new();
        }
        let items: Vec<String> = match self.kind() {
            StepVerb::Explored | StepVerb::Edited => {
                shown.split(", ").map(str::to_string).collect()
            }
            StepVerb::Searched
            | StepVerb::Ran
            | StepVerb::Fetched
            | StepVerb::Inspected
            | StepVerb::Loaded
            | StepVerb::Planned
            | StepVerb::Delegated
            | StepVerb::Called => vec![shown],
        };
        items
            .iter()
            .map(|item| crate::cells::ellipsize(item, CHIP_COLS))
            .collect()
    }
}

pub(super) fn chip(body: String) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .padding_x(Space::Sm)
                .padding_y(Space::raw(1.0, "a chip keeps the line height of its text"))
                .shrink(1.0)
                .min_size(0.0, 0.0),
        )
        .bg(Token::SurfaceInset)
        .border_w(1)
        .border_color(Token::BorderSubtle)
        .rounded(Radius::Sm)
        .child(label(body, TextStyle::xs(Token::Gray11).mono()).ellipsize())
}

pub(super) fn step_meta(step: &Step<'_>) -> Option<(String, TextStyle)> {
    let output = preview::plain(&step.call.output);
    match step.call.state {
        ToolState::Running => Some(("running…".to_string(), TextStyle::xs(Token::Gray10))),
        ToolState::Failed => Some((
            preview::exit(&output).map_or_else(
                || crate::cells::ellipsize(&preview::summary(&output, true), META_COLS),
                |code| format!("exit {code}"),
            ),
            TextStyle::xs(Token::Error11),
        )),
        ToolState::Ok => ok_meta(step, &output),
    }
}

fn ok_meta(step: &Step<'_>, output: &str) -> Option<(String, TextStyle)> {
    let muted = TextStyle::xs(Token::Gray10);
    match step.kind() {
        StepVerb::Edited => {
            patch_counts(step.call).map(|(adds, dels)| (format!("+{adds} −{dels}"), muted))
        }
        StepVerb::Explored => summonr_agent::tools::read_span(output)
            .map(|(from, to)| (format!("lines {from}–{to}"), muted)),
        StepVerb::Searched => {
            let found = output
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count();
            Some((summonr_core::text::plural(found, "match"), muted))
        }
        StepVerb::Ran => {
            let text = preview::summary(output, false);
            let last = summonr_agent::tools::command::test_result(&text)
                .unwrap_or_else(|| text.lines().last().unwrap_or("").trim())
                .to_string();
            (!last.is_empty()).then(|| {
                (
                    crate::cells::ellipsize(&last, META_COLS),
                    TextStyle::xs(Token::Success11),
                )
            })
        }
        StepVerb::Fetched
        | StepVerb::Inspected
        | StepVerb::Loaded
        | StepVerb::Planned
        | StepVerb::Delegated
        | StepVerb::Called => None,
    }
}

pub(super) fn patch_counts(call: &Call) -> Option<(usize, usize)> {
    let patch = call
        .patch
        .clone()
        .or_else(|| patch_of(&preview::plain(&call.output)))?;
    let body = || {
        patch
            .lines()
            .filter(|line| !line.starts_with("+++ ") && !line.starts_with("--- "))
    };
    Some((
        body().filter(|line| line.starts_with('+')).count(),
        body().filter(|line| line.starts_with('-')).count(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_counts_skip_the_file_headers() {
        let patch = "--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n-old\n+new\n+more\n same";
        let mut call = Call::new(
            String::new(),
            "x.rs".to_owned(),
            ToolState::Ok,
            String::new(),
        );
        call.patch = Some(patch.to_owned());
        assert_eq!(patch_counts(&call), Some((2, 1)));
    }
}
