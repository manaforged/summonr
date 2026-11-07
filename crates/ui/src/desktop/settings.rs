use std::cell::Cell;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, JustifyContent, LayoutStyle, NodeBuilder, Paint, Signal, Widget, container, icon,
    scroll, signal,
};
use sigil::widgets::{ListRowSlots, list_row};

use super::App;
use super::style::{CANVAS, ICON, L, RECESSED, S, Text, XL, text};

mod accounts;
mod appearance;
mod general;
mod mcp;
mod models;
pub(super) mod parts;
mod plugins;
mod shortcuts;
mod tools;
mod usage;

/// One page of the Settings window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Page {
    General,
    Models,
    Accounts,
    Mcp,
    Plugins,
    Appearance,
    Shortcuts,
    Tools,
    Usage,
}

impl Page {
    const ALL: [Self; 9] = [
        Self::General,
        Self::Models,
        Self::Accounts,
        Self::Usage,
        Self::Mcp,
        Self::Plugins,
        Self::Appearance,
        Self::Shortcuts,
        Self::Tools,
    ];

    /// The page's name in the Settings list.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Models => "Models",
            Self::Accounts => "Accounts",
            Self::Mcp => "MCP servers",
            Self::Plugins => "Plugins & skills",
            Self::Appearance => "Appearance",
            Self::Shortcuts => "Keyboard shortcuts",
            Self::Tools => "Tools",
            Self::Usage => "Usage",
        }
    }

    /// The page's icon in the Settings list.
    pub(super) const fn icon(self) -> Icon {
        match self {
            Self::General => Icon::SlidersHorizontal,
            Self::Models => Icon::Cpu,
            Self::Accounts => Icon::Key,
            Self::Mcp => Icon::Server,
            Self::Plugins => Icon::Package,
            Self::Appearance => Icon::Sun,
            Self::Shortcuts => Icon::Command,
            Self::Tools => Icon::Zap,
            Self::Usage => Icon::ChartColumn,
        }
    }

    fn view(self, app: &App) -> Vec<NodeBuilder> {
        match self {
            Self::General => general::view(app),
            Self::Models => models::view(app),
            Self::Accounts => accounts::view(app),
            Self::Mcp => mcp::view(app),
            Self::Plugins => plugins::view(app),
            Self::Appearance => appearance::view(app),
            Self::Shortcuts => shortcuts::view(app),
            Self::Tools => tools::view(app),
            Self::Usage => usage::view(app),
        }
    }
}

/// The window's Settings state: whether it shows, and the open group of its pages.
type Shown = (Signal<bool>, Signal<Option<crate::settings::Group>>);

thread_local! {
    static PAGE: Cell<Option<Signal<Page>>> = const { Cell::new(None) };
    static OPEN: Cell<Option<Shown>> = const { Cell::new(None) };
}

/// Creates the Settings page state for this window.
pub(super) fn install(app: &App) {
    page();
    OPEN.with(|slot| slot.set(Some((app.settings_open, app.settings_group))));
}

/// Whether the Settings window shows.
pub(super) fn is_open() -> bool {
    OPEN.with(Cell::get)
        .and_then(|(open, _)| open.try_peek())
        .unwrap_or(false)
}

fn page() -> Signal<Page> {
    PAGE.with(|slot| {
        let page = slot
            .get()
            .filter(|page| page.try_peek().is_some())
            .unwrap_or_else(|| signal(Page::General));
        slot.set(Some(page));
        page
    })
}

/// Shows `page` in the window's Settings, from any pane.
pub(super) fn open(app: &App, at: Page) {
    let (open, group) = OPEN
        .with(Cell::get)
        .filter(|(open, _)| open.try_peek().is_some())
        .unwrap_or((app.settings_open, app.settings_group));
    page().set(at);
    group.set(None);
    open.set(true);
}

/// The Settings window: page list on the left, the page on the right.
pub(super) fn view(app: &App) -> NodeBuilder {
    let current = page();
    let body = app.clone();
    container()
        .layout(LayoutStyle::flex_row().fill().min_size(0.0, 0.0))
        .bg(CANVAS)
        .child(nav(app, current))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .grow(1.0)
                        .fill_height()
                        .min_size(0.0, 0.0),
                )
                .child(
                    container()
                        .layout(
                            LayoutStyle::flex_row()
                                .fill_width()
                                .height(super::TITLE_BAR)
                                .shrink(0.0),
                        )
                        .child(super::drag_area()),
                )
                .children_reactive(move || {
                    let at = current.get();
                    let content = container()
                        .layout(
                            LayoutStyle::flex_column()
                                .fill_width()
                                .max_width(parts::PAGE)
                                .align_items(AlignItems::Stretch)
                                .gap(XL)
                                .padding_x(XL)
                                .padding_bottom(XL),
                        )
                        .child(text(Text::Display, at.name()));
                    let content = sigil::reactive::untrack(|| at.view(&body))
                        .into_iter()
                        .fold(content, NodeBuilder::child);
                    vec![
                        scroll(
                            container()
                                .layout(
                                    LayoutStyle::flex_row()
                                        .fill_width()
                                        .justify_content(JustifyContent::Center),
                                )
                                .child(content),
                        )
                        .grow(1.0),
                    ]
                }),
        )
}

fn nav(app: &App, current: Signal<Page>) -> NodeBuilder {
    let back = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .width(super::sidebar::WIDTH)
                .fill_height()
                .shrink(0.0)
                .padding_x(S)
                .padding_top(super::TITLE_BAR)
                .gap(L),
        )
        .bg(RECESSED)
        .child(list_row(
            false,
            "Back to app",
            ListRowSlots {
                leading: Some(Box::new(icon(Icon::ArrowLeft).px(ICON))),
                activate: Some(std::rc::Rc::new(move || back.settings_open.set(false))),
                ..ListRowSlots::default()
            },
        ))
        .child(
            container()
                .layout(LayoutStyle::flex_column())
                .children_reactive(move || {
                    let at = current.get();
                    Page::ALL
                        .into_iter()
                        .map(|page| {
                            list_row(
                                page == at,
                                page.name(),
                                ListRowSlots {
                                    leading: Some(Box::new(icon(page.icon()).px(ICON))),
                                    activate: Some(std::rc::Rc::new(move || current.set(page))),
                                    ..ListRowSlots::default()
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                }),
        )
}
