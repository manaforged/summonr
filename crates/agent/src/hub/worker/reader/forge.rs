use crate::forge::{self, Ask, State};
use crate::hub::{Fact, Msg};
use crate::scripts::{Script, Scripts, Workspace};
use crate::worktree::{self, Tree};

use super::Reader;

impl Reader {
    /// Answers `ask` off the worker; `own` is the asking session, which never blocks its own cleanup.
    pub(in crate::hub::worker) fn forge(&self, ask: Ask, own: String) {
        let reader = self.clone();
        drop(tokio::task::spawn_blocking(move || reader.ship(&ask, &own)));
    }

    pub(super) fn ship(&self, ask: &Ask, own: &str) {
        let reply = forge::answer(&self.workspace, ask);
        let merged = matches!(&reply, Ok(Some(pull)) if pull.state == State::Merged);
        let tree = merged.then(|| worktree::tree_at(&self.workspace)).flatten();
        self.send(Msg::Fact(Fact::Forge(reply)));
        if let Some(tree) = tree {
            self.clean_up(&tree, own);
        }
    }

    fn clean_up(&self, tree: &Tree, own: &str) {
        match worktree::dirty(&tree.path) {
            Ok(false) => {}
            Ok(true) => {
                return self.note(format!(
                    "kept worktree {}: it has uncommitted changes",
                    tree.branch
                ));
            }
            Err(error) => return self.note(format!("kept worktree {}: {error}", tree.branch)),
        }
        if let Err(note) = archive(tree) {
            return self.note(note);
        }
        match worktree::throw_away(tree, own) {
            Ok(()) => {
                self.note(format!("removed worktree {}", tree.branch));
                self.send(Msg::Fact(Fact::Moved(tree.primary.clone())));
            }
            Err(error) => self.note(format!("kept worktree {}: {error}", tree.branch)),
        }
    }

    fn note(&self, text: String) {
        self.send(Msg::Fact(Fact::System(text)));
    }
}

fn archive(tree: &Tree) -> Result<(), String> {
    let kept = |why: String| format!("kept worktree {}: {why}", tree.branch);
    let scripts = Scripts::load(&tree.primary).map_err(|error| kept(error.to_string()))?;
    let Some(script) = scripts.get(Script::Archive) else {
        return Ok(());
    };
    let workspace = Workspace::at(&tree.path).map_err(|error| kept(error.to_string()))?;
    let status = scripts
        .command(&workspace, script)
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|error| kept(format!("archive script did not start: {error}")))?;
    if status.success() {
        return Ok(());
    }
    Err(kept(format!(
        "archive script `{script}` exited with {}",
        status
            .code()
            .map_or_else(|| "a signal".to_owned(), |code| code.to_string())
    )))
}

#[cfg(all(test, unix))]
#[path = "forge_tests.rs"]
mod tests;
