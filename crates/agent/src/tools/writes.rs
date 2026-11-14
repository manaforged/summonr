use std::path::PathBuf;

use super::ToolRegistry;

impl ToolRegistry {
    pub(crate) fn clear_wrote(&self) {
        if let Ok(mut wrote) = self.wrote.lock() {
            wrote.clear();
        }
    }

    pub(crate) fn note_wrote(&self, path: &str) {
        let path = path.trim().trim_start_matches("./").replace('\\', "/");
        if path.is_empty() {
            return;
        }
        let full = self.workspace.join(&path);
        if let Some(layout) = full
            .ancestors()
            .find(|dir| dir.is_dir())
            .and_then(crate::diff::layout)
            && let Ok(mut repos) = self.repos.lock()
        {
            repos.insert(layout.top);
        }
        if let Ok(mut wrote) = self.wrote.lock() {
            wrote.insert(path);
        }
    }

    /// The git repositories this session has written to.
    pub(crate) fn repos(&self) -> Vec<PathBuf> {
        self.repos
            .lock()
            .map(|repos| repos.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Saves each file's current content to `turn`'s checkpoint before an edit tool rewrites it.
    pub(crate) fn checkpoint_edits(&self, points: crate::checkpoint::Checkpoints, turn: usize) {
        if let Ok(mut slot) = self.checkpoint.lock() {
            *slot = Some((points, turn));
        }
    }

    pub(super) fn save_before(&self, paths: &[String]) {
        let armed = self.checkpoint.lock().ok().and_then(|slot| slot.clone());
        if let Some((points, turn)) = armed {
            drop(points.capture_paths(turn, paths));
        }
    }

    pub(crate) fn wrote(&self) -> Vec<String> {
        self.wrote
            .lock()
            .map(|wrote| wrote.iter().cloned().collect())
            .unwrap_or_default()
    }
}
