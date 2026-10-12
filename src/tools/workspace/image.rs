use std::io::Read as _;
use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::provider::{CompletionRequest, Image, Message, Provider, Source, UserInput};

use super::{ToolOutput, ToolRegistry, object_string, parse_args};

pub(crate) const IMAGES_MAX_BYTES: usize = 10 * 1024 * 1024;

impl ToolRegistry {
    pub(super) fn show_image(&self, arguments: &str) -> Result<ToolOutput> {
        let args: Value = parse_args("show_image", arguments)?;
        let path = object_string(&args, "path")?;
        let canonical = self.resolve_read(path, crate::tools::paths::Shield::Contained)?;
        let mut head = Vec::new();
        std::fs::File::open(&canonical)
            .and_then(|file| file.take(12).read_to_end(&mut head))
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        image_type(&head).ok_or_else(|| {
            Error::Tool(format!("`{path}` is not a PNG, JPEG, GIF, or WebP image"))
        })?;
        self.read_file(arguments)
    }

    pub(super) fn image_outcome(
        &self,
        path: &str,
        media: &str,
        bytes: &[u8],
        width: u32,
        height: u32,
    ) -> Result<ToolOutput> {
        if self.model_images {
            return Ok(ToolOutput {
                images: vec![Image::fit(bytes)?],
                ..ToolOutput::ok(format!("{path} ({media}, {width}x{height}) is attached"))
            });
        }
        if let Some(endpoint) = &self.vision {
            let images = [Image::fit(bytes)?];
            let described =
                tokio::runtime::Handle::current().block_on(self.narrate_images(endpoint, &images));
            return match described {
                Ok(text) => Ok(ToolOutput::ok(format!(
                    "{path} ({media}, {width}x{height}) described by {}: {text}",
                    endpoint.name
                ))),
                Err(error) => Err(Error::Tool(format!(
                    "vision endpoint `{}` failed on `{path}`: {error}",
                    endpoint.name
                ))),
            };
        }
        Ok(ToolOutput::ok(format!(
            "{path} shown to the user ({media}, {width}x{height}); the model does not receive it; attach it with @{path} if the model accepts image input"
        )))
    }

    pub(crate) async fn fold_unseen_images(&self, input: &mut UserInput) -> Result<()> {
        if self.model_images || input.images.is_empty() {
            return Ok(());
        }
        let count = input.images.len();
        let note = match &self.vision {
            Some(endpoint) => match self.narrate_images(endpoint, &input.images).await {
                Ok(text) => format!(
                    "[attached {count} image(s), described by {}: {}]",
                    endpoint.name,
                    text.trim()
                ),
                Err(error) => format!(
                    "[attached {count} image(s); vision endpoint `{}` failed: {error}; the model cannot see them]",
                    endpoint.name
                ),
            },
            None => format!(
                "[attached {count} image(s); the active model does not accept image input, so they were not sent]"
            ),
        };
        input.text = if input.text.trim().is_empty() {
            note
        } else {
            format!("{}\n\n{note}", input.text)
        };
        input.images.clear();
        Ok(())
    }

    async fn narrate_images(
        &self,
        endpoint: &crate::endpoint::Endpoint,
        images: &[Image],
    ) -> Result<String> {
        let client = crate::provider::Client::connect(endpoint)?;
        let messages = [Message::user(UserInput {
            text: "Describe this image so an agent that cannot see it can act on it. State what it shows, name any text it contains, and be concise.".to_string(),
            images: images.to_vec(),
            source: None,
        })];
        let request = CompletionRequest::new(
            "vision".to_string(),
            endpoint.model.clone(),
            &messages,
            &[],
            Some(512),
            false,
            None,
        );
        let reply = client.complete(&request).await?;
        Ok(reply.content.unwrap_or_default())
    }
}

pub(crate) fn attach(workspace: &Path, line: &str) -> Result<UserInput> {
    let mut out = String::new();
    let mut images = Vec::new();
    let mut image_bytes = 0_usize;
    let mut attached = false;
    for word in words(line) {
        let path = word.strip_prefix('@').map(|path| {
            if path.starts_with('"') {
                serde_json::from_str::<String>(path).unwrap_or_else(|_| path.to_string())
            } else {
                path.to_string()
            }
        });
        if let Some(path) = path.as_deref()
            && !path.is_empty()
        {
            let Some(bytes) = read_attachment(workspace, word, path)? else {
                continue;
            };
            if image_type(&bytes).is_some() {
                image_bytes = image_bytes.saturating_add(bytes.len());
                if image_bytes > IMAGES_MAX_BYTES {
                    return Err(Error::Tool(format!(
                        "attached images are larger than {IMAGES_MAX_BYTES} bytes"
                    )));
                }
                images.push(Image::fit(&bytes)?);
            } else {
                let text = String::from_utf8(bytes).map_err(|error| {
                    Error::Tool(format!(
                        "`{path}` is not a supported image or UTF-8 text: {error}"
                    ))
                })?;
                out.push_str(&format!("\n--- {path} ---\n{text}\n"));
            }
            attached = true;
        }
    }
    let text = if out.is_empty() {
        line.to_string()
    } else {
        format!("{line}\n{out}")
    };
    let source = attached.then(|| Source {
        text: line.to_string(),
        images: images.len(),
    });
    Ok(UserInput {
        text,
        images,
        source,
    })
}

fn read_attachment(workspace: &Path, word: &str, path: &str) -> Result<Option<Vec<u8>>> {
    let full = workspace.join(path);
    if !full.is_file() {
        if !word.starts_with("@\"") {
            return Ok(None);
        }
        return Err(Error::Tool(format!(
            "attachment `{path}` is not a readable file; check the path or remove the @ reference"
        )));
    }
    let root = workspace
        .canonicalize()
        .map_err(|error| Error::Tool(format!("cannot open workspace: {error}")))?;
    let canonical = full
        .canonicalize()
        .map_err(|error| Error::Tool(format!("cannot open attachment `{path}`: {error}")))?;
    if !canonical.starts_with(&root) {
        return Err(Error::Tool(format!(
            "attachment `{path}` is outside the workspace"
        )));
    }
    std::fs::read(&canonical)
        .map(Some)
        .map_err(|error| Error::Tool(format!("cannot read attachment `{path}`: {error}")))
}

fn words(mut line: &str) -> Vec<&str> {
    let mut words = Vec::new();
    loop {
        line = line.trim_start();
        if line.is_empty() {
            return words;
        }
        let quoted = line.strip_prefix("@\"").and_then(|_| {
            let mut parsed = serde_json::Deserializer::from_str(&line[1..]).into_iter::<String>();
            matches!(parsed.next(), Some(Ok(_))).then(|| parsed.byte_offset() + 1)
        });
        let end = quoted.unwrap_or_else(|| line.find(char::is_whitespace).unwrap_or(line.len()));
        words.push(&line[..end]);
        line = &line[end..];
    }
}

pub(crate) fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod image_tests;
