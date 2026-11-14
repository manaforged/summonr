use std::path::{Path, PathBuf};

use summonr_core::text::Frontmatter;
use summonr_core::walk::Walk;

const DIR: &str = "rules";
const PATHS: &str = "paths";
const EXTENSION: &str = "md";

/// Each rule under `root`'s `.claude/rules` that loads at launch, with its frontmatter removed, in path order.
pub(super) fn launch(root: &Path) -> Vec<(PathBuf, String)> {
    let dir = summonr_core::dirs::claude(root).join(DIR);
    if !dir.is_dir() {
        return Vec::new();
    }
    Walk {
        sorted: true,
        ..Walk::RAW
    }
    .builder(&dir)
    .follow_links(true)
    .build()
    .filter_map(Result::ok)
    .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
    .map(|entry| entry.into_path())
    .filter(|path| path.extension().is_some_and(|ext| ext == EXTENSION))
    .filter_map(|path| {
        let text = std::fs::read_to_string(&path).ok()?;
        let front = Frontmatter::parse(&text);
        let scoped = front.list(PATHS).is_some_and(|globs| !globs.is_empty());
        (!scoped).then(|| (path, front.body.to_string()))
    })
    .collect()
}
