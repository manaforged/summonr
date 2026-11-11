use summonr_agent::hub::{Open, Status};
use summonr_agent::session::SessionInfo;
use summonr_agent::worktree::Tree;

use crate::view::keys::bindings::Jump;

use super::super::App;
use super::{projects, repo, row, space, state};

/// Opens the agent `to` names, in sidebar order.
pub(in crate::desktop) fn jump(app: &App, to: Jump) {
    let model = app.model.peek();
    let session = super::super::split::current(app)
        .model
        .peek()
        .session
        .clone();
    let order = ordered_ids(app);
    let at = order.iter().position(|id| *id == session);
    let target = match to {
        Jump::Nth(n) => order.get(n - 1),
        Jump::Step(forward) => {
            let len = order.len().max(1);
            let next = match (at, forward) {
                (Some(at), true) => (at + 1) % len,
                (Some(at), false) => (at + len - 1) % len,
                (None, _) => 0,
            };
            order.get(next)
        }
        Jump::Needing => {
            let needing = |id: &&String| state(&model.live, id) == Some(Status::NeedsYou);
            let after = at.map_or(0, |at| at + 1);
            order[after.min(order.len())..]
                .iter()
                .find(needing)
                .or_else(|| order.iter().find(needing))
        }
    };
    if let Some(id) = target.filter(|id| **id != session) {
        super::super::split::show(app, Open::Resume(id.clone()));
    }
}

fn ordered_ids(app: &App) -> Vec<String> {
    let trees = app.worktrees();
    let list = super::super::spaces::all();
    let (put, rest) = space::split(&list, row::listed(app));
    let mut added = super::super::spaces::repositories();
    added.extend(list.iter().flat_map(|space| space.folders.iter().cloned()));
    let groups = projects(&trees, &app.workspace(), (&added, rest));
    let (placed, loose) = space::by_space(&list, (put, groups));
    let mut order = super::super::shelf::pinned();
    for (_, theirs, mine) in placed {
        order.extend(topic_ids(theirs));
        for (folder, agents) in mine {
            order.extend(repo_ids(&trees, &folder, agents));
        }
    }
    for (folder, agents) in loose {
        order.extend(repo_ids(&trees, &folder, agents));
    }
    order
}

fn repo_ids(trees: &[Tree], folder: &std::path::Path, agents: Vec<SessionInfo>) -> Vec<String> {
    let (loose, branches) = repo::split_branches(trees, folder, agents);
    topic_ids(loose)
        .into_iter()
        .chain(
            branches
                .into_iter()
                .flat_map(|(_, inside)| topic_ids(inside)),
        )
        .collect()
}

fn topic_ids(agents: Vec<SessionInfo>) -> Vec<String> {
    super::super::topic::group(agents)
        .into_iter()
        .flat_map(|topic| topic.threads.into_iter().map(|info| info.id))
        .collect()
}
