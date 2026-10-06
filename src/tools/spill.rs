use std::path::{Path, PathBuf};

use base64::Engine;

use crate::durable::store;
use crate::error::{Error, Result};
use crate::tools::budget;
use crate::tools::{ToolOutput, ToolRegistry};

const SNIFF_CHARS: usize = 24;
const SPILL_PREFIX: &str = "output-";
const SPILL_SUFFIX: &str = ".log";

impl ToolRegistry {
    pub(crate) fn fit_output(&self, result: Result<ToolOutput>) -> Result<ToolOutput> {
        match result {
            Ok(mut output) => {
                output.text = self.bounded(&output.text);
                self.fit_images(&mut output);
                Ok(output)
            }
            Err(Error::Tool(text)) => Err(Error::Tool(self.bounded(&text))),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn bounded(&self, raw: &str) -> String {
        if budget::within(raw) {
            return raw.trim_end().to_string();
        }
        let shown = budget::plain(raw);
        if !shown.cut {
            return shown.text;
        }
        match self.spill(raw) {
            Ok(note) => format!("{note}\n{}", shown.text),
            Err(error) => format!("[full output not saved: {error}]\n{}", shown.text),
        }
    }

    fn spill(&self, raw: &str) -> Result<String> {
        let log_path = self.spill_file(SPILL_PREFIX, raw.as_bytes())?;
        let note = full_output_note(&log_path, true);
        Ok(note.trim_start().to_string())
    }

    pub(super) fn spill_file(&self, prefix: &str, bytes: &[u8]) -> Result<PathBuf> {
        Ok(store::keep(self.spill_dir()?, prefix, SPILL_SUFFIX, bytes)?)
    }

    fn spill_dir(&self) -> Result<&Path> {
        if let Some(dir) = self.spill_dir.get() {
            return Ok(dir.path());
        }
        drop(self.spill_dir.set(tempfile::tempdir()?));
        self.spill_dir
            .get()
            .map(tempfile::TempDir::path)
            .ok_or_else(|| Error::Tool("spill directory is unavailable".to_string()))
    }

    fn fit_images(&self, output: &mut ToolOutput) {
        let total = output.images.len();
        if self.model_images {
            output.images.retain(|image| {
                let head = &image.data[..image.data.floor_char_boundary(SNIFF_CHARS)];
                base64::engine::general_purpose::STANDARD
                    .decode(head)
                    .ok()
                    .and_then(|bytes| super::workspace::image_type(&bytes))
                    == Some(image.media_type.as_str())
            });
        } else {
            output.images.clear();
        }
        let dropped = total - output.images.len();
        if dropped > 0 {
            output.text.push_str(&format!(
                "\n[{dropped} image(s) omitted: this model does not take them, or they are not PNG, JPEG, GIF, or WebP]"
            ));
        }
    }
}

pub(super) fn full_output_note(log_path: &Path, cut: bool) -> String {
    if !cut {
        return String::new();
    }
    if let Ok(Some(lines)) = crate::diff::line_count(log_path) {
        return format!("\n[full output: {} ({lines} lines)]", log_path.display());
    }
    let size = std::fs::metadata(log_path).map_or(0, |meta| meta.len());
    format!("\n[full output: {} ({size} bytes)]", log_path.display())
}
