use std::io::Cursor;

use image::{DynamicImage, ImageFormat, ImageReader, imageops::FilterType};

use super::{Error, Image, Result};

impl Image {
    pub(crate) fn fit(bytes: &[u8]) -> Result<Self> {
        let limits = crate::limits::limits();
        let side = limits.image_side_px;
        let format = image::guess_format(bytes).map_err(unreadable)?;
        let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
            .into_dimensions()
            .map_err(unreadable)?;
        let media = match format {
            ImageFormat::Png => Some("image/png"),
            ImageFormat::Jpeg => Some("image/jpeg"),
            _ => None,
        };
        if let Some(media) = media
            && width.max(height) <= side
            && bytes.len() <= limits.image_bytes
        {
            return Ok(Self::encode(media, bytes));
        }
        let mut picture = image::load_from_memory_with_format(bytes, format).map_err(unreadable)?;
        if width.max(height) > side {
            picture = picture.resize(side, side, FilterType::Triangle);
        }
        let png = written(&picture, ImageFormat::Png)?;
        if png.len() <= limits.image_bytes {
            return Ok(Self::encode("image/png", &png));
        }
        let flat = DynamicImage::ImageRgb8(picture.to_rgb8());
        Ok(Self::encode(
            "image/jpeg",
            &written(&flat, ImageFormat::Jpeg)?,
        ))
    }
}

fn written(picture: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    picture.write_to(&mut out, format).map_err(unreadable)?;
    Ok(out.into_inner())
}

fn unreadable(error: impl std::fmt::Display) -> Error {
    Error::Tool(format!("cannot read image: {error}"))
}
