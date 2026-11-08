use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::icons::Icon;
use sigil::prelude::{LayoutStyle, ListRowSlots, NodeBuilder, Signal, Widget, container, list_row};
use sigil::widgets::icon;
use summonr_agent::hub::{Live, Status};
use summonr_agent::session::SessionInfo;
use summonr_agent::worktree::Tree;

use super::super::App;
use super::super::style::{ICON, S};
use super::{folder_name, shown_state, tone, topics};

/// What one sidebar pass reads: the live agents, the shown ones, the open terminals and the worktrees.
pub(super) struct Seen {
    pub(super) live: Vec<Live>,
    pub(super) shown: Vec<String>,
    pub(super) terms: Vec<Rc<super::super::terminal::Term>>,
    pub(super) trees: Vec<Tree>,
    pub(super) closed: Vec<PathBuf>,
    pub(super) shut: Signal<Vec<PathBuf>>,
}

/// One repository and its agents.
pub(super) type Group = (PathBuf, Vec<SessionInfo>);

/// A repository row at `depth`, then its terminals, its agents and its worktree branches.
pub(super) fn repository(app: &App, (folder, agents): Group, seen: &Seen, depth: u16) -> Vec<NodeBuilder> {
    let open = !seen.closed.contains(&folder);
    let states: Vec<Status> = agents
        .iter()
        .filter_map(|info| shown_state(&seen.live, &info.id))
        .collect();
    let mut nodes = vec![section(app, &folder, (open, depth), &states, seen.shut)];
    if !open {
        return nodes;
    }
    let branches = workspaces(&seen.trees, &folder);
    let (nested, loose): (Vec<SessionInfo>, Vec<SessionInfo>) = agents
        .into_iter()
        .partition(|info| branches.iter().any(|tree| ran_in(info, tree)));
    let inner = depth + 1;
    nodes.extend(super::super::terminal::rows(app, &seen.terms, &folder, depth));
    nodes.extend(topics(app, loose, (&seen.shown, &seen.live), depth));
    for tree in branches {
        nodes.push(super::super::topic::worktree_row(app, tree, depth));
        nodes.extend(super::super::terminal::rows(app, &seen.terms, &tree.path, inner));
        let inside = nested.iter().filter(|info| ran_in(info, tree)).cloned().collect();
        nodes.extend(topics(app, inside, (&seen.shown, &seen.live), inner));
    }
    nodes
}

/// The worktrees of `folder` a person made, by branch; summonr's own run worktrees stay out.
fn workspaces<'a>(trees: &'a [Tree], folder: &Path) -> Vec<&'a Tree> {
    let mut found: Vec<&Tree> = trees
        .iter()
        .filter(|tree| tree.primary == folder && tree.path != folder && tree.run_id().is_none())
        .collect();
    found.sort_by_cached_key(|tree| tree.branch.to_lowercase());
    found
}

fn ran_in(info: &SessionInfo, tree: &Tree) -> bool {
    info.cwd.as_deref().map(Path::new) == Some(tree.path.as_path())
}

/// A repository row that folds its agents; folded, it shows their strongest status and count.
fn section(
    app: &App,
    folder: &Path,
    (open, depth): (bool, u16),
    states: &[Status],
    shut: sigil::prelude::Signal<Vec<PathBuf>>,
) -> NodeBuilder {
    let flip = folder.to_path_buf();
    let strongest = [
        Status::NeedsYou,
        Status::Failed,
        Status::Working,
        Status::Review,
    ]
    .into_iter()
    .find(|status| states.contains(status));
    let summary = strongest.filter(|_| !open).map(|status| {
        let count = states.iter().filter(|seen| **seen == status).count();
        (count.to_string().into(), tone(status))
    });
    let folder_icon = if open { Icon::FolderOpen } else { Icon::Folder };
    let row = list_row(
        false,
        folder_name(folder),
        ListRowSlots {
            depth,
            leading: Some(Box::new(icon(folder_icon).px(ICON))),
            badge: summary,
            activate: Some(Rc::new(move || {
                shut.update(|closed| {
                    if let Some(at) = closed.iter().position(|seen| *seen == flip) {
                        closed.remove(at);
                    } else {
                        closed.push(flip.clone());
                    }
                });
            })),
            ..ListRowSlots::default()
        },
    )
    .expanded(sigil::prelude::signal(open));
    let (menu, folder) = (app.clone(), folder.to_path_buf());
    container()
        .layout(LayoutStyle::flex_row().fill_width().padding_top(S))
        .child(sigil::widgets::context_menu(row, move || {
            super::super::agent_menu::folder_items(&menu, &folder)
        }))
}

