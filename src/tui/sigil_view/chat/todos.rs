use super::super::{app::App, model::Model, panes::ledger::item};
use super::{Cell, Span, body_rows, cell_width, ellipsize, margin, palette, write_line_at};

pub(in crate::tui::sigil_view) fn height(model: &Model, available: u16) -> u16 {
    if available < 4 || model.ledger.iter().all(|line| item(line).0 == '✓') {
        return 0;
    }
    if model.todos_expanded {
        2 + model
            .ledger
            .len()
            .min(6)
            .min(usize::from(available / 2 - 2)) as u16
    } else {
        1
    }
}

pub(super) fn draw(cells: &mut [Cell], model: &Model, app: &App, cols: u16, rows: u16, top: usize) {
    let reserved = height(
        model,
        body_rows(cols, rows, &app.draft.peek(), model.busy_since.is_some())
            .saturating_sub(super::agent_strip_rows(&model.agents, model.agents_hidden)),
    );
    if reserved == 0 {
        return;
    }
    let stride = usize::from(cols);
    let left = margin(stride);
    let width = stride.saturating_sub(left * 2);
    let done = model
        .ledger
        .iter()
        .filter(|line| item(line).0 == '✓')
        .count();
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
    let title = format!(" Tasks {done}/{}  ", model.ledger.len());
    let action = if model.todos_expanded {
        " Hide list ⌃ "
    } else {
        " Show list ⌄ "
    };
    let room = width.saturating_sub(cell_width(&title) + cell_width(action));
    let text = current.map_or(String::new(), |(mark, text)| {
        ellipsize(&format!("{mark} {text}"), room)
    });
    let bg = palette().tool_bg;
    let line = [
        Span::surface(title, palette().dim, bg),
        Span::surface(&text, palette().text, bg),
        Span::surface(
            " ".repeat(room.saturating_sub(cell_width(&text))),
            palette().dim,
            bg,
        ),
        Span::surface(action, palette().accent, bg),
    ];
    write_line_at(cells, stride, left, width, top, &line);
    let count = usize::from(reserved).saturating_sub(2);
    let start = model
        .todos_offset
        .min(model.ledger.len().saturating_sub(count));
    for (index, task) in model.ledger.iter().skip(start).take(count).enumerate() {
        let (mark, text) = item(task);
        let fg = match mark {
            '✓' => palette().dim,
            '●' => palette().accent,
            '!' => palette().error,
            _ => palette().text,
        };
        let text = ellipsize(&format!(" {mark} {text}"), width);
        let padding = " ".repeat(width.saturating_sub(cell_width(&text)));
        write_line_at(
            cells,
            stride,
            left,
            width,
            top + index + 1,
            &[Span::surface(format!("{text}{padding}"), fg, bg)],
        );
    }
    if count > 0 && model.ledger.len() > count {
        let text = ellipsize(
            &format!(
                " {}–{} of {} · scroll for more",
                start + 1,
                start + count,
                model.ledger.len()
            ),
            width,
        );
        write_line_at(
            cells,
            stride,
            left,
            width,
            top + count + 1,
            &[Span::styled(text, palette().dim)],
        );
    }
}

impl App {
    pub(in crate::tui::sigil_view) fn history_rows(&self, draft: &str) -> u16 {
        let model = self.model.borrow();
        let available = body_rows(
            self.chat_cols(),
            super::overlay_rows(self.viewport.peek().1),
            draft,
            model.busy_since.is_some(),
        )
        .saturating_sub(super::agent_strip_rows(&model.agents, model.agents_hidden));
        available.saturating_sub(height(&model, available))
    }

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

    pub(in crate::tui::sigil_view) fn todo_hit(&self, col: usize, row: usize) -> Option<bool> {
        let cols = self.chat_cols();
        let model = self.model.borrow();
        let available = body_rows(
            cols,
            super::overlay_rows(self.viewport.peek().1),
            &self.draft.peek(),
            model.busy_since.is_some(),
        );
        let agents = super::agent_strip_rows(&model.agents, model.agents_hidden);
        let count = usize::from(height(&model, available.saturating_sub(agents)));
        let top = 2 + usize::from(available).saturating_sub(count);
        let left = margin(usize::from(cols));
        (count > 0
            && (left..usize::from(cols).saturating_sub(left)).contains(&col)
            && (top..top + count).contains(&row))
        .then_some(row == top)
    }

    pub(in crate::tui::sigil_view) fn scroll_todos(&self, delta: isize) {
        let mut model = self.model.borrow_mut();
        let available = body_rows(
            self.chat_cols(),
            super::overlay_rows(self.viewport.peek().1),
            &self.draft.peek(),
            model.busy_since.is_some(),
        )
        .saturating_sub(super::agent_strip_rows(&model.agents, model.agents_hidden));
        let count = usize::from(height(&model, available).saturating_sub(2)).max(1);
        model.todos_offset = model
            .todos_offset
            .saturating_add_signed(delta)
            .min(model.ledger.len().saturating_sub(count));
        drop(model);
        self.refresh();
    }
}
