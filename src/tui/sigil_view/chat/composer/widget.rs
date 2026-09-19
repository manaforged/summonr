use sigil::prelude::{IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, Position, container};
use sigil_tui::cell_width;

use super::super::super::app::App;
use super::super::super::canvas::{self, Span, palette};
use super::super::super::model::Model;
use super::super::super::suggest;
use super::super::{accent, composer_hint, render_background, run};
use super::margin;

pub(in crate::tui::sigil_view::chat) fn widget<E: IntoNode + 'static>(
    app: App,
    full_width: usize,
    width: usize,
    editor: E,
) -> NodeBuilder {
    let header_app = app.clone();
    let hint_app = app.clone();
    let editing = app.view_edit.peek();
    let mut editor_area = container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .min_size(0.0, 0.0)
            .position(Position::Relative),
    );
    if editing {
        let model = app.model.borrow();
        let path = model.view.as_ref().map_or("", |view| view.path.as_str());
        let text = canvas::ellipsize(
            &format!("editing {path} · ctrl+s save · esc leave"),
            width.saturating_sub(4),
        );
        let text = format!(
            "│ {text}{} │",
            " ".repeat(width.saturating_sub(cell_width(&text) + 4))
        );
        editor_area = editor_area
            .child(render_background(
                [Span::styled(text, palette().gray)].iter().map(run),
                palette().base,
            ))
            .child(editor);
    } else {
        editor_area = editor_area.child(editor);
    }
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .child(suggest::widget(app.clone(), width, full_width))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .height(canvas::cell().h),
                )
                .children_reactive(move || {
                    let _panes = header_app.panes.get();
                    let model = header_app.model.borrow();
                    vec![row(header_line(&model, width), palette().base, full_width)]
                }),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .shrink(0.0)
                        .min_size(0.0, 0.0)
                        .padding_x(margin(full_width) as f32 * canvas::cell().w),
                )
                .child(editor_area),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .height(canvas::cell().h),
                )
                .children_reactive(move || {
                    let _panes = hint_app.panes.get();
                    let _select = hint_app.select.get();
                    let model = hint_app.model.borrow();
                    let options = model.prefs.hotkeys.label(crate::prefs::Hotkey::Options);
                    let select = hint_app
                        .select
                        .peek()
                        .then(|| model.prefs.hotkeys.label(crate::prefs::Hotkey::Select));
                    let hint = if hint_app.view_edit.get() {
                        " ctrl+s save · esc leave "
                    } else {
                        &composer_hint(
                            model.busy_since.map(|(activity, _)| activity),
                            width,
                            &options,
                            select.as_deref(),
                        )
                    };
                    vec![row(hint_line(hint, width), palette().base, full_width)]
                }),
        )
}

fn header_line(model: &Model, width: usize) -> Vec<Span> {
    let (glyph, mode, mode_color) = if model.snapshot.plan {
        ('◇', "plan", palette().cyan)
    } else {
        ('●', "auto", palette().green)
    };
    let left = format!(" {glyph} {mode} ");
    let suffix = model
        .snapshot
        .effort
        .filter(|effort| *effort != crate::provider::Effort::None)
        .map_or(String::new(), |effort| format!("({})", effort.as_str()));
    let model_width = width.saturating_sub(cell_width(&left) + cell_width(&suffix) + 3);
    let shown_model = canvas::ellipsize(&model.snapshot.model, model_width);
    let fill = width
        .saturating_sub(cell_width(&left) + cell_width(&shown_model) + cell_width(&suffix) + 3);
    let border = if model.busy_since.is_some() {
        accent(&model.prefs.theme)
    } else {
        palette().gray
    };
    vec![
        Span::styled("╭", border),
        Span::styled(left, mode_color),
        Span::styled("─".repeat(fill), border),
        Span::styled(shown_model, palette().dim),
        Span::styled(suffix, palette().yellow).bold(),
        Span::text(" "),
        Span::styled("╮", border),
    ]
}

fn hint_line(hint: &str, width: usize) -> Vec<Span> {
    let hint = canvas::ellipsize(hint, width.saturating_sub(2));
    let dash = width.saturating_sub(cell_width(&hint) + 2);
    vec![
        Span::styled("╰", palette().gray),
        Span::styled("─".repeat(dash), palette().gray),
        Span::styled(hint, palette().gray),
        Span::styled("╯", palette().gray),
    ]
}

fn row(spans: Vec<Span>, background: sigil::prelude::Rgba8, width: usize) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::cell().h)
                .padding_x(margin(width) as f32 * canvas::cell().w),
        )
        .child(render_background(spans.iter().map(run), background))
}
