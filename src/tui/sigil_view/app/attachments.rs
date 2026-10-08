use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn paste_text(&self, text: &str) -> bool {
        if self.options_active() || self.view_edit.peek() || self.pane_focus.peek() {
            return false;
        }
        let lines = text.lines().count();
        if lines < crate::limits::limits().paste_fold_lines
            && text.len() < crate::limits::limits().paste_fold_chars
        {
            return false;
        }
        let index = {
            let mut store = self.pasted_text.borrow_mut();
            store.push(text.to_string());
            store.len()
        };
        let token = paste_token(lines, index);
        self.editor.insert(&token);
        self.change(&self.draft.peek());
        self.refresh();
        true
    }

    pub(in crate::tui::sigil_view) fn expand_pastes(&self, text: &str) -> String {
        let store = self.pasted_text.borrow();
        let mut out = text.to_string();
        for (index, pasted) in store.iter().enumerate() {
            out = out.replace(&paste_token(pasted.lines().count(), index + 1), pasted);
        }
        out
    }

    pub(in crate::tui::sigil_view) fn attach(&self, text: &str) -> Option<UserInput> {
        let input = if text.trim_start().starts_with('!') {
            Ok(text.into())
        } else {
            crate::tools::attach(&self.workspace, text)
        };
        match input {
            Ok(input) => Some(input),
            Err(error) => {
                self.push_entry(Entry::Error(format!("cannot attach files: {error}")));
                self.refresh();
                None
            }
        }
    }

    pub(in crate::tui::sigil_view) fn take_attachments(&self) -> Vec<Attachment> {
        let taken = std::mem::take(&mut *self.attachments.borrow_mut());
        self.attached.set(0);
        taken
    }

    pub(in crate::tui::sigil_view) fn attach_picture(
        &self,
        image: crate::provider::Image,
        source: ImageSource,
        label: String,
    ) {
        let mut attachments = self.attachments.borrow_mut();
        attachments.push(Attachment::new(image, source, label));
        let count = attachments.len();
        drop(attachments);
        self.attached.set(count);
        self.splash.set(false);
    }

    pub(in crate::tui::sigil_view) fn paste(&self, text: &str) -> bool {
        if self.options_active() || self.view_edit.peek() {
            return false;
        }
        if let Some(path) = image_path(&self.workspace, text) {
            match read_image(&path) {
                Ok(Some((image, source))) => {
                    self.paste_image(image, source, format!("image · {}", path.display()));
                }
                Ok(None) => return false,
                Err(error) => {
                    self.push_entry(Entry::Error(format!(
                        "cannot attach image {}: {error}",
                        path.display()
                    )));
                    self.refresh();
                }
            }
            return true;
        }
        if let Some((image, source)) = image_from_bytes(text.as_bytes()) {
            self.paste_image(image, source, "image · pasted".to_string());
            return true;
        }
        false
    }

    pub(in crate::tui::sigil_view) fn clipboard(&self) -> bool {
        if !cfg!(target_os = "macos") || self.options_active() || self.view_edit.peek() {
            return false;
        }
        drop(self.cmd.send(Cmd::Clipboard));
        true
    }

    pub(super) fn pasted(&self, read: std::result::Result<Vec<u8>, String>) {
        match read {
            Ok(bytes) if bytes.len() > IMAGES_MAX_BYTES => {
                self.push_entry(Entry::Error(
                    "cannot attach clipboard image: image exceeds the 10 MB limit".to_string(),
                ));
            }
            Ok(bytes) => {
                if let Some((image, source)) = image_from_bytes(&bytes) {
                    self.attach_picture(image, source, "image · clipboard".to_string());
                } else {
                    let text = String::from_utf8_lossy(&bytes);
                    if !text.is_empty() && !self.paste(&text) && !self.paste_text(&text) {
                        self.editor.insert(&text);
                        self.change(&self.draft.peek());
                    }
                }
            }
            Err(error) => self.model_mut(|model| {
                model.entries.push(Entry::Error(format!(
                    "cannot read the clipboard: {error}; paste an image file path instead"
                )))
            }),
        }
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn paste_image(
        &self,
        image: Image,
        source: ImageSource,
        label: String,
    ) {
        self.attach_picture(image, source, label);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn mention(&self, path: &str) {
        let mut draft = self.draft.peek();
        if !draft.is_empty() && !draft.ends_with(' ') {
            draft.push(' ');
        }
        draft.push_str(&super::suggest::reference(path));
        draft.push(' ');
        self.replace(draft);
        self.pane_focus.set(false);
        self.refresh();
    }
}

fn paste_token(lines: usize, index: usize) -> String {
    format!("[pasted {} #{index}]", crate::text::plural(lines, "line"))
}

fn image_path(workspace: &Path, text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    if let Some(path) = file_url(trimmed) {
        return (path.is_file() || image::ImageFormat::from_path(&path).is_ok()).then_some(path);
    }
    let quoted = trimmed.starts_with(['"', '\'', '(']);
    let text = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .or_else(|| {
            trimmed
                .strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
        })
        .unwrap_or(trimmed)
        .trim();
    if text.is_empty() || text.contains('\n') {
        return None;
    }
    let path = PathBuf::from(text);
    let expected = image::ImageFormat::from_path(&path).is_ok()
        && (quoted || path.is_absolute() || !text.chars().any(char::is_whitespace));
    let path = if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    };
    (path.is_file() || expected).then_some(path)
}

fn read_image(path: &Path) -> Result<Option<(Image, ImageSource)>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(IMAGES_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > IMAGES_MAX_BYTES {
        return Err("image exceeds the 10 MB limit".to_string());
    }
    let Some(media_type) = crate::tools::image_type(&bytes) else {
        return if image::ImageFormat::from_path(path).is_ok() {
            Err("expected a PNG, JPEG, GIF, or WebP image".to_string())
        } else {
            Ok(None)
        };
    };
    let source = preview(&bytes)?;
    Ok(Some((Image::encode(media_type, &bytes), source)))
}

fn image_from_bytes(bytes: &[u8]) -> Option<(Image, ImageSource)> {
    let media = crate::tools::image_type(bytes)?;
    let source = preview(bytes).ok()?;
    Some((Image::encode(media, bytes), source))
}
