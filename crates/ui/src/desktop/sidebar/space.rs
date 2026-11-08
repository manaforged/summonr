use std::rc::Rc;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, Dimension, LayoutStyle, ListRowSlots, NodeBuilder, Position, Signal, Widget, container,
    list_row, signal,
};
use sigil::widgets::{MenuItem, icon};
use summonr_agent::hub::Live;
use summonr_agent::prefs::Space;
use summonr_agent::session::SessionInfo;

use super::super::App;
use super::super::spaces;
use super::super::style::{ICON, M, S, SCROLLBAR, Text, text};

const PROJECTS: &str = "Projects";
const NEW_PROJECT: &str = "New project";
const ADD_PROJECT: &str = "Add project";

/// The Projects heading, with a button whose menu adds a repository or makes a named project.
pub(in crate::desktop) fn heading(app: &App) -> NodeBuilder {
    let picked = signal(None);
    super::super::welcome::follow(app, picked);
    let open = signal(false);
    let button = sigil::prelude::icon_button(Icon::Plus, move || open.update(|shown| *shown = !*shown))
        .accessible(ADD_PROJECT);
    let anchored = container()
        .layout(LayoutStyle::block().position(Position::Relative).shrink(0.0))
        .child(super::super::tip(ADD_PROJECT, None, button.into()))
        .child(
            container()
                .layout(
                    LayoutStyle::block()
                        .position(Position::Absolute)
                        .inset_top(Dimension::Percent(1.0))
                        .inset_right(0.0)
                        .width(super::super::style::MENU),
                )
                .children_reactive(move || {
                    if !open.get() {
                        return Vec::new();
                    }
                    let menu = sigil::widgets::menu(open)
                        .label(ADD_PROJECT)
                        .item(
                            MenuItem::new(super::super::thread::ADD_REPOSITORY)
                                .activate(move || picked.set(Some(super::super::welcome::folder()))),
                        )
                        .item(MenuItem::new(NEW_PROJECT).activate(|| {
                            sigil::reactive::defer(|| {
                                let made = spaces::create(NEW_PROJECT);
                                super::super::agent_menu::rename_space(&made, NEW_PROJECT);
                            });
                        }));
                    vec![NodeBuilder::from(menu)]
                }),
        );
    title_row(PROJECTS, anchored.into())
}

fn title_row(title: &str, button: NodeBuilder) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_top(M)
                .padding_left(S)
                .padding_right(SCROLLBAR)
                .align_items(AlignItems::Center),
        )
        .child(text(Text::Heading, title).layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)))
        .child(button)
}

/// Each workspace, folded or followed by its agents; tracked.
pub(in crate::desktop) fn rows(app: &App, live: Signal<Vec<Live>>, shown: Signal<Vec<String>>) -> NodeBuilder {
    let (app, shut) = (app.clone(), signal(Vec::<String>::new()));
    container()
        .layout(LayoutStyle::flex_column().fill_width())
        .children_reactive(move || {
            let (list, live, shown, closed) = (spaces::all(), live.get(), shown.get(), shut.get());
            let chats = super::row::listed(&app);
            let mut nodes = Vec::new();
            for (at, space) in list.iter().enumerate() {
                let agents: Vec<SessionInfo> = chats
                    .iter()
                    .filter(|info| spaces::of(&list, info) == Some(at))
                    .cloned()
                    .collect();
                let open = !closed.contains(&space.id);
                nodes.push(section(&app, space, (open, agents.len()), shut));
                if !open {
                    continue;
                }
                for topic in super::super::topic::group(agents) {
                    let selected = topic.threads.iter().any(|info| shown.contains(&info.id));
                    nodes.extend(super::super::topic::row(&app, &topic, (selected, 0), &live));
                }
            }
            nodes
        })
}

/// The agents in no workspace; tracked.
pub(in crate::desktop) fn outside(chats: Vec<SessionInfo>) -> Vec<SessionInfo> {
    let list = spaces::all();
    chats
        .into_iter()
        .filter(|info| spaces::of(&list, info).is_none())
        .collect()
}

fn section(app: &App, space: &Space, (open, count): (bool, usize), shut: Signal<Vec<String>>) -> NodeBuilder {
    let status = match count {
        0 => "empty".to_owned(),
        1 => "1 agent".to_owned(),
        n => format!("{n} agents"),
    };
    let flip = space.id.clone();
    let row = list_row(
        false,
        space.name.clone(),
        ListRowSlots {
            leading: Some(Box::new(icon(Icon::Layers).px(ICON))),
            status: Some(status.into()),
            activate: Some(Rc::new(move || {
                shut.update(|closed| match closed.iter().position(|seen| *seen == flip) {
                    Some(found) => drop(closed.remove(found)),
                    None => closed.push(flip.clone()),
                });
            })),
            ..ListRowSlots::default()
        },
    )
    .expanded(signal(open));
    let (menu, held) = (app.clone(), space.clone());
    container()
        .layout(LayoutStyle::flex_row().fill_width().padding_top(S))
        .child(sigil::widgets::context_menu(row, move || items(&menu, &held)))
}

fn items(app: &App, space: &Space) -> Vec<MenuItem> {
    let open = app.clone();
    let (start, pick, rename, gone) = (space.id.clone(), space.id.clone(), space.id.clone(), space.id.clone());
    let name = space.name.clone();
    vec![
        MenuItem::new("New agent here").activate(move || spaces::new_agent(&open, &start)),
        MenuItem::new("Add folder…").activate(move || spaces::pick_folder(&pick)),
        MenuItem::new("Rename…").activate(move || super::super::agent_menu::rename_space(&rename, &name)),
        MenuItem::new("Delete project").activate(move || spaces::remove(&gone)),
    ]
}
