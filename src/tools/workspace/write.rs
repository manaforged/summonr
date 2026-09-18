use std::io::Write;
use std::path::Path;

use super::ToolRegistry;
use crate::error::{Error, Result};

impl ToolRegistry {
    pub(in crate::tools) fn atomic_write(
        &self,
        dest: &Path,
        display: &str,
        content: &str,
    ) -> Result<String> {
        let before = std::fs::read_to_string(dest).unwrap_or_default();
        if let Some((option, detail)) =
            crate::prefs::banned_edit(dest, &before, content, &self.allowed_edits)
        {
            return Err(Error::EditApproval {
                setting: option,
                detail,
            });
        }
        let preview = Self::edit_view(&before, content)?;
        let parent = dest
            .parent()
            .ok_or_else(|| Error::Tool(format!("cannot write `{display}` without a parent")))?;
        let existing_mode = match std::fs::metadata(dest) {
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(Error::Tool(format!(
                    "cannot inspect `{display}` permissions: {error}"
                )));
            }
        };
        let mut temp = tempfile::Builder::new()
            .prefix(".mage-tmp-")
            .tempfile_in(parent)
            .map_err(|error| Error::Tool(format!("cannot prepare `{display}`: {error}")))?;
        temp.write_all(content.as_bytes())
            .map_err(|error| Error::Tool(format!("cannot write `{display}`: {error}")))?;
        if let Some(permissions) = existing_mode {
            temp.as_file()
                .set_permissions(permissions)
                .map_err(|error| {
                    Error::Tool(format!("cannot preserve `{display}` permissions: {error}"))
                })?;
        }
        temp.persist(dest)
            .map_err(|error| Error::Tool(format!("cannot replace `{display}`: {error}")))?;
        Ok(preview)
    }

    fn edit_view(before: &str, after: &str) -> Result<String> {
        let (first, last) = crate::hashline::dirty_span(before, after);
        let tagged =
            crate::hashline::tagged_span(after, first.saturating_sub(1), last.saturating_add(1))
                .lines()
                .map(|line| crate::child::bounded(line, 160))
                .collect::<Vec<_>>()
                .join("\n");
        if before == after {
            return Ok(tagged);
        }
        let directory = tempfile::tempdir()?;
        crate::durable::store::write(&directory.path().join("before"), before)?;
        crate::durable::store::write(&directory.path().join("after"), after)?;
        let diff = crate::diff::git_env(
            directory.path(),
            &[],
            &[
                "diff",
                "--no-index",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--unified=3",
                "--",
                "before",
                "after",
            ],
        )?;
        let hunks = diff
            .lines()
            .skip_while(|line| !line.starts_with("@@"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!("{tagged}\n{hunks}"))
    }

    pub(crate) fn allow_edit(&mut self, setting: &'static str) {
        if !self.allowed_edits.contains(&setting) {
            self.allowed_edits.push(setting);
        }
    }

    pub(crate) fn reset_edits(&mut self) {
        self.allowed_edits.clear();
    }
}
