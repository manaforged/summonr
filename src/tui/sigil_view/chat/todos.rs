use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, ScrollDelta, container,
};
use sigil_tui::cell_width;

use super::super::app::App;
use super::super::canvas::{Span, palette};
use super::super::model::Model;
use super::super::panes::ledger::item;
use super::{render_background, run};

pub(super) fn widget(app: App, cols: u16) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .padding_x(
                    super::margin(usize::from(cols)) as f32 * super::super::canvas::cell().w,
                ),
        )
        .children_reactive(move || {
            let _panes = update.panes.get();
            let _viewport = update.viewport.get();
            let draft = update.draft.get();
            let caret = update.caret.get();
            let model = update.model.borrow();
            let width = content_width(cols);
            if width == 0
                || model.ledger.iter().all(|line| item(line).0 == '✓')
                || update.commands.open(&draft, caret, &model)
            {
                return Vec::new();
            }
            vec![panel(update.clone(), &model, width)]
        })
}

fn panel(app: App, model: &Model, width: usize) -> NodeBuilder {
    let expanded = model.todos_expanded;
    let done = model
        .ledger
        .iter()
        .filter(|line| item(line).0 == '✓')
        .count();
    let title = format!(" Tasks {done}/{}  ", model.ledger.len());
    let action = if expanded {
        " Hide list ⌃ "
    } else {
        " Show list ⌄ "
    };
    let room = width.saturating_sub(cell_width(&title) + cell_width(action));
    let current = model
        .ledger
        .iter()
        .map(|line| item(line))
        .find(|(mark, _)| *mark == '●')
        .or_else(|| {
            model
                .ledger
                .iter()
                .map(|line| item(line))
                .find(|(mark, _)| *mark != '✓')
        });
    let text = current.map_or(String::new(), |(mark, text)| {
        super::super::canvas::ellipsize(&format!("{mark} {text}"), room)
    });
    let bg = palette().tool_bg;
    let mut title_line = vec![
        Span::surface(title, palette().dim, bg),
        Span::surface(&text, palette().text, bg),
        Span::surface(
            " ".repeat(room.saturating_sub(cell_width(&text))),
            palette().dim,
            bg,
        ),
        Span::surface(action, palette().accent, bg),
    ];
    trim_line(&mut title_line, width);
    let toggle = app.clone();
    let title_node = render_background(title_line.iter().map(run), bg).pointer_down(move |event| {
        if event.button == Some(sigil::prelude::MouseButton::Left) {
            toggle.toggle_todos();
            event.stop_propagation();
        }
    });
    let mut body = container()
        .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
        .child(title_node);
    if expanded {
        let count = visible_rows(model, app.viewport.peek().1);
        let start = model
            .todos_offset
            .min(model.ledger.len().saturating_sub(count));
        for task in model.ledger.iter().skip(start).take(count) {
            let (mark, text) = item(task);
            let fg = match mark {
                '✓' => palette().dim,
                '●' => palette().accent,
                '!' => palette().error,
                _ => palette().text,
            };
            let text = super::super::canvas::ellipsize(&format!(" {mark} {text}"), width);
            let line = format!(
                "{text}{}",
                " ".repeat(width.saturating_sub(cell_width(&text)))
            );
            body = body.child(render_background(
                [Span::surface(line, fg, bg)].iter().map(run),
                bg,
            ));
        }
        if model.ledger.len() > count {
            let note = super::super::canvas::ellipsize(
                &format!(
                    " {}–{} of {} · scroll for more",
                    start + 1,
                    start + count,
                    model.ledger.len()
                ),
                width,
            );
            let line = format!(
                "{note}{}",
                " ".repeat(width.saturating_sub(cell_width(&note)))
            );
            body = body.child(render_background(
                [Span::surface(line, palette().dim, bg)].iter().map(run),
                palette().base,
            ));
        }
    }
    let scroll = app.clone();
    body.on_scroll(move |event| {
        let delta = match event.delta {
            ScrollDelta::Pixels(point) => point.y / super::super::canvas::cell().h,
            ScrollDelta::Lines(point) => point.y,
        };
        scroll.scroll_todos(-delta.round() as isize);
        event.stop_propagation();
    })
}

fn trim_line(line: &mut Vec<Span>, width: usize) {
    let mut used = 0;
    for span in line {
        super::super::canvas::truncate(&mut span.text, width.saturating_sub(used));
        used += span.width();
    }
}

fn content_width(cols: u16) -> usize {
    let width = usize::from(cols);
    width.saturating_sub(super::margin(width) * 2)
}

fn visible_rows(model: &Model, rows: u16) -> usize {
    model
        .ledger
        .len()
        .min(usize::from(rows / 3).saturating_sub(2).clamp(1, 6))
}

impl App {
    pub(in crate::tui::sigil_view) fn toggle_todos(&self) {
        let mut model = self.model.borrow_mut();
        if model.ledger.iter().all(|line| item(line).0 == '✓') {
            drop(model);
            self.toggle_pane(super::super::pane::Pane::Ledger);
            return;
        }
        model.todos_expanded = !model.todos_expanded;
        model.todos_offset = model
            .ledger
            .iter()
            .position(|line| item(line).0 == '●')
            .unwrap_or(0)
            .saturating_sub(1);
        drop(model);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn scroll_todos(&self, delta: isize) -> bool {
        let mut model = self.model.borrow_mut();
        let count = if model.todos_expanded {
            visible_rows(&model, self.viewport.peek().1)
        } else {
            1
        };
        let before = model.todos_offset;
        model.todos_offset = model
            .todos_offset
            .saturating_add_signed(delta)
            .min(model.ledger.len().saturating_sub(count));
        let changed = model.todos_offset != before;
        drop(model);
        if changed {
            self.refresh();
        }
        changed
    }
}
