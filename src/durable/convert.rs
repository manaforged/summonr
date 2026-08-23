//! Versioned converter for a legacy `.mage/plan.md` source.

use std::path::Path;

use thiserror::Error;

use crate::plan;

#[derive(Debug, Error)]
pub(crate) enum ConvertError {
    #[error("legacy plan is present but not readable markdown")]
    Malformed,
    #[error("legacy plan could not be read: {0}")]
    Io(#[from] std::io::Error),
}

/// Read the legacy plan file.
///
/// Absent → `Ok(None)`. Present well-formed UTF-8 markdown → `Ok(Some)`.
/// A present file that is empty, not UTF-8, or not a regular file blocks.
pub(crate) fn plan(workspace: &Path) -> Result<Option<String>, ConvertError> {
    let path = plan::path(workspace);
    match std::fs::metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConvertError::Io(error)),
        Ok(meta) if !meta.is_file() => Err(ConvertError::Malformed),
        Ok(_) => {
            let bytes = std::fs::read(&path)?;
            let text = String::from_utf8(bytes).map_err(|_error| ConvertError::Malformed)?;
            if text.trim().is_empty() {
                return Err(ConvertError::Malformed);
            }
            Ok(Some(text))
        }
    }
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
