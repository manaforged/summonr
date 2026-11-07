use super::{App, Model, composer_hint, suggest};
use sigil::prelude::AlignItems;
use sigil::prelude::IntoNode;
use sigil::prelude::LayoutStyle;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Overflow;
use sigil::prelude::Paint;
use sigil::prelude::Position;
use sigil::prelude::Space;
use sigil::prelude::Tone;
use sigil::prelude::container;
use sigil::prelude::divider_horizontal;
use sigil::prelude::node;
use sigil::prelude::notice_band;

use crate::ui::{Hints, Key};
use sigil::prelude::Widget;

pub(in crate::chat) fn widget<E: IntoNode + 'static>(
    app: App,
    width: usize,
    editor: E,
) -> NodeBuilder {
    let editing = app.view_edit.peek();
    let focus = app.editor_focus;
    let mut editor_area = container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .min_size(0.0, 0.0)
            .position(Position::Relative),
    );
    if editing {
        let model = app.model.get();
        let path = model.view.as_ref().map_or("", |view| view.path.as_str());
        editor_area = editor_area
            .child(notice_band(format!("editing {path}"), Tone::Info))
            .child(editor);
    } else {
        editor_area = editor_area.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .min_size(0.0, 0.0)
                        .align_items(AlignItems::Start)
                        .gap(Space::Sm),
                )
                .child(
                    sigil::prelude::text("❯")
                        .nowrap()
                        .font_size(
                            12.0,
                            "12 px chrome text that keeps its own line height and weight",
                        )
                        .text_color(sigil::prelude::Token::Gray12),
                )
                .child(
                    container()
                        .layout(
                            LayoutStyle::flex_column()
                                .grow(1.0)
                                .min_size(0.0, 0.0)
                                .overflow_y(Overflow::Hidden),
                        )
                        .child(editor),
                ),
        );
    }
    let inner_width = width.saturating_sub(8);
    container()
        .pointer_down(move |_| focus.update(|n| *n += 1))
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .padding_top(Space::Sm)
                .padding_x(Space::Sm)
                .gap(Space::None),
        )
        .child(suggest::widget(app.clone(), width))
        .child(crate::effort::strip(app.clone()))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .min_size(0.0, 0.0)
                        .padding_x(Space::Sm)
                        .gap(Space::None),
                )
                .bg(sigil::prelude::Token::Surface)
                .child(header_row(app.clone()))
                .child(pending_rows(&app, inner_width))
                .child(editor_area)
                .child(hint_row(app.clone(), inner_width)),
        )
}

fn pending_rows(app: &App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let pending = sigil::reactive::derived(move || update.model.get().pending.clone());
    container()
        .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
        .children_reactive(move || {
            let rows = pending.get();
            let room = summonr_agent::limits::limits().queue_rows;
            let mut lines: Vec<String> = rows
                .iter()
                .take(room)
                .map(|(next, text)| {
                    let label = if *next { "next" } else { "after turn" };
                    let first = text.lines().next().unwrap_or_default();
                    crate::cells::ellipsize(
                        &format!("{label} · {first}"),
                        summonr_agent::display::Span::Cells(width),
                    )
                })
                .collect();
            if rows.len() > room {
                lines.push(format!("+{} more", rows.len() - room));
            }
            lines.into_iter().map(pending_line).collect::<Vec<_>>()
        })
}

fn pending_line(line: String) -> NodeBuilder {
    container().child(
        sigil::prelude::text(line)
            .nowrap()
            .font_size(
                12.0,
                "12 px chrome text that keeps its own line height and weight",
            )
            .text_color(sigil::prelude::Token::Gray11),
    )
}

fn rule() -> NodeBuilder {
    if crate::canvas::is_window() {
        return gap();
    }
    container()
        .layout(LayoutStyle::flex_row().grow(1.0).min_size(0.0, 0.0))
        .child(divider_horizontal())
}

fn gap() -> NodeBuilder {
    container().layout(LayoutStyle::flex_row().grow(1.0).min_size(0.0, 0.0))
}

type ModeText = (String, u8);

fn mode_text(model: &Model) -> ModeText {
    let class: usize = if model.snapshot.work {
        0
    } else if model.snapshot.goal {
        1
    } else {
        2
    };
    let mode = model.snapshot.mode_name();
    let glyph = ["▣", "◎", "●"][class];
    let approval = if model.snapshot.strict {
        "strict"
    } else if model.snapshot.approval {
        "approval"
    } else {
        ""
    };
    let text = if approval.is_empty() {
        format!("{glyph} {mode}")
    } else if class == 2 {
        format!("{glyph} {approval}")
    } else {
        format!("{glyph} {mode} · {approval}")
    };
    (text, class as u8)
}

fn model_text(model: &Model) -> String {
    let suffix = model
        .snapshot
        .effort
        .filter(|effort| *effort != summonr_agent::provider::Effort::None)
        .map_or(String::new(), |effort| format!(" ({})", effort.as_str()));
    model.snapshot.model_label(&suffix)
}

fn header_row(app: App) -> NodeBuilder {
    let update = app.clone();
    let labels = sigil::reactive::derived(move || {
        let model = update.model.get();
        let selection = if !model.snapshot.unconfigured {
            model_text(&model)
        } else {
            "no provider · /login".to_string()
        };
        (mode_text(&model), selection)
    });
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .align_items(AlignItems::Center)
                .gap(Space::Sm),
        )
        .children_reactive(move || {
            let (mode, model) = labels.get();
            vec![mode_label(mode), rule(), model_label(model)]
        })
}

fn hint_row(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let hint = sigil::reactive::derived(move || {
        if update.hotkeys.armed.get() {
            return " ctrl+x ".to_string();
        }
        let note = update.hotkeys.note.get();
        if !update.hotkeys.open.get() && !note.is_empty() {
            return note;
        }
        let select = update.select.get();
        let model = update.model.get();
        let keys = &model.prefs.hotkeys;
        let select = select.then(|| keys.label(summonr_agent::prefs::Hotkey::Select));
        if update.view_edit.get() {
            Hints::new()
                .key(Key::Fixed("ctrl+s"), "save")
                .key(Key::Esc, "leave")
                .text(keys)
        } else {
            composer_hint(
                model.busy_since.map(|(activity, _)| activity),
                width,
                select.as_deref(),
            )
            .trim()
            .to_string()
        }
    });
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .align_items(AlignItems::Center)
                .gap(Space::None),
        )
        .children_reactive(move || {
            let text = hint.get();
            let text = if text.is_empty() {
                text
            } else {
                format!(" {text} ")
            };
            vec![
                node(rule()),
                node(
                    sigil::prelude::text(text)
                        .nowrap()
                        .font_size(
                            12.0,
                            "12 px chrome text that keeps its own line height and weight",
                        )
                        .text_color(sigil::prelude::Token::Gray11),
                ),
                node(rule()),
            ]
        })
}

fn mode_label(mode: ModeText) -> NodeBuilder {
    let (text, class) = mode;
    let tone = [
        sigil::prelude::Token::Warn11,
        sigil::prelude::Token::Error11,
        sigil::prelude::Token::Success11,
    ][usize::from(class).min(2)];
    let title = sigil::prelude::text(text)
        .nowrap()
        .font_size(
            12.0,
            "12 px chrome text that keeps its own line height and weight",
        )
        .text_color(tone);
    container()
        .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0))
        .child(title.ellipsize())
}

fn model_label(model: String) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0))
        .child(
            sigil::prelude::text(model)
                .nowrap()
                .ellipsize()
                .font_size(
                    12.0,
                    "12 px chrome text that keeps its own line height and weight",
                )
                .text_color(sigil::prelude::Token::Gray12),
        )
}
