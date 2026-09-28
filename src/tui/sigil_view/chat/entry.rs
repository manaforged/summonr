use sigil::prelude::{
    AlignItems, DiffBuilder, DiffLayout as SigilDiffLayout, ImageSource, IntoNode,
    LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt,
    ObjectFit, Overflow, PointerEvent, WidgetLayoutExt, chart, classes, container, diff,
    icon_button, image, label, markdown, markdown_inline, signal, text,
};

use super::{App, Call, Entry};
use crate::prefs::{DiffLayout, Prefs, Visibility};

mod preview;
mod tools;

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
            name,
            arg,
            state,
            output,
            expanded,
            ..
        } => tools::tool_card(
            &name,
            &Call {
                id: String::new(),
                arg,
                state,
                output,
                expanded,
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
            .class(classes!("bg-gray-5"))
            .child(text(format!("▌ {body}")).class(classes!("text-sm text-gray-12"))),
        Entry::Assistant { text: body, .. } => assistant(body, app),
        Entry::Thinking(body) => collapsible(&body, "thinking", prefs.reasoning, app, true, live),
        Entry::Receipt(body) => collapsible(&body, "receipt", prefs.receipts, app, false, false),
        Entry::Verify { command, passed } => verify(&command, passed),
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
        } => picture(source, bytes, name),
        Entry::Tool { .. } | Entry::ToolGroup { .. } => container(),
    }
}

fn assistant(body: String, app: &App) -> NodeBuilder {
    let copy_app = app.clone();
    let copied = body.clone();
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
                    .child(markdown(signal(body))),
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
        label(format!("{} {command}", if passed { "✓" } else { "✗" })).class(if passed {
            classes!("text-xs text-success-11")
        } else {
            classes!("text-xs text-error-11")
        }),
    )
}

fn picture(source: ImageSource, bytes: std::rc::Rc<[u8]>, name: String) -> NodeBuilder {
    let (width, height) = (source.width() as f32, source.height() as f32);
    let id = source.id();
    let kind = crate::tools::image_type(&bytes).and_then(|media| media.strip_prefix("image/"));
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
        Visibility::Collapsed if reasoning => {
            let words = body.split_whitespace().count();
            wrap(
                label(format!("⌄ {kind} · {words} words"))
                    .class(classes!("text-xs italic text-gray-11")),
            )
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
            .child(prose(body, "text-xs italic text-gray-10"))
            .pointer_down(press),
    }
}

fn wrap(child: impl IntoNode + 'static) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
        .child(child)
}

fn patch_of(output: &str) -> Option<String> {
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

pub(in crate::tui::sigil_view) fn patch_view(
    app: &App,
    patch: impl Into<String>,
    layout: DiffLayout,
) -> DiffBuilder {
    let pick = app.clone();
    diff(patch)
        .layout(match layout {
            DiffLayout::Split => SigilDiffLayout::Split,
            DiffLayout::Unified => SigilDiffLayout::Unified,
        })
        .choose(move |choice| {
            pick.set_diff_layout(match choice {
                SigilDiffLayout::Split => DiffLayout::Split,
                SigilDiffLayout::Unified => DiffLayout::Unified,
            });
        })
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
        column = column.child(markdown_inline(para).class(class));
    }
    column
}
