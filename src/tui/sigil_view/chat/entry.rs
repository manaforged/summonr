use sigil::prelude::{
    AlignItems, DiffLayout as SigilDiffLayout, Fill, Geometry, IntoNode, LayoutBuilderExt,
    LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt, ObjectFit, Overflow,
    PointerEvent, chart, classes, container, diff, image, label, markdown, signal, text,
};

use super::{App, Entry};
use crate::prefs::{DiffLayout, Prefs, ToolOutput, Visibility};

use crate::tui::ToolState;

const COMPACT_ROWS: usize = 32;

pub(in crate::tui::sigil_view) fn row(entry: Entry, prefs: &Prefs, app: &App) -> NodeBuilder {
    let tools = prefs.tools;
    let layout = prefs.diff_layout;
    match entry {
        Entry::User(body) => container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .padding_x(16.0)
                    .min_size(0.0, 0.0),
            )
            .class(classes!("bg-gray-5"))
            .child(text(format!("▌ {body}")).class(classes!("text-sm text-gray-12"))),
        Entry::Assistant { text: body, .. } if body.trim().is_empty() => container(),
        Entry::Thinking(body) if body.trim().is_empty() => container(),
        Entry::Receipt(body) if body.trim().is_empty() => container(),
        Entry::Assistant { text: body, .. } => wrap(markdown(signal(body))),
        Entry::Thinking(body) => {
            let live = {
                let model = app.model.get();
                model.busy_since.is_some()
                    && matches!(model.entries.last(), Some(Entry::Thinking(last)) if *last == body)
            };
            collapsible(&body, "thinking", prefs.reasoning, app, true, live)
        }
        Entry::Receipt(body) => collapsible(&body, "receipt", prefs.receipts, app, false, false),
        Entry::Tool {
            name,
            arg,
            state,
            output,
        } => tool_card(&name, &arg, &state, &output, tools, layout, app),
        Entry::Verify { command, passed } => wrap(
            label(format!("{} {command}", if passed { "✓" } else { "✗" })).class(if passed {
                classes!("text-xs text-success-11")
            } else {
                classes!("text-xs text-error-11")
            }),
        ),
        Entry::Mail { from, text: body } => {
            prose(&format!("{from}: {body}"), "text-sm text-info-11")
        }
        Entry::Aborted(reason) | Entry::Error(reason) => {
            wrap(text(reason).class(classes!("text-xs text-error-11")))
        }
        Entry::System(body) => wrap(text(body).class(classes!("text-xs text-gray-11"))),
        Entry::Chart(rounds) => wrap(
            chart(rounds.iter().map(|round| round.total() as f32))
                .height(72.0)
                .width(280.0),
        ),
        Entry::Image {
            source,
            bytes,
            label: name,
        } => {
            let (width, height) = (source.width() as f32, source.height() as f32);
            let id = source.id();
            let kind =
                crate::tools::image_type(&bytes).and_then(|media| media.strip_prefix("image/"));
            container()
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
                })
                .child(
                    image(source)
                        .alt(name.clone())
                        .fit(ObjectFit::Contain)
                        .layout(LayoutStyle::block().width(width).height(height)),
                )
                .child(label(name).class(classes!("text-xs text-gray-11")))
        }
    }
}

fn collapsible(
    body: &str,
    kind: &str,
    visibility: Visibility,
    app: &App,
    reasoning: bool,
    live: bool,
) -> NodeBuilder {
    let toggle = app.clone();
    let next = if visibility == Visibility::Expanded {
        "collapsed"
    } else {
        "expanded"
    };
    let press = move |event: &mut PointerEvent| {
        if event.button == Some(MouseButton::Left) {
            if reasoning {
                toggle.reasoning_command(Some(next));
            } else {
                toggle.receipts_command(Some(next));
            }
            event.stop_propagation();
        }
    };
    match visibility {
        Visibility::Off | Visibility::Collapsed if live => {
            wrap(label("✻ Thinking…").class(classes!("text-xs italic text-gray-11")))
                .pointer_down(press)
        }
        Visibility::Off => {
            wrap(label(format!("⌄ {kind} · hidden")).class(classes!("text-xs italic text-gray-11")))
                .pointer_down(press)
        }
        Visibility::Collapsed => {
            let count = body.lines().filter(|line| !line.trim().is_empty()).count();
            wrap(
                label(format!("⌄ {kind} · {count} lines"))
                    .class(classes!("text-xs italic text-gray-11")),
            )
            .pointer_down(press)
        }
        Visibility::Expanded => container()
            .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
            .child(
                label(if live {
                    "✻ Thinking…".to_string()
                } else {
                    format!("✻ {kind}")
                })
                .class(classes!("text-xs italic text-gray-11")),
            )
            .child(prose(body, "text-xs italic text-gray-11"))
            .pointer_down(press),
    }
}

fn wrap(child: impl IntoNode + 'static) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
        .child(child)
}

fn patch_of(output: &str) -> Option<String> {
    let patch = output
        .lines()
        .skip_while(|line| !line.starts_with("@@ "))
        .take_while(|line| line.is_empty() || line.starts_with([' ', '+', '-', '@', '\\']))
        .collect::<Vec<_>>()
        .join("\n");
    let patch = patch.trim_end().to_string();
    (!patch.is_empty()).then_some(patch)
}

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

fn glyph(state: &ToolState) -> (&'static str, &'static str) {
    match state {
        ToolState::Running => ("○", "text-xs text-gray-11"),
        ToolState::Ok => ("✓", "text-xs text-success-11"),
        ToolState::Failed => ("✗", "text-xs text-error-11"),
    }
}

fn tool_header(name: &str, arg: &str, state: &ToolState) -> NodeBuilder {
    let (mark, tone) = glyph(state);
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(label(mark).class(tone))
        .child(label(name.to_string()).class(classes!("text-xs text-gray-12")))
        .child(
            label(arg.to_string())
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
                .class(classes!("text-xs font-mono text-gray-11")),
        )
}

fn tool_card(
    name: &str,
    arg: &str,
    state: &ToolState,
    output: &str,
    tools: ToolOutput,
    layout: DiffLayout,
    app: &App,
) -> NodeBuilder {
    let card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(tool_header(name, arg, state));
    let Some(patch) = patch_of(output) else {
        return card.child(output_block(tools.text(output)));
    };
    let path = patch_path(output).unwrap_or_else(|| arg.to_string());
    let status = match state {
        ToolState::Running => "editing",
        ToolState::Ok => "✓ Applied",
        ToolState::Failed => "✗ Failed",
    };
    let sigil_layout = match layout {
        DiffLayout::Split => SigilDiffLayout::Split,
        DiffLayout::Unified => SigilDiffLayout::Unified,
    };
    let rows = if tools == ToolOutput::Compact {
        COMPACT_ROWS
    } else {
        usize::MAX
    };
    let pick = app.clone();
    card.child(
        diff(patch)
            .bg("field")
            .border_w(0)
            .path(path)
            .status(status)
            .layout(sigil_layout)
            .max_rows(rows)
            .choose(move |choice| {
                pick.set_diff_layout(match choice {
                    SigilDiffLayout::Split => DiffLayout::Split,
                    SigilDiffLayout::Unified => DiffLayout::Unified,
                });
            }),
    )
}

fn output_block(output: impl AsRef<str>) -> NodeBuilder {
    let output = output.as_ref();
    let mut column = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(0.0)
                .overflow_y(Overflow::Scroll),
        )
        .class(classes!("bg-gray-4"));
    for line in output.lines() {
        let class = if line.starts_with('+') {
            classes!("text-xs font-mono text-success-11")
        } else if line.starts_with('-') {
            classes!("text-xs font-mono text-error-11")
        } else if line.starts_with("@@") {
            classes!("text-xs font-mono text-info-11")
        } else {
            classes!("text-xs font-mono text-gray-11")
        };
        column = column.child(
            label(line.replace('\t', "    "))
                .ellipsize()
                .layout(
                    LayoutStyle::block()
                        .fill_width()
                        .padding_x(16.0)
                        .min_size(0.0, 0.0),
                )
                .class(class),
        );
    }
    column
}

fn prose(body: &str, class: &'static str) -> NodeBuilder {
    let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(0.0));
    for para in body.split('\n') {
        let mut row = container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .gap(8.0)
                    .align_items(AlignItems::Center)
                    .min_size(0.0, 0.0),
            )
            .class(classes!("flex-wrap"));
        let mut rest = para;
        while let Some(start) = rest.find('`') {
            if start > 0 {
                row = row.child(text(rest[..start].to_string()).class(class));
            }
            rest = &rest[start + 1..];
            let Some(end) = rest.find('`') else {
                break;
            };
            row = row.child(
                text(rest[..end].replace('\n', " "))
                    .class(classes!("text-xs font-mono text-info-11")),
            );
            rest = &rest[end + 1..];
        }
        if !rest.is_empty() {
            row = row.child(text(rest.to_string()).class(class));
        }
        column = column.child(row);
    }
    column
}
