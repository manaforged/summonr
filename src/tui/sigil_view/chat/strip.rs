use sigil::prelude::{
    AlignItems, ControlSize, JustifyContent, LayoutStyle, NodeBuilder, Token, Tone, button,
    container,
};

use super::entry::{bits, glyph, loading_mark};
use super::{Agent, App};
use crate::tui::sigil_view::text_style::TextStyle;
use crate::tui::sigil_view::ui::{Dialog, Hints, Key, Size};
use sigil::prelude::Widget;

fn text(
    body: impl Into<sigil::kernel::TextContent>,
    text_style: TextStyle,
) -> sigil::kernel::TextBuilder {
    text_style.on(sigil::kernel::text(body).nowrap())
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Row {
    Head(bool),
    More(usize),
    Headless {
        id: String,
        title: String,
        model: String,
        status: String,
    },
    Agent {
        id: String,
        title: String,
        state: u8,
        detail: bool,
        clear: bool,
        widths: Widths,
    },
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
struct Widths {
    title: usize,
    access: usize,
    clock: usize,
    tokens: usize,
}

impl Widths {
    fn of(rows: &[Agent], width: usize) -> Self {
        let most = |pick: &dyn Fn(&Agent) -> String| {
            rows.iter()
                .map(|agent| super::cell_width(&pick(agent)))
                .max()
                .unwrap_or(0)
        };
        let mut widths = Self {
            title: 0,
            access: most(&|agent| agent.access.name().to_string()),
            clock: most(&Agent::clock),
            tokens: most(&|agent| crate::provider::compact(agent.tokens)),
        };
        widths.title = most(&|agent| agent.title.clone())
            .min(width * TITLE_SHARE.0 / TITLE_SHARE.1)
            .min(width.saturating_sub(widths.taken()));
        widths
    }

    fn taken(self) -> usize {
        let chrome: usize = CHROME.iter().map(|part| super::cell_width(part)).sum();
        chrome + CHROME_GAPS + self.access + self.clock + self.tokens + ACTIVITY_MIN
    }
}

const TITLE_SHARE: (usize, usize) = (2, 5);
const ACTIVITY_MIN: usize = 12;
const CHROME: [&str; 5] = ["✳", "Subagent", "·", "·", "tok"];
const CHROME_GAPS: usize = 9;

fn fixed(
    body: impl Into<sigil::kernel::TextContent>,
    text_style: TextStyle,
    cells: usize,
    justify: JustifyContent,
) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .width(cells as f32 * super::canvas::cell().w)
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .justify_content(justify),
        )
        .child(text(body, text_style).ellipsize())
}

type View = sigil::prelude::Signal<Census>;

fn field(view: View, id: String, pick: impl Fn(&Agent) -> String + 'static) -> impl Fn() -> String {
    move || {
        view.with(|census| {
            census.rows.iter()
                .find(|agent| agent.id == id)
                .map(&pick)
                .unwrap_or_default()
        })
    }
}

fn doing(agent: &Agent, roots: &[String]) -> String {
    let last = super::entry::workspace_relative(&agent.last, &agent.roots(roots));
    if last.is_empty() {
        "· starting".to_string()
    } else {
        format!("· {last}")
    }
}

fn line(agent: &Agent, app: &App, detail: bool, view: View, widths: Widths) -> NodeBuilder {
    let roots = app.roots.clone();
    let mark = if agent.done {
        let (glyph, tone) = glyph(&agent.state());
        text(glyph, tone)
    } else {
        loading_mark(app.ticks)
    };
    let mut line = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(mark)
        .child(text("Subagent", TextStyle::xs(Token::Accent11).bold()))
        .child(text("·", TextStyle::xs(Token::Gray10)))
        .child(fixed(
            agent.title.clone(),
            TextStyle::xs(Token::Gray12),
            widths.title,
            JustifyContent::Start,
        ))
        .child(
            text(
                field(view, agent.id.clone(), move |agent| doing(agent, &roots)),
                TextStyle::xs(Token::Gray10),
            )
            .ellipsize()
            .layout(
                LayoutStyle::block()
                    .grow(1.0)
                    .shrink(1.0)
                    .min_size(0.0, 0.0),
            ),
        );
    if detail {
        line = line
            .child(fixed(
                agent.access.name(),
                TextStyle::xs(Token::Gray10),
                widths.access,
                JustifyContent::Start,
            ))
            .child(text("·", TextStyle::xs(Token::Gray10)));
    }
    line = line
        .child(fixed(
            field(view, agent.id.clone(), Agent::clock),
            TextStyle::xs(Token::Gray10),
            widths.clock,
            JustifyContent::End,
        ))
        .child(text("·", TextStyle::xs(Token::Gray10)))
        .child(fixed(
            field(view, agent.id.clone(), |agent| {
                crate::provider::compact(agent.tokens)
            }),
            TextStyle::xs(Token::Gray10),
            widths.tokens,
            JustifyContent::End,
        ))
        .child(text("tok", TextStyle::xs(Token::Gray10)));
    if detail && agent.background {
        let clear = app.clone();
        let id = agent.id.clone();
        line = line.child(
            button("Clear")
                .ghost()
                .size(ControlSize::Compact)
                .accessible(format!("Clear subagent {}", agent.title))
                .activate(move || clear.clear_agent(&id)),
        );
    }
    line
}

#[derive(Clone, PartialEq)]
struct Census {
    view: super::super::model::AgentsView,
    rows: Vec<Agent>,
    headless: Vec<crate::mail::Agent>,
    total: usize,
    rest: usize,
}

fn shown(app: &App) -> Census {
    use super::super::model::AgentsView;
    let model = app.model.get();
    let view = model.agents_view;
    let hidden: &std::collections::BTreeSet<String> = &model.agents_hidden;
    let all: Vec<Agent> = model
        .agents
        .iter()
        .filter(|agent| !hidden.contains(&agent.id))
        .cloned()
        .collect();
    let peers: Vec<crate::mail::Agent> = model
        .peers
        .iter()
        .filter(|peer| peer.parent.as_deref() == Some(model.session.as_str()))
        .filter(|peer| !hidden.contains(&peer.id))
        .cloned()
        .collect();
    let total = all.len() + peers.len();
    let mut rows = all;
    if !view.open() {
        rows.retain(|agent| agent.background && !agent.done);
    }
    let mut rest = 0;
    if matches!(view, AgentsView::Collapsed | AgentsView::Strip) {
        let cap = crate::limits::limits().strip_rows.max(1);
        let shown = rows.len() + peers.len();
        if shown > cap {
            rest = shown - cap;
            let keep = cap.saturating_sub(peers.len());
            rows.truncate(keep);
        }
    }
    Census {
        view,
        rows,
        headless: peers,
        total,
        rest,
    }
}

pub(in crate::tui::sigil_view) fn rows(app: &App) -> usize {
    let census = shown(app);
    let count = census.rows.len() + census.headless.len();
    count + 2 * usize::from(count > 0) + usize::from(census.rest > 0)
}

const SPLIT_COLS: u16 = 72;

pub(in crate::tui::sigil_view) fn agents(app: App, width: usize) -> NodeBuilder {
    let paint = app.clone();
    container().children_reactive(move || {
        let split = paint.model.get().agents_view == super::super::model::AgentsView::Split
            && super::super::ui::browser_split(width as u16, SPLIT_COLS);
        let list = list(paint.clone(), width);
        if !split {
            return vec![list];
        }
        let detail = detail(&paint);
        super::super::ui::browser_columns(
            std::iter::once(list),
            LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0),
            vec![detail],
            8.0,
        )
    })
}

fn detail(app: &App) -> NodeBuilder {
    let census = shown(app);
    let cursor = app.model.get().agents_cursor;
    let agent = census.rows.get(cursor).or_else(|| census.rows.first());
    let title = agent.map(|agent| agent.title.clone()).unwrap_or_default();
    let last = agent.map(|agent| agent.last.clone()).unwrap_or_default();
    container()
        .layout(LayoutStyle::flex_column().fill_width().gap(2.0).min_size(0.0, 0.0))
        .child(text("selected", TextStyle::xs(Token::Gray10)))
        .child(text(title, TextStyle::xs(Token::Gray12)))
        .child(text(format!("· {last}"), TextStyle::xs(Token::Gray11)))
}

fn list(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let view = sigil::reactive::derived(move || {
        update.ticks.with(|_| ());
        if width >= 12 {
            shown(&update)
        } else {
            Census {
                view: super::super::model::AgentsView::Collapsed,
                rows: Vec::new(),
                headless: Vec::new(),
                total: 0,
                rest: 0,
            }
        }
    });
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_keyed(move || {
            let census = view.get();
            let expanded = census.view.open();
            let rows = census.rows;
            let headless = census.headless;
            let mut nodes = Vec::new();
            if !rows.is_empty() || !headless.is_empty() || census.total > 0 {
                let count = move || view.with(|census| census.total.to_string());
                let toggle = app.clone();
                let hint = Hints::new()
                    .key(
                        Key::Hotkey(crate::prefs::Hotkey::Agents),
                        if expanded { "closes" } else { "shows all" },
                    )
                    .text(&app.model.peek().prefs.hotkeys);
                nodes.push((
                    Row::Head(expanded),
                    container()
                        .layout(
                            LayoutStyle::flex_row()
                                .fill_width()
                                .padding_top(super::canvas::cell().h)
                                .gap(8.0)
                                .align_items(AlignItems::Center),
                        )
                        .child(
                            button(if expanded { "▾" } else { "▸" })
                                .ghost()
                                .size(ControlSize::Compact)
                                .accessible(if expanded {
                                    "Collapse subagents"
                                } else {
                                    "Expand subagents"
                                })
                                .activate(move || toggle.toggle_agents()),
                        )
                        .child(text("Subagents", TextStyle::xs(Token::Gray12).bold()))
                        .child(text(count, TextStyle::xs(Token::Gray10)))
                        .child(text(hint, TextStyle::xs(Token::Gray9))),
                ));
            }
            let widths = Widths::of(&rows, width);
            for agent in &rows {
                let key = Row::Agent {
                    id: agent.id.clone(),
                    title: agent.title.clone(),
                    state: bits(&agent.state(), false),
                    detail: expanded,
                    clear: expanded && agent.background,
                    widths,
                };
                let open = app.clone();
                let index = nodes.len();
                let mut row = line(agent, &app, expanded, view, widths);
                row = row.pointer_down(move |event| {
                    open.model_mut(|model| {
                        model.agents_cursor = index;
                        model.agents_view = super::super::model::AgentsView::Split;
                    });
                    event.stop_propagation();
                });
                nodes.push((key, row));
            }
            let model_width = headless
                .iter()
                .map(|peer| super::cell_width(&peer.model))
                .max()
                .unwrap_or(0)
                .min(width / 4);
            let status_width = headless
                .iter()
                .map(|peer| super::cell_width(&peer.status.to_string()))
                .max()
                .unwrap_or(0);
            let tool_width = headless
                .iter()
                .map(|peer| super::cell_width(&peer.tool))
                .max()
                .unwrap_or(0);
            if expanded {
                for peer in headless {
                    let clear_id = peer.id.clone();
                    let title = peer.label().to_string();
                    let status = peer.status.to_string();
                    let working = peer.status == crate::mail::Presence::Working;
                    let tone = if working {
                        Token::Success11
                    } else {
                        Token::Gray10
                    };
                    nodes.push((
                        Row::Headless {
                            id: peer.id,
                            title: title.clone(),
                            model: peer.model.clone(),
                            status: status.clone(),
                        },
                        container()
                            .layout(
                                LayoutStyle::flex_row()
                                    .fill_width()
                                    .gap(8.0)
                                    .min_size(0.0, 0.0)
                                    .align_items(AlignItems::Center),
                            )
                            .child({
                                let id = clear_id;
                                let clear = app.clone();
                                button(if working { "●" } else { "○" })
                                    .ghost()
                                    .tone(if working {
                                        Tone::Success
                                    } else {
                                        Tone::Neutral
                                    })
                                    .size(ControlSize::Compact)
                                    .accessible(format!("Clear subagent {title}"))
                                    .activate(move || clear.clear_agent(&id))
                            })
                            .child(
                                text(title, TextStyle::xs(Token::Gray12))
                                    .ellipsize()
                                    .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)),
                            )
                            .child(fixed(
                                peer.model,
                                TextStyle::xs(Token::Gray10),
                                model_width,
                                JustifyContent::Start,
                            ))
                            .child(fixed(
                                status,
                                TextStyle::xs(tone),
                                status_width,
                                JustifyContent::Start,
                            ))
                            .child(fixed(
                                peer.tool.clone(),
                                TextStyle::xs(Token::Gray11),
                                tool_width,
                                JustifyContent::Start,
                            )),
                    ));
                }
            }
            if census.rest > 0 {
                let open = app.clone();
                let more = census.rest;
                nodes.push((
                    Row::More(more),
                    container().child(
                        button(format!("{more} more"))
                            .ghost()
                            .size(ControlSize::Compact)
                            .accessible(format!("{more} more subagents"))
                            .activate(move || {
                                open.model_mut(|model| {
                                    model.agents_view = super::super::model::AgentsView::Sheet;
                                });
                            }),
                    ),
                ));
            }
            nodes
        })
}

pub(in crate::tui::sigil_view) fn sheet(app: App) -> NodeBuilder {
    let paint = app.clone();
    let keys = app.model.peek().prefs.hotkeys.clone();
    Dialog::new(
        Size::Reading,
        "Subagents",
        Hints::new().key(Key::Esc, "close"),
    )
    .child(container().children_reactive(move || {
        let census = shown(&paint);
        let mut nodes = Vec::new();
        for agent in &census.rows {
            nodes.push(text(agent.title.clone(), TextStyle::xs(Token::Gray12)));
        }
        nodes
    }))
    .build(app.agents_sheet_open(), app.viewport.peek(), &keys)
}
