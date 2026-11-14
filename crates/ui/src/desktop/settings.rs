use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, JustifyContent, NodeBuilder, Paint, Signal, Space, Widget, icon, scroll,
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
mod will;
pub(super) mod workflows;

/// One page of the Settings window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Page {
    General,
    Models,
    Workflows,
    Accounts,
    Mcp,
    Plugins,
    Appearance,
    Will,
    Shortcuts,
    Tools,
    Usage,
}

impl Page {
    const ALL: [Self; 11] = [
        Self::General,
        Self::Models,
        Self::Workflows,
        Self::Accounts,
        Self::Usage,
        Self::Mcp,
        Self::Plugins,
        Self::Appearance,
        Self::Will,
        Self::Shortcuts,
        Self::Tools,
    ];

    /// The page's name in the Settings list.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Models => "Models",
            Self::Workflows => "Workflows",
            Self::Accounts => "Accounts",
            Self::Mcp => "MCP servers",
            Self::Plugins => "Plugins & skills",
            Self::Appearance => "Appearance",
            Self::Will => "Will",
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
            Self::Workflows => Icon::Layers,
            Self::Accounts => Icon::Key,
            Self::Mcp => Icon::Server,
            Self::Plugins => Icon::Package,
            Self::Appearance => Icon::Sun,
            Self::Will => Icon::Star,
            Self::Shortcuts => Icon::Command,
            Self::Tools => Icon::Zap,
            Self::Usage => Icon::ChartColumn,
        }
    }

    /// The page's mark in the Settings list: its icon, or Will himself on his own page.
    fn leading(self, app: &App) -> NodeBuilder {
        match self {
            Self::Will => super::will::still(app, ICON),
            Self::General
            | Self::Models
            | Self::Workflows
            | Self::Accounts
            | Self::Mcp
            | Self::Plugins
            | Self::Appearance
            | Self::Shortcuts
            | Self::Tools
            | Self::Usage => icon(self.icon()).px(ICON).into(),
        }
    }

    fn view(self, app: &App) -> Vec<NodeBuilder> {
        match self {
            Self::General => general::view(app),
            Self::Models => models::view(app),
            Self::Workflows => workflows::view(app),
            Self::Accounts => accounts::view(app),
            Self::Mcp => mcp::view(app),
            Self::Plugins => plugins::view(app),
            Self::Appearance => appearance::view(app),
            Self::Will => will::view(app),
            Self::Shortcuts => shortcuts::view(app),
            Self::Tools => tools::view(app),
            Self::Usage => usage::view(app),
        }
    }
}

/// Shows `page` in the window's Settings, from any pane.
pub(super) fn open(app: &App, at: Page) {
    app.window.settings_page.set(at);
    app.settings_group.set(None);
    app.settings_open.set(true);
}

/// The Settings window: page list on the left, the page on the right.
pub(super) fn view(app: &App) -> NodeBuilder {
    let current = app.window.settings_page;
    let body = app.clone();
    sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .align_items(sigil::prelude::AlignItems::Stretch)
        .fill()
        .min_width(0.0)
        .min_height(0.0)
        .background(CANVAS)
        .child(nav(app, current))
        .child(
            sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .grow(1.0)
                .fill_height()
                .min_width(0.0)
                .min_height(0.0)
                .child(
                    sigil::prelude::row()
                        .gap(sigil::prelude::Space::None)
                        .align_items(sigil::prelude::AlignItems::Stretch)
                        .fill_width()
                        .height(super::TITLE_BAR)
                        .shrink(0.0)
                        .child(super::drag_area()),
                )
                .children_reactive(move || {
                    let at = current.get();
                    let content = sigil::prelude::column()
                        .fill_width()
                        .max_width(parts::PAGE)
                        .align_items(AlignItems::Stretch)
                        .gap(XL)
                        .padding_x(XL)
                        .padding_bottom(XL)
                        .child(text(Text::Title, at.name()));
                    let content = sigil::reactive::untrack(|| at.view(&body))
                        .into_iter()
                        .fold(content, NodeBuilder::child);
                    vec![
                        scroll(
                            sigil::prelude::row()
                                .gap(sigil::prelude::Space::None)
                                .align_items(sigil::prelude::AlignItems::Stretch)
                                .fill_width()
                                .justify_content(JustifyContent::Center)
                                .child(content),
                        )
                        .grow(1.0),
                    ]
                }),
        )
}

fn nav(app: &App, current: Signal<Page>) -> NodeBuilder {
    let (back, app) = (app.clone(), app.clone());
    sigil::prelude::column()
        .width(super::sidebar::WIDTH)
        .fill_height()
        .shrink(0.0)
        .padding_x(S)
        .padding_top(Space::raw(super::TITLE_BAR, "clears the title bar"))
        .gap(L)
        .background(RECESSED)
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
            sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .children_reactive(move || {
                    let at = current.get();
                    Page::ALL
                        .into_iter()
                        .map(|page| {
                            list_row(
                                page == at,
                                page.name(),
                                ListRowSlots {
                                    leading: Some(Box::new(page.leading(&app))),
                                    activate: Some(std::rc::Rc::new(move || current.set(page))),
                                    ..ListRowSlots::default()
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                }),
        )
}
