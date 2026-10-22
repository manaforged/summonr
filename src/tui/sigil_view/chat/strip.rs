use sigil::prelude::{
    AlignItems, ControlSize, JustifyContent, LayoutBuilderExt, LayoutStyle, MouseButton,
    NodeBuilder, NodeEventsExt, Token, button, container,
};

use super::{Agent, App};
use crate::tui::ToolState;
use crate::tui::sigil_view::look::Look;
use crate::tui::sigil_view::ui::{Hints, Key};

fn text(body: impl Into<sigil::kernel::TextContent>, look: Look) -> sigil::kernel::TextBuilder {
    look.on(sigil::kernel::text(body).nowrap())
}

fn mark(agent: &Agent) -> (&'static str, Look) {
    match agent.state() {
        ToolState::Running => ("○", Look::xs(Token::Gray11)),
        ToolState::Ok => ("●", Look::xs(Token::Success11)),
        ToolState::Failed => ("●", Look::xs(Token::Error11)),
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Row {
    Head(bool),
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
const CHROME: [&str; 5] = ["○", "Subagent", "·", "·", "tok"];
const CHROME_GAPS: usize = 9;

fn fixed(
    body: impl Into<sigil::kernel::TextContent>,
    look: Look,
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
        .child(text(body, look).ellipsize())
}

type View = sigil::prelude::Signal<(bool, Vec<Agent>)>;

fn state_code(agent: &Agent) -> u8 {
    match agent.state() {
        ToolState::Running => 0,
        ToolState::Ok => 1,
        ToolState::Failed => 2,
    }
}

fn field(view: View, id: String, pick: impl Fn(&Agent) -> String + 'static) -> impl Fn() -> String {
    move || {
        view.with(|(_, rows)| {
            rows.iter()
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
    let (glyph, tone) = mark(agent);
    let roots = app.roots.clone();
    let mut line = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(text(glyph, tone))
        .child(text("Subagent", Look::xs(Token::Accent11)))
        .child(fixed(
            agent.title.clone(),
            Look::xs(Token::Gray12),
            widths.title,
            JustifyContent::Start,
        ))
        .child(
            text(
                field(view, agent.id.clone(), move |agent| doing(agent, &roots)),
                Look::xs(Token::Gray10),
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
                Look::xs(Token::Gray10),
                widths.access,
                JustifyContent::Start,
            ))
            .child(text("·", Look::xs(Token::Gray10)));
    }
    line = line
        .child(fixed(
            field(view, agent.id.clone(), Agent::clock),
            Look::xs(Token::Gray10),
            widths.clock,
            JustifyContent::End,
        ))
        .child(text("·", Look::xs(Token::Gray10)))
        .child(fixed(
            field(view, agent.id.clone(), |agent| {
                crate::provider::compact(agent.tokens)
            }),
            Look::xs(Token::Gray10),
            widths.tokens,
            JustifyContent::End,
        ))
        .child(text("tok", Look::xs(Token::Gray10)));
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

fn shown(app: &App) -> (bool, Vec<Agent>) {
    let model = app.model.get();
    let expanded = model.agents_expanded;
    let hidden: &std::collections::BTreeSet<String> = &model.agents_hidden;
    let rows = model
        .agents
        .iter()
        .filter(|agent| !hidden.contains(&agent.id))
        .filter(|agent| expanded || (agent.background && !agent.done))
        .cloned()
        .collect();
    (expanded, rows)
}

pub(in crate::tui::sigil_view) fn rows(app: &App) -> usize {
    let (_, rows) = shown(app);
    rows.len() + usize::from(!rows.is_empty())
}

pub(in crate::tui::sigil_view) fn agents(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let view = sigil::reactive::derived(move || {
        update.ticks.with(|_| ());
        if width >= 12 {
            shown(&update)
        } else {
            (false, Vec::new())
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
            let (expanded, rows) = view.get();
            let mut nodes = Vec::new();
            if !rows.is_empty() {
                let count = move || view.with(|(_, rows)| rows.len().to_string());
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
                        .layout(LayoutStyle::flex_row().fill_width().gap(8.0))
                        .child(text(
                            if expanded { "▾" } else { "▸" },
                            Look::xs(Token::Gray10),
                        ))
                        .child(text("Subagents", Look::xs(Token::Gray12).bold()))
                        .child(text(count, Look::xs(Token::Gray10)))
                        .child(text(hint, Look::xs(Token::Gray9)))
                        .pointer_down(move |event| {
                            if event.button == Some(MouseButton::Left) {
                                toggle.toggle_agents();
                                event.stop_propagation();
                            }
                        }),
                ));
            }
            let widths = Widths::of(&rows, width);
            for agent in &rows {
                let key = Row::Agent {
                    id: agent.id.clone(),
                    title: agent.title.clone(),
                    state: state_code(agent),
                    detail: expanded,
                    clear: expanded && agent.background,
                    widths,
                };
                nodes.push((key, line(agent, &app, expanded, view, widths)));
            }
            nodes
        })
}
