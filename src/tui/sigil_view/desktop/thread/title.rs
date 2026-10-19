use super::*;
use sigil::prelude::{MenuItem, menu, signal, tooltip};

const OPTIONS: &str = "Settings";
const OPTIONS_GLYPH: &str = "···";
const MENU_W: f32 = 192.0;
const BAR_H: f32 = 46.0;
const CHANGES_HINT: &str = "Uncommitted changes in this workspace";

pub(super) fn active(app: &App) -> Option<crate::tui::hub::Live> {
    app.model
        .get()
        .live
        .iter()
        .find(|entry| entry.active)
        .cloned()
}

fn first_prompt(app: &App) -> Option<String> {
    app.model.get().entries.iter().find_map(|entry| {
        if let crate::tui::sigil_view::model::Entry::User(text) = entry {
            Some(crate::text::ellipsize(text.trim(), TITLE_CELLS))
        } else {
            None
        }
    })
}

fn session_title(app: &App) -> String {
    active(app)
        .map(|entry| crate::text::ellipsize(entry.title.trim(), TITLE_CELLS))
        .filter(|title| !title.trim().is_empty())
        .or_else(|| first_prompt(app).filter(|title| !title.is_empty()))
        .unwrap_or_else(|| super::super::rail::UNTITLED.to_string())
}

fn options(app: &App) -> NodeBuilder {
    let open = signal(false);
    let (help, hotkeys, settings) = (app.clone(), app.clone(), app.clone());
    container()
        .layout(
            LayoutStyle::flex_row()
                .shrink(0.0)
                .align_items(AlignItems::Center),
        )
        .child(
            icon_button(OPTIONS_GLYPH, move || open.update(|shown| *shown = !*shown))
                .accessible(OPTIONS),
        )
        .child(
            container()
                .layout(LayoutStyle::block().width(0.0).height(0.0).shrink(0.0))
                .child(
                    menu(open)
                        .label(OPTIONS)
                        .anchor(-MENU_W, BAR_H / 2.0)
                        .width(MENU_W)
                        .item(MenuItem::new("Hotkeys").activate(move || hotkeys.open_hotkeys()))
                        .item(MenuItem::new("Help").activate(move || help.help.show()))
                        .item(MenuItem::new("Settings").activate(move || {
                            if !settings.settings_open.peek() {
                                settings.toggle_options();
                            }
                        })),
                ),
        )
}

pub(in crate::tui::sigil_view::desktop) fn title_bar(app: App) -> NodeBuilder {
    let place = {
        let app = app.clone();
        sigil::reactive::derived(move || (workspace_name(&app), session_title(&app)))
    };
    let changes = {
        let model = app.model;
        sigil::reactive::derived(move || {
            let model = model.get();
            (
                model.diff.files.len(),
                model.diff.added(),
                model.diff.removed(),
            )
        })
    };
    let mut crumbs = container()
        .layout(
            LayoutStyle::flex_row()
                .gap(S)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(
            container()
                .layout(LayoutStyle::block().shrink(0.0))
                .children_reactive(move || vec![quiet(place.get().0)]),
        );
    if let Some(branch) = app.git_branch.clone().filter(|branch| !branch.is_empty()) {
        crumbs = crumbs.child(quiet(CRUMB)).child(quiet(branch));
    }
    crumbs = crumbs.child(quiet(CRUMB)).child(
        container()
            .layout(LayoutStyle::block().min_size(0.0, 0.0))
            .children_reactive(move || vec![strong(place.get().1)]),
    );
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(BAR_H)
                .padding_x(L)
                .gap(M)
                .align_items(AlignItems::Center),
        )
        .child(crumbs)
        .child(spacer())
        .child(
            container()
                .layout(LayoutStyle::flex_row().align_items(AlignItems::Center))
                .children_reactive({
                    let review = app.clone();
                    move || {
                        let (files, added, removed) = changes.get();
                        if files == 0 {
                            return Vec::new();
                        }
                        let open = review.clone();
                        vec![
                            container().child(
                                tooltip(CHANGES_HINT).child(
                                    button(format!("Changes +{added} −{removed}"))
                                        .size(ControlSize::Compact)
                                        .accessible(CHANGES_HINT)
                                        .activate(move || open.toggle_pane(Pane::Diff)),
                                ),
                            ),
                        ]
                    }
                }),
        )
        .child(options(&app))
}
