use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, Dimension, JustifyContent, LayoutStyle, NodeBuilder, Paint, Position, Radius,
    Widget, button, container, icon_button, signal,
};
use sigil::widgets::MenuItem;

use super::super::style::{FLOATING, L, M, OUTLINE, RAISED, S, TINT, Text, text};
use crate::app::{App, Pick};
use crate::chat::panels::Band;
use summonr_agent::runtime::GrantAnswer;

const DETAIL_LINES: usize = 12;
const RUN: &str = "Run this command?";
const EDIT: &str = "Allow edits to this file?";
const ALLOW: &str = "Allow this step?";

/// The card above the composer that asks the user to approve a step or answer a question.
pub(super) fn card(app: &App, band: &Band) -> Option<NodeBuilder> {
    let mut reason = None;
    let (title, detail) = match band {
        Band::Approval { detail, persist } => {
            let (shown, why) = split(detail);
            reason = why;
            let title = if *persist {
                EDIT
            } else if reason.is_some() {
                RUN
            } else {
                ALLOW
            };
            (title.to_string(), first_lines(shown))
        }
        Band::Form { lines, review } => {
            let mut lines = lines.iter();
            let head = lines.next().cloned().unwrap_or_default();
            let rest: Vec<&str> = lines.map(String::as_str).collect();
            let detail = if *review || app.picks().is_empty() {
                rest.join("\n")
            } else {
                String::new()
            };
            (head, detail)
        }
        Band::Attached(_) => return None,
    };
    let picks = app.picks();
    let mut column = container()
        .layout(LayoutStyle::flex_column().fill_width().padding(L).gap(M))
        .rounded(Radius::Lg)
        .shadow(FLOATING)
        .bg(RAISED)
        .border_w(1)
        .border_color(OUTLINE)
        .child(text(Text::Title, title));
    if !detail.is_empty() {
        column = column.child(
            container()
                .layout(LayoutStyle::block().fill_width().padding(M))
                .rounded(Radius::Md)
                .bg(TINT)
                .child(
                    text(Text::Label, detail)
                        .font_family(sigil::prelude::FontFamily::Mono)
                        .wrap(),
                ),
        );
    }
    if let Some(reason) = reason {
        column = column.child(text(Text::Body, reason).wrap());
    }
    if !picks.is_empty() {
        column = column.child(choices(app, &picks));
    }
    Some(
        container()
            .layout(LayoutStyle::block().fill_width().padding_bottom(S))
            .child(column),
    )
}

const ESC: &str = "esc";
const MORE: &str = "More";
const RETURN: &str = "⏎";

/// The answer buttons: decisions end-aligned with the main action last, options in reading order.
fn choices(app: &App, picks: &[(String, Pick)]) -> NodeBuilder {
    let decision = picks
        .iter()
        .all(|(_, pick)| !matches!(pick, Pick::Option(_)));
    let row = container().layout(
        LayoutStyle::flex_row()
            .fill_width()
            .gap(S)
            .flex_wrap(sigil::prelude::FlexWrap::Wrap)
            .justify_content(if decision {
                JustifyContent::End
            } else {
                JustifyContent::Start
            })
            .align_items(AlignItems::Center),
    );
    let wider: Vec<&(String, Pick)> = picks.iter().filter(|(_, pick)| is_wider(*pick)).collect();
    let mut ordered: Vec<(usize, &(String, Pick))> = picks
        .iter()
        .enumerate()
        .filter(|(_, (_, pick))| !is_wider(*pick))
        .collect();
    if decision {
        ordered.reverse();
    }
    let row = ordered
        .into_iter()
        .fold(row, |row, (index, (label, pick))| {
            let answer = app.clone();
            let pick = *pick;
            let base = button(label.clone()).activate(move || answer.pick(pick));
            row.child(match pick {
                Pick::Grant(GrantAnswer::Deny) => base.neutral().ghost().shortcut(ESC),
                Pick::Reply(crate::app::Reply::Cancel) => base.neutral().ghost(),
                Pick::Grant(GrantAnswer::Allow) if index == 0 => base.primary().shortcut(RETURN),
                _ if index == 0 => base.primary(),
                Pick::Grant(_) | Pick::Option(_) | Pick::Reply(_) => base.neutral().soft(),
            })
        });
    if wider.is_empty() {
        row
    } else {
        row.child(wider_menu(app, &wider))
    }
}

const fn is_wider(pick: Pick) -> bool {
    matches!(pick, Pick::Grant(GrantAnswer::Always))
}

/// The arrow beside the main action, with the grants that reach past this step.
fn wider_menu(app: &App, wider: &[&(String, Pick)]) -> NodeBuilder {
    let open = signal(false);
    let (items, answer) = (
        wider
            .iter()
            .map(|(label, pick)| (label.clone(), *pick))
            .collect::<Vec<_>>(),
        app.clone(),
    );
    let arrow = icon_button(Icon::ChevronDown, move || open.set(!open.peek())).accessible(MORE);
    container()
        .layout(
            LayoutStyle::block()
                .position(Position::Relative)
                .shrink(0.0),
        )
        .child(arrow)
        .child(
            container()
                .layout(
                    LayoutStyle::block()
                        .position(Position::Absolute)
                        .inset_bottom(Dimension::Percent(1.0))
                        .inset_right(0.0)
                        .width(super::super::style::MENU),
                )
                .children_reactive(move || {
                    if !open.get() {
                        return Vec::new();
                    }
                    let menu = items.iter().fold(
                        sigil::widgets::menu(open).label(MORE),
                        |menu, (label, pick)| {
                            let (answer, pick) = (answer.clone(), *pick);
                            menu.item(
                                MenuItem::new(label.clone()).activate(move || answer.pick(pick)),
                            )
                        },
                    );
                    vec![NodeBuilder::from(menu)]
                }),
        )
        .into()
}

/// Splits a step's detail into what runs and why it needs the user's OK.
fn split(detail: &str) -> (&str, Option<String>) {
    let Some((shown, why)) = detail.rsplit_once(" — ") else {
        return (detail, None);
    };
    let mut chars = why.trim().chars();
    let reason = chars.next().map(|first| {
        let rest: String = chars.collect();
        format!("It {}{rest}.", first.to_lowercase()).replace("..", ".")
    });
    (shown, reason)
}

fn first_lines(detail: &str) -> String {
    let mut lines: Vec<&str> = detail.lines().collect();
    if lines.len() <= DETAIL_LINES {
        return detail.to_string();
    }
    lines.truncate(DETAIL_LINES);
    format!("{} …", lines.join("\n"))
}
