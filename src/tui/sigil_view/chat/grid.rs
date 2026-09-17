use super::super::{
    app::App,
    input::{self, Input},
    model::{Agent, Model},
    suggest,
};
use super::chrome::{cache_card, context_card, header};
use super::composer::{self, Prompt, attachment_line, busy_line, empty_hint, prompt_rows};
use super::{
    Cell, Span, accent, agent_strip_rows, approval_row, body_rows, busy_status, canvas, cell_width,
    ellipsize, margin, palette, prompt_row, secret_row, write_line_at,
};

pub(super) fn grid(
    model: &Model,
    app: &App,
    draft: &str,
    cols: u16,
    rows: u16,
    attached: usize,
) -> Vec<Cell> {
    let width = usize::from(cols);
    let height = usize::from(rows);
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    if width == 0 || height == 0 {
        return cells;
    }
    let content_left = margin(width);
    let content_width = width.saturating_sub(content_left * 2);
    let accent = accent(&model.prefs.theme);
    write_line_at(
        &mut cells,
        width,
        content_left,
        content_width,
        0,
        &header(model, app.fps.get(), content_width, accent),
    );
    if height < 5 {
        return cells;
    }
    let prompt = usize::from(prompt_row(cols, rows, draft));
    let busy = busy_status(model);
    let strip = usize::from(agent_strip_rows(&model.agents, model.agents_hidden));
    let body_height =
        usize::from(body_rows(cols, rows, draft, busy.is_some())).saturating_sub(strip);
    let mut history = app.history.borrow_mut();
    let visible = history.lines(model, content_width, model.entries.stamp);
    let max_start = visible.len().saturating_sub(body_height);
    let start = ((app.scroll_y.peek() / canvas::cell().h) as usize).min(max_start);
    let shown = visible.len().saturating_sub(start).min(body_height);
    let lift = 0;
    if visible.is_empty() && body_height > 0 {
        let hint = empty_hint(content_width);
        let pad = content_width.saturating_sub(cell_width(hint)) / 2;
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            2,
            &[
                Span::text(" ".repeat(pad)),
                Span::styled(hint, palette().dim),
            ],
        );
    }
    for (offset, line) in visible.iter().skip(start).take(body_height).enumerate() {
        let y = 2 + lift + offset;
        let start = y * width + content_left;
        for cell in &mut cells[start..start + content_width] {
            cell.bg = line.bg;
        }
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            y,
            &line.spans,
        );
    }
    if !model.agents_hidden {
        stamp_agents(
            &mut cells,
            width,
            content_left,
            content_width,
            2 + lift + shown,
            &model.agents,
            model.spinner,
        );
        let live = model.agents.iter().filter(|agent| !agent.done).count();
        if live > 0 && content_width >= 12 {
            let row = 2 + lift + shown;
            app.hits.add(
                row,
                content_left + content_width.saturating_sub(4),
                content_left + content_width,
                super::super::hit::Target::AgentsClose,
            );
        }
    }
    let status_row = prompt.saturating_sub(1);
    if let Some(status) = &busy {
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            status_row,
            &busy_line(status, content_width, accent),
        );
    }
    if let Some((prompt, typed)) = app.secret.peek() {
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            status_row,
            &[Span::styled(
                secret_row(&prompt, typed, content_width),
                palette().yellow,
            )],
        );
    } else if let Some((detail, persist)) = app.approval.peek() {
        let row = approval_row(&detail, content_width, persist);
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            status_row,
            &[Span::styled(row.text, palette().yellow)],
        );
    } else if busy.is_none() && attached > 0 {
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            status_row,
            &attachment_line(attached),
        );
    }
    let editing = app
        .view_edit
        .peek()
        .then(|| model.view.as_ref().map(|view| view.path.clone()))
        .flatten();
    prompt_rows(Prompt {
        cells: &mut cells,
        stride: width,
        origin: content_left,
        width: content_width,
        top: prompt,
        model,
        input: &Input::new(draft, input::width(cols)),
        caret: (app.caret.peek(), app.placement.get()),
        ticks: app.ticks.peek(),
        editing: editing.as_deref(),
        selecting: app.select.peek(),
        selection: app.editor.selection(),
        focused: app.pane_active().is_none(),
    });
    let hover = app.meter_hover.peek();
    let open_ctx = app.context_open.peek();
    let open_cache = app.cache_open.peek();
    let card_bottom = prompt.saturating_sub(1).min(height.saturating_sub(3));
    let show_ctx = open_ctx || hover == Some(super::super::hit::Target::Context);
    let show_cache = open_cache || hover == Some(super::super::hit::Target::Cache);
    if show_ctx && !show_cache {
        stamp_card(
            &mut cells,
            width,
            content_left,
            content_width,
            card_bottom,
            " ctx ",
            &context_card(model),
        );
    }
    if show_cache && !show_ctx {
        stamp_card(
            &mut cells,
            width,
            content_left,
            content_width,
            card_bottom,
            " cache ",
            &cache_card(model),
        );
    }
    cells
}

fn stamp_agents(
    cells: &mut [Cell],
    stride: usize,
    left: usize,
    content: usize,
    top: usize,
    agents: &[Agent],
    spinner: usize,
) {
    let live: Vec<&Agent> = agents.iter().filter(|agent| !agent.done).collect();
    if live.is_empty() || content < 12 {
        return;
    }
    let count = live.len().min(6);
    let inner = content;
    let origin = left;
    let title = if live.len() == 1 {
        " 1 agent ".to_string()
    } else {
        format!(" {} agents ", live.len())
    };
    let close = " x ";
    let fill = inner.saturating_sub(cell_width(&title) + cell_width(close) + 2);
    let bg = palette().tool_bg;
    let border = palette().gray;
    write_line_at(
        cells,
        stride,
        origin,
        inner,
        top,
        &[
            Span::surface("╭", border, bg),
            Span::surface(title, palette().text, bg).bold(),
            Span::surface("─".repeat(fill), border, bg),
            Span::surface(close, palette().dim, bg),
            Span::surface("╮", border, bg),
        ],
    );
    for (offset, agent) in live.iter().take(count).enumerate() {
        let spin = composer::SPINNER[spinner % composer::SPINNER.len()];
        let body = ellipsize(
            &format!(" {spin} {}  {}", agent.id, agent.last),
            inner.saturating_sub(2),
        );
        let pad = inner.saturating_sub(cell_width(&body) + 2);
        write_line_at(
            cells,
            stride,
            origin,
            inner,
            top + 1 + offset,
            &[
                Span::surface("│", border, bg),
                Span::surface(body, palette().text, bg),
                Span::surface(" ".repeat(pad), palette().text, bg),
                Span::surface("│", border, bg),
            ],
        );
    }
    write_line_at(
        cells,
        stride,
        origin,
        inner,
        top + 1 + count,
        &[
            Span::surface("╰", border, bg),
            Span::surface("─".repeat(inner.saturating_sub(2)), border, bg),
            Span::surface("╯", border, bg),
        ],
    );
}

fn stamp_card(
    cells: &mut [Cell],
    stride: usize,
    left: usize,
    content: usize,
    bottom: usize,
    title: &str,
    lines: &[Vec<Span>],
) {
    let inner = content.clamp(20, 26);
    let origin = left + content.saturating_sub(inner);
    let height = lines.len() + 2;
    let top = bottom.saturating_sub(height);
    let bg = palette().tool_bg;
    let border = palette().gray;
    write_line_at(
        cells,
        stride,
        origin,
        inner,
        top,
        &[
            Span::surface("╭", border, bg),
            Span::surface(title, palette().dim, bg),
            Span::surface(
                "─".repeat(inner.saturating_sub(cell_width(title) + 2)),
                border,
                bg,
            ),
            Span::surface("╮", border, bg),
        ],
    );
    for (offset, line) in lines.iter().enumerate() {
        let used: usize = line.iter().map(Span::width).sum();
        let mut row = vec![
            Span::surface("│", border, bg),
            Span::surface(" ", palette().text, bg),
        ];
        row.extend(line.iter().cloned().map(|mut span| {
            span.bg = bg;
            span
        }));
        row.push(Span::surface(
            " ".repeat(inner.saturating_sub(used + 3)),
            palette().text,
            bg,
        ));
        row.push(Span::surface("│", border, bg));
        write_line_at(cells, stride, origin, inner, top + 1 + offset, &row);
    }
    write_line_at(
        cells,
        stride,
        origin,
        inner,
        top + 1 + lines.len(),
        &[
            Span::surface("╰", border, bg),
            Span::surface("─".repeat(inner.saturating_sub(2)), border, bg),
            Span::surface("╯", border, bg),
        ],
    );
}

pub(super) fn stamp_menu(cells: &mut [Cell], stride: usize, menu: &suggest::Menu) {
    for (offset, line) in menu.lines.iter().enumerate() {
        let y = menu.top.saturating_add(offset);
        let selected = line.chars().nth(1) == Some('›');
        let fg = if selected {
            palette().text
        } else {
            palette().gray
        };
        let bg = if selected {
            palette().user_bg
        } else {
            palette().code_bg
        };
        let mut span = Span::surface(line, fg, bg);
        span.bold = selected;
        write_line_at(
            cells,
            stride,
            menu.left,
            stride.saturating_sub(menu.left),
            y,
            &[span],
        );
    }
}
