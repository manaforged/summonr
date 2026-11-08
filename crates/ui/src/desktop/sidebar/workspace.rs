use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::icons::Icon;
use sigil::prelude::{ListRowSlots, NodeBuilder, Tone, list_row};
use sigil::widgets::icon;
use summonr_agent::hub::{Live, Open, Status};
use summonr_agent::session::SessionInfo;
use summonr_agent::worktree::Tree;

use super::super::App;
use super::super::style::ICON;

const NEEDS_YOU: &str = "Needs you";

/// One checkout of a repository and the agents that work in it.
struct Workspace {
    path: PathBuf,
    label: String,
    agents: Vec<SessionInfo>,
}

/// The rows of `folder`'s workspaces: its primary checkout, then each worktree, each followed by its topics.
pub(in crate::desktop) fn rows(
    app: &App,
    trees: &[Tree],
    folder: &Path,
    agents: Vec<SessionInfo>,
    (live, shown): (&[Live], &[String]),
) -> Vec<NodeBuilder> {
    let mut nodes = Vec::new();
    for space in workspaces(trees, folder, agents) {
        nodes.push(row(app, &space, live));
        for topic in super::super::topic::group(space.agents) {
            let selected = topic.threads.iter().any(|info| shown.contains(&info.id));
            nodes.extend(super::super::topic::row(app, &topic, selected, live));
        }
    }
    nodes
}

fn workspaces(trees: &[Tree], folder: &Path, agents: Vec<SessionInfo>) -> Vec<Workspace> {
    let mut spaces = vec![Workspace {
        path: folder.to_path_buf(),
        label: crate::app::read_git_branch(folder).unwrap_or_else(|| name(folder)),
        agents: Vec::new(),
    }];
    spaces.extend(trees.iter().filter(|tree| tree.primary == folder).map(|tree| Workspace {
        path: tree.path.clone(),
        label: if tree.branch.is_empty() { name(&tree.path) } else { tree.branch.clone() },
        agents: Vec::new(),
    }));
    for info in agents {
        let cwd = info.cwd.as_deref().map_or_else(|| folder.to_path_buf(), PathBuf::from);
        let home = spaces
            .iter()
            .enumerate()
            .filter(|(_, space)| cwd.starts_with(&space.path))
            .max_by_key(|(_, space)| space.path.as_os_str().len())
            .map(|(at, _)| at);
        match home {
            Some(at) => spaces[at].agents.push(info),
            None => spaces.push(Workspace {
                label: name(&cwd),
                path: cwd,
                agents: vec![info],
            }),
        }
    }
    spaces
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A workspace row: its branch, how many agents work there and their strongest state; opening it shows its newest agent or starts one.
fn row(app: &App, space: &Workspace, live: &[Live]) -> NodeBuilder {
    let state = space
        .agents
        .iter()
        .filter_map(|info| super::shown_state(live, &info.id))
        .max_by_key(|status| super::super::topic::rank(*status));
    let needs = state == Some(Status::NeedsYou);
    let count = match space.agents.len() {
        0 => None,
        1 => Some("1 agent".to_owned()),
        n => Some(format!("{n} agents")),
    };
    let open = match space.agents.first() {
        Some(newest) => Open::Resume(newest.id.clone()),
        None => Open::Folder(space.path.clone()),
    };
    let target = app.clone();
    let row = list_row(
        false,
        space.label.clone(),
        ListRowSlots {
            leading: Some(Box::new(icon(Icon::GitBranch).px(ICON))),
            status: count.filter(|_| !needs).map(Into::into),
            badge: needs.then(|| (NEEDS_YOU.into(), Tone::Warn)),
            activate: Some(Rc::new(move || super::super::split::show(&target, open.clone()))),
            ..ListRowSlots::default()
        },
    );
    let (menu, path) = (app.clone(), space.path.clone());
    sigil::widgets::context_menu(row, move || super::super::agent_menu::folder_items(&menu, &path))
}
