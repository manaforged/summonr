use sigil_tui::cell_width;
use unicode_segmentation::UnicodeSegmentation;

use super::app::App;
use super::canvas::{Cell, palette};
use super::chat::row;
use super::input::Input;
use super::pane::Pane;
use super::patch::{self, Kind, SPLIT};
use sigil::prelude::{
    ImageSource, LayoutBuilderExt, LayoutStyle, NodeBuilder, ObjectFit, Position, Rgba8, container,
    image,
};

#[derive(Clone)]
struct Line {
    text: String,
    fg: Rgba8,
    bg: Rgba8,
}

impl Line {
    fn new(text: impl Into<String>, fg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg: palette().base,
        }
    }

    fn tinted(text: impl Into<String>, fg: Rgba8, bg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg,
        }
    }
}

pub(super) fn body(app: &App, pane: Pane, cols: u16, rows: u16) -> NodeBuilder {
    let app = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .width(f32::from(cols) * super::canvas::cell().w)
                .fill_height()
                .gap(0.0),
        )
        .children_reactive(move || {
            let _version = app.panes.get();
            let mut children = Vec::new();
            let width = usize::from(cols);
            app.hits.clear();
            let mut lines: Vec<Vec<Line>> = match pane {
                Pane::Ledger => single(ledger(&app)),
                Pane::Files => single(files(&app, usize::from(rows))),
                Pane::Diff => diff(&app, width, usize::from(rows)),
                Pane::Worktrees => single(trees(&app, usize::from(rows))),
                Pane::View => single(view(&app, usize::from(rows))),
                Pane::Chat => Vec::new(),
            };
            close_header(&mut lines, width);
            let left = usize::from(app.chat_cols()) + 1;
            app.hits.add(
                0,
                left + width.saturating_sub(1),
                left + width,
                super::hit::Target::Close,
            );
            let caret = (pane == Pane::View && app.view_edit.get()).then(|| {
                let body = usize::from(rows).saturating_sub(2);
                let window = super::editor::window(
                    &app.view_text.get(),
                    app.view_caret.get(),
                    app.view_scroll.get(),
                    body,
                );
                (window.line - window.start + 2, 7 + window.col)
            });
            for index in 0..usize::from(rows) {
                let spans: &[Line] = lines.get(index).map_or(&[], Vec::as_slice);
                let at = caret.filter(|(row, _)| *row == index).map(|(_, col)| col);
                children.push(row(&cells(spans, width, at)));
            }
            if pane == Pane::View
                && let Some(source) = app
                    .model
                    .borrow()
                    .view
                    .as_ref()
                    .and_then(|view| view.image.clone())
            {
                children.push(picture(&source, width, usize::from(rows)));
            }
            children
        })
}

fn picture(source: &ImageSource, width: usize, rows: usize) -> NodeBuilder {
    let (cols, tall) = super::chat::transcript::image_cells(source, width.saturating_sub(2));
    let tall = tall.min(rows.saturating_sub(3)).max(1);
    let source = super::chat::transcript::paint_source(source, cols, tall);
    let cell = super::canvas::cell();
    container()
        .layout(
            LayoutStyle::block()
                .position(Position::Absolute)
                .inset_left(cell.w)
                .inset_top(2.0 * cell.h)
                .width(cols as f32 * cell.w)
                .height(tall as f32 * cell.h),
        )
        .child(
            image(source.clone())
                .fit(ObjectFit::Contain)
                .layout(LayoutStyle::block().fill_width().fill_height()),
        )
}

fn single(lines: Vec<Line>) -> Vec<Vec<Line>> {
    lines.into_iter().map(|line| vec![line]).collect()
}

fn close_header(lines: &mut [Vec<Line>], width: usize) {
    let Some(header) = lines.first_mut() else {
        return;
    };
    let Some(Line {
        text: title,
        fg: color,
        ..
    }) = header.first().cloned()
    else {
        return;
    };
    let limit = width.saturating_sub(4);
    let title: String = title.chars().take(limit).collect();
    let gap = width.saturating_sub(title.chars().count() + 3);
    *header = vec![
        Line::new(title, color),
        Line::new(" ".repeat(gap), palette().text),
        Line::new("×".to_string(), palette().dim),
    ];
}

fn ledger(app: &App) -> Vec<Line> {
    let model = app.model.borrow();
    let accent = super::chat::accent(&model.prefs.theme);
    let done = model
        .ledger
        .iter()
        .filter(|line| line.trim_start().starts_with("[x] ") || line.trim_start().starts_with("✓ "))
        .count();
    let total = model.ledger.len();
    let title = if total == 0 {
        "todos".to_string()
    } else {
        format!("todos · {done}/{total}")
    };
    let mut lines = vec![
        Line::new(title, accent),
        Line::new(String::new(), palette().text),
    ];
    if model.ledger.is_empty() {
        lines.push(Line::new("no todos yet".to_string(), palette().dim));
        return lines;
    }
    for line in &model.ledger {
        lines.push(plan_line(line, accent));
    }
    lines
}

fn plan_line(line: &str, accent: Rgba8) -> Line {
    let indent = line.len() - line.trim_start().len();
    let body = line.trim_start();
    let (mark, rest, fg) = if let Some(rest) = body.strip_prefix("[x] ") {
        ("✓ ", rest, palette().dim)
    } else if let Some(rest) = body.strip_prefix("[/] ") {
        ("● ", rest, accent)
    } else if let Some(rest) = body.strip_prefix("[ ] ") {
        ("○ ", rest, palette().text)
    } else if let Some(rest) = body.strip_prefix("[!] ") {
        ("! ", rest, palette().error)
    } else {
        return Line::new(line.to_string(), palette().dim);
    };
    Line::new(format!("{}{mark}{rest}", " ".repeat(indent)), fg)
}

fn files(app: &App, rows: usize) -> Vec<Line> {
    let model = app.model.borrow();
    let accent = super::chat::accent(&model.prefs.theme);
    let focused = app.pane_focus.get();
    let repo = app
        .workspace
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let branch = app
        .git_branch
        .as_deref()
        .map_or(String::new(), |branch| format!(" · {branch}"));
    let mut lines = vec![
        Line::new(
            format!("files · {repo}{branch}"),
            if focused { accent } else { palette().text },
        ),
        Line::new(String::new(), palette().text),
    ];
    let tree = app.file_rows();
    if tree.is_empty() {
        lines.push(Line::new("no files listed yet".to_string(), palette().dim));
        return lines;
    }
    let cursor = app.file_cursor.get().min(tree.len() - 1);
    let body = rows.saturating_sub(3).max(1);
    let start = cursor
        .saturating_sub(body / 2)
        .min(tree.len().saturating_sub(body));
    let left = usize::from(app.chat_cols()) + 1;
    for (index, row) in tree.iter().enumerate().skip(start).take(body) {
        app.hits.add(
            index - start + 2,
            left,
            left + 6 + row.depth * 2 + row.name.chars().count(),
            super::hit::Target::File(index),
        );
        let marker = if index == cursor && focused {
            "›"
        } else {
            " "
        };
        let glyph = if row.dir {
            if row.open { "▾ " } else { "▸ " }
        } else {
            "  "
        };
        let color = if index == cursor && focused {
            accent
        } else if row.dir {
            palette().text
        } else {
            palette().dim
        };
        lines.push(Line::new(
            format!("{marker}{}{glyph}{}", "  ".repeat(row.depth), row.name),
            color,
        ));
    }
    overlay(app, &mut lines, accent);
    lines
}

fn diff(app: &App, width: usize, rows: usize) -> Vec<Vec<Line>> {
    let model = app.model.borrow();
    let accent = super::chat::accent(&model.prefs.theme);
    let focused = app.pane_focus.get();
    let head = if focused { accent } else { palette().text };
    if let Some(error) = &model.diff_error {
        let mut lines = vec![
            Line::new("diff unavailable".to_string(), palette().red),
            Line::new(String::new(), palette().text),
        ];
        lines.extend(
            Input::new(error, width)
                .rows
                .into_iter()
                .map(|range| Line::new(error[range].to_string(), palette().dim)),
        );
        lines.push(Line::new(String::new(), palette().text));
        lines.push(Line::new("reopen Diff to retry".to_string(), palette().dim));
        return single(lines);
    }
    if let Some(path) = app.diff_view.get() {
        let counts = model
            .diff
            .files
            .iter()
            .find(|stat| stat.path == path)
            .map_or(String::new(), |stat| {
                format!(" · +{} −{}", stat.added, stat.removed)
            });
        let mut lines = vec![
            Line::new(format!("diff · {path}{counts}"), head),
            Line::new(String::new(), palette().text),
        ];
        match model.patch.as_ref().filter(|(seen, _)| *seen == path) {
            None => lines.push(Line::new("loading…".to_string(), palette().dim)),
            Some((_, Err(error))) => {
                lines.push(Line::new("patch unavailable".to_string(), palette().red));
                lines.extend(
                    Input::new(error, width)
                        .rows
                        .into_iter()
                        .skip(app.diff_scroll.get())
                        .take(rows.saturating_sub(3))
                        .map(|range| Line::new(error[range].to_string(), palette().dim)),
                );
            }
            Some((_, Ok(text))) if text.is_empty() => {
                lines.push(Line::new("no changes".to_string(), palette().dim));
            }
            Some((_, Ok(text))) if width >= SPLIT => {
                let mut out = single(lines);
                let scroll = app.diff_scroll.get();
                let half = width.saturating_sub(5) / 2;
                for pair in patch::pairs(text)
                    .into_iter()
                    .skip(scroll)
                    .take(rows.saturating_sub(2))
                {
                    out.push(split(&pair, half));
                }
                return out;
            }
            Some((_, Ok(text))) => {
                let scroll = app.diff_scroll.get();
                for line in text.lines().skip(scroll).take(rows.saturating_sub(2)) {
                    let (fg, bg) = if line.starts_with("+++") || line.starts_with("---") {
                        (palette().dim, palette().base)
                    } else if line.starts_with('+') {
                        (palette().green, palette().diff_add_bg)
                    } else if line.starts_with('-') {
                        (palette().red, palette().diff_del_bg)
                    } else if line.starts_with("@@") {
                        (palette().cyan, palette().base)
                    } else if line.starts_with("diff ") || line.starts_with("index ") {
                        (palette().dim, palette().base)
                    } else {
                        (palette().text, palette().base)
                    };
                    let mut text = line.replace('\t', "    ");
                    if bg != palette().base {
                        let available = width.saturating_sub(2);
                        let used = cell_width(&text);
                        if used < available {
                            text.push_str(&" ".repeat(available - used));
                        }
                    }
                    lines.push(Line::tinted(text, fg, bg));
                }
            }
        }
        return single(lines);
    }
    let recent = if app.diff_recent.get() {
        " · last turn"
    } else {
        ""
    };
    let scope = app
        .diff_root
        .get()
        .map_or(String::new(), |(_, branch)| format!(" · {branch}"));
    let title = if model.diff.base.is_empty() {
        format!("diff{scope}")
    } else {
        format!("diff{scope} · {}{recent}", model.diff.summary())
    };
    let mut lines = vec![
        Line::new(title, head),
        Line::new(String::new(), palette().text),
    ];
    let stats = app.diff_stats();
    if stats.is_empty() {
        let note = if model.diff.base.is_empty() {
            "loading…"
        } else if recent.is_empty() {
            "no changes"
        } else {
            "the last turn edited nothing — t shows every change"
        };
        lines.push(Line::new(note.to_string(), palette().dim));
        return single(lines);
    }
    let count = stats.len();
    let cursor = app.diff_cursor.get().min(count - 1);
    let body = rows.saturating_sub(3).max(1);
    let start = cursor
        .saturating_sub(body / 2)
        .min(count.saturating_sub(body));
    let left = usize::from(app.chat_cols()) + 1;
    for (index, stat) in stats.iter().enumerate().skip(start).take(body) {
        let text = format!(
            "+{:<4} −{:<4} {}{}",
            stat.added,
            stat.removed,
            stat.path,
            if stat.new { " (new)" } else { "" }
        );
        app.hits.add(
            index - start + 2,
            left,
            left + 3 + text.chars().count(),
            super::hit::Target::File(index),
        );
        let marker = if index == cursor && focused {
            "›"
        } else {
            " "
        };
        let color = if index == cursor && focused {
            accent
        } else {
            palette().text
        };
        lines.push(Line::new(format!("{marker}{text}"), color));
    }
    overlay(app, &mut lines, accent);
    single(lines)
}

fn split(pair: &patch::Pair, half: usize) -> Vec<Line> {
    let (left_color, right_color, left_bg, right_bg) = match pair.kind {
        Kind::Meta => (palette().dim, palette().dim, palette().base, palette().base),
        Kind::Hunk => (
            palette().cyan,
            palette().cyan,
            palette().base,
            palette().base,
        ),
        Kind::Context => (
            palette().text,
            palette().text,
            palette().base,
            palette().base,
        ),
        Kind::Change => (
            palette().red,
            palette().green,
            palette().diff_del_bg,
            palette().diff_add_bg,
        ),
    };
    let fit = |text: &Option<String>| -> String {
        let text = text.as_deref().unwrap_or("").replace('\t', "    ");
        let mut out: String = text.chars().take(half).collect();
        while out.chars().count() < half {
            out.push(' ');
        }
        out
    };
    vec![
        Line::tinted(fit(&pair.left), left_color, left_bg),
        Line::new(" │ ".to_string(), palette().dim),
        Line::tinted(fit(&pair.right), right_color, right_bg),
    ]
}

fn trees(app: &App, rows: usize) -> Vec<Line> {
    let model = app.model.borrow();
    let accent = super::chat::accent(&model.prefs.theme);
    let focused = app.pane_focus.get();
    let head = if focused { accent } else { palette().text };
    let count = model.trees.len();
    let mut lines = vec![
        Line::new(format!("worktrees · {count}"), head),
        Line::new(String::new(), palette().text),
    ];
    if count == 0 {
        lines.push(Line::new(
            "no worktrees — summonr --worktree, --goal, or --steps".to_string(),
            palette().dim,
        ));
        return lines;
    }
    let cursor = app.tree_cursor.get().min(count - 1);
    let body = rows.saturating_sub(3).max(1);
    let start = cursor
        .saturating_sub(body / 2)
        .min(count.saturating_sub(body));
    let left = usize::from(app.chat_cols()) + 1;
    for (index, row) in model.trees.iter().enumerate().skip(start).take(body) {
        let text = format!(
            "{:<16} +{:<3} {:<5} {:<8} {}",
            row.tree.branch,
            row.ahead,
            if row.dirty { "dirty" } else { "clean" },
            row.session.as_deref().map_or("-", crate::session::short),
            row.tree
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        app.hits.add(
            index - start + 2,
            left,
            left + 3 + text.chars().count(),
            super::hit::Target::File(index),
        );
        let marker = if index == cursor && focused {
            "›"
        } else {
            " "
        };
        let color = if index == cursor && focused {
            accent
        } else if row.dirty {
            palette().red
        } else {
            palette().text
        };
        lines.push(Line::new(format!("{marker}{text}"), color));
    }
    overlay(app, &mut lines, accent);
    lines
}

fn view(app: &App, rows: usize) -> Vec<Line> {
    let model = app.model.borrow();
    let accent = super::chat::accent(&model.prefs.theme);
    let head = if app.pane_focus.get() {
        accent
    } else {
        palette().text
    };
    let Some(view) = model.view.as_ref() else {
        return vec![
            Line::new("view".to_string(), head),
            Line::new(String::new(), palette().text),
            Line::new(
                "no file — /view PATH, or → on a file in the tree".to_string(),
                palette().dim,
            ),
        ];
    };
    if let Some(source) = &view.image {
        return vec![
            Line::new(
                format!(
                    "view · {} · {}×{} · esc back",
                    view.path,
                    source.width(),
                    source.height()
                ),
                accent,
            ),
            Line::new(String::new(), palette().text),
        ];
    }
    if app.view_edit.get() {
        let text = app.view_text.get();
        let all: Vec<&str> = text.lines().collect();
        let body = rows.saturating_sub(2);
        let window =
            super::editor::window(&text, app.view_caret.get(), app.view_scroll.get(), body);
        let mut lines = vec![
            Line::new(
                format!(
                    "edit · {} · {} line{} · ctrl+s save · esc leave",
                    view.path,
                    all.len(),
                    if all.len() == 1 { "" } else { "s" }
                ),
                accent,
            ),
            Line::new(String::new(), palette().text),
        ];
        for (index, line) in all.iter().enumerate().skip(window.start).take(body) {
            lines.push(Line::new(
                format!("{:>4}   {}", index + 1, line.replace('\t', "    ")),
                palette().text,
            ));
        }
        if all.len() <= window.line {
            lines.push(Line::new(
                format!("{:>4}   ", window.line + 1),
                palette().text,
            ));
        }
        return lines;
    }
    let text = view.rope.to_string_lossy();
    let all: Vec<&str> = text.lines().collect();
    let added_total: usize = view.added.iter().map(|(_, count)| count).sum();
    let mut lines = vec![
        Line::new(
            format!(
                "view · {} · {} line{} · +{added_total} added",
                view.path,
                all.len(),
                if all.len() == 1 { "" } else { "s" }
            ),
            head,
        ),
        Line::new(String::new(), palette().text),
    ];
    let added = |number: usize| {
        view.added
            .iter()
            .any(|(start, count)| number >= *start && number < start + count)
    };
    let scroll = app.view_scroll.get().min(all.len().saturating_sub(1));
    for (index, line) in all
        .iter()
        .enumerate()
        .skip(scroll)
        .take(rows.saturating_sub(2))
    {
        let number = index + 1;
        let mark = if added(number) { '+' } else { ' ' };
        let (fg, bg) = if added(number) {
            (palette().green, palette().diff_add_bg)
        } else {
            (palette().text, palette().base)
        };
        lines.push(Line::tinted(
            format!("{number:>4} {mark} {}", line.replace('\t', "    ")),
            fg,
            bg,
        ));
    }
    lines
}

fn overlay(app: &App, lines: &mut Vec<Line>, accent: Rgba8) {
    let Some(context) = app.context.get() else {
        return;
    };
    let items = context.items();
    let width = items
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0)
        + 3;
    let mut block = vec![Line::new(format!("┌{}┐", "─".repeat(width)), accent)];
    for (index, (label, _)) in items.iter().enumerate() {
        let marker = if index == context.cursor { "›" } else { " " };
        block.push(Line::new(
            format!("│{marker}{label:<w$} │", w = width - 2),
            if index == context.cursor {
                accent
            } else {
                palette().text
            },
        ));
    }
    block.push(Line::new(format!("└{}┘", "─".repeat(width)), accent));
    let top = context.row.max(2);
    for (offset, line) in block.into_iter().enumerate() {
        let at = top + offset;
        if at < lines.len() {
            lines[at] = line;
        } else {
            lines.push(line);
        }
    }
}

fn cells(spans: &[Line], width: usize, caret: Option<usize>) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width];
    if width > 1 {
        cells[0] = Cell {
            text: "│".into(),
            fg: palette().dim,
            bg: palette().base,
            bold: false,
            italic: false,
        };
    }
    let mut slot = 2;
    for line in spans {
        for grapheme in line.text.graphemes(true) {
            let step = cell_width(grapheme);
            if slot >= width || slot.saturating_add(step) > width {
                return cells;
            }
            if step == 0 {
                continue;
            }
            cells[slot] = Cell {
                text: grapheme.into(),
                fg: line.fg,
                bg: line.bg,
                bold: false,
                italic: false,
            };
            let continuation = Cell {
                text: "".into(),
                ..cells[slot].clone()
            };
            cells[slot + 1..slot + step].fill(continuation);
            slot += step;
        }
    }
    if let Some(col) = caret
        && let Some(cell) = cells.get_mut(col + 2)
    {
        cell.fg = palette().base;
        cell.bg = palette().text;
    }
    cells
}
