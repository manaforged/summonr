use sigil::prelude::{LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, Radius, Token, container};

use super::fold::patch_counts;
use super::preview;
use super::tools::workspace_relative;
use super::tree::label;
use crate::tools::Builtin;
use crate::tui::ToolState;
use crate::tui::sigil_view::look::Look;
use crate::tui::sigil_view::model::Call;

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
            Some(Builtin::RunCommand) => Self::Ran,
            Some(Builtin::Web) => Self::Fetched,
            Some(Builtin::Lsp) => Self::Inspected,
            Some(Builtin::Skill) => Self::Loaded,
            Some(Builtin::Todo) => Self::Planned,
            Some(Builtin::Task) => Self::Delegated,
            None => Self::Called,
        }
    }

    pub(super) fn verb(self, name: &str) -> String {
        match self {
            Self::Explored => "Explored",
            Self::Searched => "Searched",
            Self::Edited => "Edited",
            Self::Ran => "Ran",
            Self::Fetched => "Fetched",
            Self::Inspected => "Inspected",
            Self::Loaded => "Loaded",
            Self::Planned => "Planned",
            Self::Delegated => "Delegated",
            Self::Called => name,
        }
        .to_string()
    }

    pub(super) const fn icon(self) -> &'static str {
        match self {
            Self::Explored => "◎",
            Self::Searched => "⌕",
            Self::Edited => "✎",
            Self::Ran => "▷",
            Self::Fetched => "↓",
            Self::Inspected => "◇",
            Self::Loaded => "◆",
            Self::Planned => "☐",
            Self::Delegated => "⇢",
            Self::Called => "•",
        }
    }
}

pub(super) struct Step {
    pub(super) name: String,
    pub(super) call: Call,
    pub(super) index: usize,
    pub(super) at: Option<usize>,
}

impl Step {
    pub(super) fn kind(&self) -> StepVerb {
        StepVerb::of(&self.name)
    }

    pub(super) fn counts(&self) -> Option<(usize, usize)> {
        patch_counts(&self.call)
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
            StepVerb::Searched => vec![shown.clone()],
            StepVerb::Ran
            | StepVerb::Fetched
            | StepVerb::Inspected
            | StepVerb::Loaded
            | StepVerb::Planned
            | StepVerb::Delegated
            | StepVerb::Called => vec![shown],
        };
        items
            .iter()
            .map(|item| crate::text::ellipsize(item, CHIP_COLS))
            .collect()
    }
}

pub(super) fn chip(body: String) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .padding_x(8.0)
                .padding_y(1.0)
                .shrink(1.0)
                .min_size(0.0, 0.0),
        )
        .bg(Token::SurfaceInset)
        .border_w(1)
        .border_color(Token::BorderSubtle)
        .rounded(Radius::Sm)
        .child(label(body, Look::xs(Token::Gray11).mono()).ellipsize())
}

pub(super) fn step_meta(step: &Step) -> Option<(String, Look)> {
    let output = preview::plain(&step.call.output);
    match step.call.state {
        ToolState::Running => Some(("running…".to_string(), Look::xs(Token::Gray10))),
        ToolState::Failed => Some((
            preview::exit(&output).map_or_else(
                || crate::text::ellipsize(&preview::summary(&output, true), META_COLS),
                |code| format!("exit {code}"),
            ),
            Look::xs(Token::Error11),
        )),
        ToolState::Ok => ok_meta(step, &output),
    }
}

fn ok_meta(step: &Step, output: &str) -> Option<(String, Look)> {
    let muted = Look::xs(Token::Gray10);
    match step.kind() {
        StepVerb::Edited => step
            .counts()
            .map(|(adds, dels)| (format!("+{adds} −{dels}"), muted)),
        StepVerb::Explored => {
            crate::tools::read_span(output).map(|(from, to)| (format!("lines {from}–{to}"), muted))
        }
        StepVerb::Searched => {
            let found = output
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count();
            Some((crate::text::plural(found, "match"), muted))
        }
        StepVerb::Ran => {
            let text = preview::summary(output, false);
            let last = crate::tools::command::test_result(&text)
                .unwrap_or_else(|| text.lines().last().unwrap_or("").trim())
                .to_string();
            (!last.is_empty()).then(|| {
                (
                    crate::text::ellipsize(&last, META_COLS),
                    Look::xs(Token::Success11),
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
