use std::path::PathBuf;
use std::rc::Rc;

use sigil::app::dialog::{FileDialog, Picked};
use sigil::prelude::{
    AlignItems, JustifyContent, LayoutStyle, NodeBuilder, Paint, Signal, Token, Tone, Widget,
    button, column, container, effect, field, notice_band, row, scroll, signal, text, text_field,
};

use super::thread::{BODY, L, M, S, XL};

mod brand;
mod recent;
#[cfg(test)]
mod tests;

const WIDTH: f32 = 704.0;
const CHOOSE: &str = "Open folder";
type Open = Rc<dyn Fn(PathBuf)>;

pub(in crate::tui::sigil_view) fn view(
    error: Signal<Option<String>>,
    busy: Signal<bool>,
    open: impl Fn(PathBuf) + 'static,
) -> NodeBuilder {
    let open: Open = Rc::new(open);
    let picked = signal(None::<Picked<PathBuf>>);
    let selected = open.clone();
    effect(move || {
        if let Some(result) = picked.get()
            && let sigil::tasks::Status::Ok(Some(path)) = result.get()
        {
            picked.set(None);
            selected(path);
        }
    });
    let content = column()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .max_width(WIDTH)
                .gap(XL),
        )
        .child(heading())
        .child(actions(picked, busy, error, open.clone()))
        .child(recent::list(busy, error, open));
    container()
        .layout(LayoutStyle::flex_column().fill().padding(XL))
        .bg(Token::Surface)
        .child(
            scroll(
                row()
                    .layout(
                        LayoutStyle::flex_row()
                            .fill_width()
                            .justify_content(JustifyContent::Center)
                            .padding_y(XL),
                    )
                    .child(content),
            )
            .fill(),
        )
}

fn actions(
    picked: Signal<Option<Picked<PathBuf>>>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    open: Open,
) -> NodeBuilder {
    column()
        .layout(LayoutStyle::flex_column().gap(L))
        .child(
            button(move || {
                if busy.get() {
                    "Opening workspace…"
                } else {
                    CHOOSE
                }
                .to_string()
            })
            .primary()
            .disabled(move || busy.get())
            .activate(move || picked.set(Some(folder()))),
        )
        .child(path_input(busy, open))
        .child(
            container().children_reactive(move || {
                error.get().map(|error| notice_band(error, Tone::Error))
            }),
        )
}

fn heading() -> NodeBuilder {
    row()
        .layout(
            LayoutStyle::flex_row()
                .gap(XL)
                .align_items(AlignItems::Center),
        )
        .child(brand::mark())
        .child(
            column()
                .layout(
                    LayoutStyle::flex_column()
                        .gap(S)
                        .grow(1.0)
                        .min_size(0.0, 0.0),
                )
                .child(text("Open a workspace").font_size(24.0).font_weight(600))
                .child(
                    text("Choose a project folder to start working.")
                        .font_size(BODY)
                        .text_color(Token::OnSurfaceMuted),
                ),
        )
}

fn path_input(busy: Signal<bool>, open: Open) -> NodeBuilder {
    let path = signal(String::new());
    let submit = open.clone();
    row()
        .layout(LayoutStyle::flex_row().gap(M).align_items(AlignItems::End))
        .child(
            container()
                .layout(LayoutStyle::flex_column().grow(1.0).min_size(0.0, 0.0))
                .child(field(
                    "Folder path",
                    text_field(path)
                        .placeholder("/path/to/project")
                        .autofocus()
                        .disabled(move || busy.get())
                        .submit(move |path| open_path(path, &submit)),
                )),
        )
        .child(
            button("Open workspace")
                .disabled(move || busy.get() || path.get().trim().is_empty())
                .activate(move || open_path(&path.peek(), &open)),
        )
}

fn open_path(path: &str, open: &Open) {
    let path = path.trim();
    if !path.is_empty() {
        open(crate::config::expand_tilde(path).unwrap_or_else(|| PathBuf::from(path)));
    }
}

fn folder() -> Picked<PathBuf> {
    let mut dialog = FileDialog::new();
    if let Some(home) = summonr_core::dirs::home_dir() {
        dialog = dialog.directory(home);
    }
    dialog.pick_folder()
}
