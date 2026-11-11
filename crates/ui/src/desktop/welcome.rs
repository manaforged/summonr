use std::path::PathBuf;
use std::rc::Rc;

use sigil::app::dialog::{FileDialog, Picked};
use sigil::prelude::{
    AlignItems, JustifyContent, NodeBuilder, Paint, Position, Radius, Signal, Space, Tone, Widget,
    button, column, container, derived, effect, notice_band, signal,
};

use super::style::{CANVAS, L, M, OUTLINE, RAISED, S, Text, XL};

pub(super) const GAP: Space = M;
use crate::theme::Palette;

mod recent;

pub(crate) use recent::last;

const WIDTH: f32 = 480.0;
const BADGE: f32 = 56.0;
const CHOOSE: &str = "Open repository…";
const OPENING: &str = "Opening…";
const TITLE: &str = "Choose a repository";
type Open = Rc<dyn Fn(PathBuf)>;

pub(crate) fn view(
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
            let selected = selected.clone();
            sigil::reactive::defer(move || {
                picked.set(None);
                selected(path);
            });
        }
    });
    let appearance = derived(|| {
        let _backdrop = sigil::terminal::backdrop().get();
        Palette::named(summonr_agent::prefs::AUTO_THEME)
    });
    let seed = crate::scene::sky_seed();
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill()
        .children_reactive(move || {
            let page = page(card(picked, busy, error, open.clone()), appearance, seed);
            vec![page.theme(crate::desktop::style::gui(appearance.get()))]
        })
}

fn page(card: NodeBuilder, appearance: Signal<Palette>, seed: u64) -> NodeBuilder {
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill()
        .position(Position::Relative)
        .background(CANVAS)
        .child(super::sky::layer(appearance, seed))
        .child(
            sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .fill()
                .padding(XL)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center)
                .child(card),
        )
}

fn card(
    picked: Signal<Option<Picked<PathBuf>>>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    open: Open,
) -> NodeBuilder {
    column()
        .fill_width()
        .max_width(WIDTH)
        .padding(XL)
        .gap(L)
        .radius(Radius::Lg)
        .background(RAISED)
        .shadow(super::style::OVERLAY)
        .border_width(1)
        .border_color(OUTLINE)
        .child(
            column()
                .fill_width()
                .gap(S)
                .align_items(AlignItems::Center)
                .child(super::brand::mark(BADGE))
                .child(
                    container()
                        .padding_top(S)
                        .child(super::style::text(Text::Title, TITLE)),
                )
                .child(
                    sigil::prelude::row()
                        .gap(sigil::prelude::Space::None)
                        .align_items(sigil::prelude::AlignItems::Stretch)
                        .padding_top(M)
                        .child(
                            button(move || if busy.get() { OPENING } else { CHOOSE }.to_string())
                                .primary()
                                .disabled(move || busy.get())
                                .activate(move || picked.set(Some(folder()))),
                        ),
                ),
        )
        .child(
            container().children_reactive(move || {
                error.get().map(|error| notice_band(error, Tone::Error))
            }),
        )
        .child(recent::list(busy, error, open))
}

/// Opens a new agent in the folder `picked` resolves to.
pub(super) fn follow(app: &super::App, picked: Signal<Option<Picked<PathBuf>>>) {
    let app = app.clone();
    sigil::prelude::effect(move || {
        if let Some(result) = picked.get()
            && let sigil::tasks::Status::Ok(Some(path)) = result.get()
        {
            let app = app.clone();
            sigil::reactive::defer(move || {
                picked.set(None);
                super::spaces::add_repository(path.clone());
                app.open_session(summonr_agent::hub::Open::Folder(path));
            });
        }
    });
}

/// Asks the user for a folder, starting in their home folder.
pub(super) fn folder() -> Picked<PathBuf> {
    let mut dialog = FileDialog::new();
    if let Some(home) = summonr_core::dirs::home_dir() {
        dialog = dialog.directory(home);
    }
    dialog.pick_folder()
}
