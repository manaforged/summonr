use super::{App, Model, composer_hint, suggest};
use crate::tui::sigil_view::canvas::GUTTER;
use sigil::events::NodeEventsExt;
use sigil::layout::Overflow;
use sigil::layout::Position;
use sigil::prelude::AlignItems;
use sigil::prelude::IntoNode;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Paint;
use sigil::prelude::Tone;
use sigil::prelude::container;
use sigil::prelude::divider_horizontal;
use sigil::prelude::node;
use sigil::prelude::notice_band;

use crate::tui::sigil_view::ui::{Hints, Key};

pub(in crate::tui::sigil_view::chat) fn widget<E: IntoNode + 'static>(
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
                        .gap(8.0),
                )
                .child(
                    sigil::kernel::text("❯")
                        .nowrap()
                        .text_color(sigil::prelude::Token::OnSurface)
                        .font_size(12.0)
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
                .padding_top(8.0)
                .padding_x(GUTTER - 8.0)
                .gap(0.0),
        )
        .child(suggest::widget(app.clone(), width))
        .child(crate::tui::sigil_view::effort::strip(app.clone()))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .min_size(0.0, 0.0)
                        .padding_x(8.0)
                        .gap(0.0),
                )
                .bg(sigil::prelude::Token::Surface)
                .child(header_row(app.clone()))
                .child(editor_area)
                .child(hint_row(app.clone(), inner_width)),
        )
}

fn rule() -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_row().grow(1.0).min_size(0.0, 0.0))
        .child(divider_horizontal())
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
        "auto approval"
    };
    (format!("{glyph} {mode} · {approval}"), class as u8)
}

fn model_text(model: &Model) -> String {
    let suffix = model
        .snapshot
        .effort
        .filter(|effort| *effort != crate::provider::Effort::None)
        .map_or(String::new(), |effort| format!(" ({})", effort.as_str()));
    model.snapshot.seat_label(&suffix)
}

fn header_row(app: App) -> NodeBuilder {
    let update = app.clone();
    let labels = sigil::reactive::derived(move || {
        let model = update.model.get();
        let seat = if !model.snapshot.unseated {
            model_text(&model)
        } else {
            "no provider · /login".to_string()
        };
        (mode_text(&model), seat)
    });
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .children_reactive(move || {
            let (mode, model) = labels.get();
            vec![mode_label(mode), rule(), model_label(model)]
        })
}

fn hint_row(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let hint = sigil::reactive::derived(move || {
        let select = update.select.get();
        let model = update.model.get();
        let keys = &model.prefs.hotkeys;
        let select = select.then(|| keys.label(crate::prefs::Hotkey::Select));
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
                .gap(0.0),
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
                    sigil::kernel::text(text)
                        .nowrap()
                        .text_color(sigil::prelude::Token::OnSurface)
                        .font_size(12.0)
                        .text_color(sigil::prelude::Token::Gray11),
                ),
                node(rule()),
            ]
        })
}

fn mode_label(mode: ModeText) -> NodeBuilder {
    let (text, class) = mode;
    let title = sigil::kernel::text(text)
        .nowrap()
        .text_color(sigil::prelude::Token::OnSurface);
    let title = if class == 0 {
        title
            .font_size(12.0)
            .text_color(sigil::prelude::Token::Warn11)
    } else if class == 1 {
        title
            .font_size(12.0)
            .text_color(sigil::prelude::Token::Error11)
    } else {
        title
            .font_size(12.0)
            .text_color(sigil::prelude::Token::Success11)
    };
    container()
        .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0))
        .child(title.ellipsize())
}

fn model_label(model: String) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0))
        .child(
            sigil::kernel::text(model)
                .nowrap()
                .ellipsize()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        )
}
