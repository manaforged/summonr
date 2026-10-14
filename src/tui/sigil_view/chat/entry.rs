use sigil::kernel::ObjectFit;
use sigil::prelude::AlignItems;
use sigil::prelude::DiffView;
use sigil::prelude::ImageSource;
use sigil::prelude::IntoNode;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::MouseButton;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::Paint;
use sigil::prelude::WidgetLayoutExt;
use sigil::prelude::chart;
use sigil::prelude::container;
use sigil::prelude::diff;
use sigil::prelude::icon_button;
use sigil::prelude::image;
use sigil::prelude::markdown_inline;
use sigil::prelude::text;
use sigil::terminal::caps;
use sigil::widgets::DiffBuilder;

use super::{App, Call, Entry, canvas};
use crate::prefs::{DiffLayout, Prefs};
use crate::tui::sigil_view::look::Look;

mod children;
mod collapse;
#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) mod fold;
mod reply;
#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) use reply::diff_card;
pub(in crate::tui::sigil_view) mod preview;
mod steps;
mod tools;
#[cfg(feature = "gui")]
mod verb;
#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) use super::surface::{Content, content};
pub(in crate::tui::sigil_view) use steps::{Slot, slot, subagents};
#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) use tools::glyph;
pub(in crate::tui::sigil_view) use tools::workspace_relative;
#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) use tree::label;
mod tree;

const COMPACT_ROWS: usize = 32;

pub(in crate::tui::sigil_view) fn row(
    entry: Entry,
    live: bool,
    prefs: &Prefs,
    app: &App,
    index: usize,
) -> NodeBuilder {
    let tools = prefs.tools;
    let layout = prefs.diff_layout;
    match entry {
        Entry::Tool {
            id,
            name,
            arg,
            state,
            output,
            expanded,
            patch,
        } => tools::tool_card(
            &name,
            &Call {
                id,
                arg,
                state,
                output,
                expanded,
                patch,
            },
            tools,
            layout,
            app,
            index,
            None,
        ),
        Entry::ToolGroup { name, calls } => {
            tools::group_card(&name, &calls, tools, layout, app, index)
        }
        entry => passive(entry, live, prefs, app),
    }
}

pub(in crate::tui::sigil_view) fn hidden(entry: &Entry, prefs: &Prefs) -> bool {
    blank(entry)
        || matches!(entry, Entry::Receipt(_)) && prefs.receipts == crate::prefs::Visibility::Off
}

fn blank(entry: &Entry) -> bool {
    matches!(
        entry,
        Entry::Assistant { text, .. } | Entry::Thinking(text) | Entry::Receipt(text)
            if text.trim().is_empty()
    )
}

fn passive(entry: Entry, live: bool, prefs: &Prefs, app: &App) -> NodeBuilder {
    if blank(&entry) {
        return container();
    }
    match entry {
        Entry::User(body) => container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .padding_x(16.0)
                    .min_size(0.0, 0.0),
            )
            .bg(sigil::prelude::Token::Gray5)
            .child(
                text(body)
                    .font_size(14.0)
                    .text_color(sigil::prelude::Token::Gray12),
            ),
        Entry::Assistant { text: body, .. } => assistant(body, app),
        Entry::Thinking(body) => {
            collapse::collapsible(&body, "thinking", prefs.reasoning, app, true, live)
        }
        Entry::Receipt(body) => {
            collapse::collapsible(&body, "receipt", prefs.receipts, app, false, false)
        }
        Entry::Verify { command, passed } => verify(&command, passed),
        Entry::Mail { from, text: body } => prose(
            &format!("{from}: {body}"),
            Look::sm(sigil::prelude::Token::Info11),
        ),
        Entry::Aborted(reason) | Entry::Error(reason) => wrap(
            text(reason)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Error11),
        ),
        Entry::System(body) => wrap(
            text(body)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        ),
        Entry::Chart(rounds) => wrap(
            chart(
                rounds
                    .iter()
                    .map(|round| round.total() as f32)
                    .collect::<Vec<f32>>(),
            )
            .height(72.0)
            .width(280.0),
        ),
        Entry::Image {
            source,
            bytes,
            label: name,
        } => picture(source, bytes, name),
        Entry::Tool { .. } | Entry::ToolGroup { .. } => container(),
    }
}

fn assistant(body: String, app: &App) -> NodeBuilder {
    let copy_app = app.clone();
    let copied = body.clone();
    let shown = reply::body(&body, app);
    wrap(
        container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .align_items(AlignItems::Start)
                    .gap(8.0),
            )
            .child(
                container()
                    .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
                    .child(shown),
            )
            .child(
                icon_button("⧉", move || copy_app.copy.set(Some(copied.clone())))
                    .ghost()
                    .accessible("Copy reply"),
            ),
    )
}

fn verify(command: &str, passed: bool) -> NodeBuilder {
    wrap(
        sigil::kernel::text(format!("{} {command}", if passed { "✓" } else { "✗" }))
            .nowrap()
            .text_color(sigil::prelude::Token::OnSurface)
            .font_size(12.0)
            .text_color(if passed {
                sigil::prelude::Token::Success11
            } else {
                sigil::prelude::Token::Error11
            }),
    )
}

fn picture(source: ImageSource, bytes: std::rc::Rc<[u8]>, name: String) -> NodeBuilder {
    if !canvas::is_window() && caps().is_none_or(|caps| caps.graphics.is_none()) {
        return wrap(Look::sm(sigil::prelude::Token::Gray11).on(text("[attached image]")));
    }
    let (width, height) = (source.width() as f32, source.height() as f32);
    let id = source.id();
    let kind = crate::tools::image_type(&bytes).and_then(|media| media.strip_prefix("image/"));
    let picture = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .pointer_up(move |event| {
            if event.button != Some(MouseButton::Left) {
                return;
            }
            let file = format!("summonr-image-{id}.{}", kind.unwrap_or("png"));
            let path = std::env::temp_dir().join(file);
            if crate::durable::store::write(&path, &bytes).is_ok() {
                drop(std::process::Command::new("open").arg(&path).spawn());
            }
        });
    picture
        .child(
            image(source)
                .alt(name.clone())
                .fit(ObjectFit::Contain)
                .layout(LayoutStyle::block().width(width).height(height)),
        )
        .child(
            sigil::kernel::text(name)
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        )
}

pub(super) fn wrap(child: impl IntoNode + 'static) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
        .child(child)
}

pub(in crate::tui::sigil_view) fn patch_of(output: &str) -> Option<String> {
    let mut patch = Vec::new();
    for line in output
        .lines()
        .skip_while(|line| !line.starts_with("@@ ") && !line.starts_with("diff --git "))
    {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if !patch.is_empty() {
                let path = rest.split_whitespace().nth(1).unwrap_or(rest);
                patch.push(path.trim_start_matches("b/"));
            }
        } else if line.starts_with("@@ ")
            || line.is_empty()
            || (line.starts_with([' ', '+', '-', '\\'])
                && !line.starts_with("--- ")
                && !line.starts_with("+++ "))
        {
            patch.push(line);
        } else if !FILE_HEADERS.iter().any(|head| line.starts_with(head)) {
            break;
        }
    }
    let patch = patch.join("\n").trim_end().to_string();
    (patch.contains("@@ ")).then_some(patch)
}

const FILE_HEADERS: [&str; 9] = [
    "index ",
    "--- ",
    "+++ ",
    "new file mode",
    "deleted file mode",
    "old mode",
    "new mode",
    "similarity index",
    "rename ",
];

fn patch_path(output: &str) -> Option<String> {
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.trim().trim_start_matches("b/");
            if path != "/dev/null" {
                return Some(path.to_string());
            }
        }
        if let Some(rest) = line.strip_prefix("diff --git ")
            && let Some(b_side) = rest.split_whitespace().nth(1)
        {
            return Some(b_side.trim_start_matches("b/").to_string());
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Fit {
    Wide,
    Narrow,
}

impl Fit {
    pub(in crate::tui::sigil_view) fn of(cols: u16) -> Self {
        if super::canvas::inner_cols(usize::from(cols))
            < usize::from(crate::limits::limits().diff_split_min_cols)
        {
            Self::Narrow
        } else {
            Self::Wide
        }
    }

    pub(in crate::tui::sigil_view) fn chat(app: &App) -> Self {
        if super::canvas::is_window() {
            Self::Wide
        } else {
            Self::of(app.chat_cols())
        }
    }
}

pub(in crate::tui::sigil_view) fn patch_view(
    app: &App,
    patch: impl Into<String>,
    layout: DiffLayout,
    fit: Fit,
) -> DiffBuilder {
    let pick = app.clone();
    diff(patch.into())
        .view(match (layout, fit) {
            (DiffLayout::Split, Fit::Wide) => DiffView::Split,
            (DiffLayout::Split, Fit::Narrow) | (DiffLayout::Unified, _) => DiffView::Unified,
        })
        .change(move |choice| {
            pick.set_diff_layout(match choice {
                DiffView::Split => DiffLayout::Split,
                DiffView::Unified => DiffLayout::Unified,
            });
        })
}

fn prose(body: &str, look: Look) -> NodeBuilder {
    let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(0.0));
    for para in body.split('\n') {
        column = column.child(look.on(markdown_inline(para)));
    }
    column
}
