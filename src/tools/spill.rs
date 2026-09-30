use std::path::Path;

use base64::Engine;

use crate::error::{Error, Result};
use crate::tools::budget;
use crate::tools::{ToolOutput, ToolRegistry};

const SNIFF_CHARS: usize = 24;

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
        let dir = tempfile::tempdir()?;
        let log_path = dir.path().join("output.log");
        std::fs::write(&log_path, raw)?;
        let note = full_output_note(&log_path, raw, true);
        self.keep_dir(dir);
        Ok(note.trim_start().to_string())
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

    pub(super) fn keep_dir(&self, dir: tempfile::TempDir) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(dir);
        }
    }
}

pub(super) fn full_output_note(log_path: &Path, raw: &str, cut: bool) -> String {
    if !cut {
        return String::new();
    }
    format!(
        "\n[full output: {} — {} lines]",
        log_path.display(),
        raw.lines().count()
    )
}
