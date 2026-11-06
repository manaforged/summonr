use std::path::PathBuf;

use crate::app::App;
use sigil::app::dialog::Picked;
use sigil::prelude::{
    ControlSize, Dimension, LayoutStyle, NodeBuilder, Position, Signal, Widget, button, container,
    signal,
};
use sigil::widgets::MenuItem;
use summonr_agent::hub::Open;

pub(super) const BRANCH: &str = "New branch";
pub(super) const ELSEWHERE: &str = "Add repository…";

/// The header chip naming this chat's folder and branch, with a menu to start a chat elsewhere.
pub(super) fn chip(app: &App) -> NodeBuilder {
    let open = signal(false);
    let picked = signal(None::<Picked<PathBuf>>);
    super::super::welcome::follow(app, picked);
    let (shown, menu) = (app.clone(), app.clone());
    container()
        .layout(
            LayoutStyle::flex_row()
                .shrink(0.0)
                .align_items(sigil::prelude::AlignItems::Center)
                .position(Position::Relative),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .shrink(0.0)
                        .align_items(sigil::prelude::AlignItems::Center),
                )
                .children_reactive(move || {
                    let mark = if shown.git_branch().is_some() {
                        sigil::icons::Icon::GitBranch
                    } else {
                        sigil::icons::Icon::Folder
                    };
                    vec![NodeBuilder::from(
                        button(label(&shown))
                            .ghost()
                            .size(ControlSize::Compact)
                            .icon(mark)
                            .trailing(sigil::icons::Icon::ChevronDown)
                            .accessible("Repository")
                            .activate(move || open.set(!open.peek())),
                    )]
                }),
        )
        .children_reactive(move || {
            if !open.get() {
                return Vec::new();
            }
            vec![panel(&menu, open, picked)]
        })
}

fn label(app: &App) -> String {
    let project = summonr_agent::worktree::repo_of(&app.workspace())
        .unwrap_or_else(|| super::super::sidebar::project_name(app));
    match app.git_branch() {
        Some(branch) => format!("{project} · {branch}"),
        None => project,
    }
}

fn panel(app: &App, open: Signal<bool>, picked: Signal<Option<Picked<PathBuf>>>) -> NodeBuilder {
    let (branch, here) = (app.clone(), app.workspace());
    let mut menu = sigil::widgets::menu(open)
        .label("Repository")
        .width(super::super::style::MENU);
    for folder in super::super::sidebar::folders(app) {
        let target = app.clone();
        let name = super::super::sidebar::folder_name(&folder);
        menu = menu.item(
            MenuItem::new(name).activate(move || target.open_session(Open::Folder(folder.clone()))),
        );
    }
    let menu = menu
        .separator()
        .item(
            MenuItem::new(BRANCH)
                .activate(move || branch.open_session(Open::Worktree(here.clone()))),
        )
        .item(
            MenuItem::new(ELSEWHERE)
                .activate(move || picked.set(Some(super::super::welcome::folder()))),
        );
    container()
        .layout(
            LayoutStyle::block()
                .position(Position::Absolute)
                .inset_bottom(Dimension::Percent(1.0))
                .inset_left(0.0)
                .width(super::super::style::MENU),
        )
        .child(menu)
}
