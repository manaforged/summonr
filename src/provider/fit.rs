use std::io::Cursor;

use image::{DynamicImage, ImageFormat, ImageReader, imageops::FilterType};

use super::{Error, Image, Result};

const SENT_AS_IS: [(ImageFormat, &str); 2] = [
    (ImageFormat::Png, "image/png"),
    (ImageFormat::Jpeg, "image/jpeg"),
];

pub(crate) fn fit(bytes: &[u8]) -> Result<Image> {
    let limits = crate::limits::limits();
    let side = limits.image_side_px;
    let format = image::guess_format(bytes).map_err(unreadable)?;
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(unreadable)?;
    let media = SENT_AS_IS
        .iter()
        .find(|(sent, _)| *sent == format)
        .map(|(_, media)| *media);
    if let Some(media) = media
        && width.max(height) <= side
        && bytes.len() <= limits.image_bytes
    {
        return Ok(Image::encode(media, bytes));
    }
    let mut picture = image::load_from_memory_with_format(bytes, format).map_err(unreadable)?;
    if width.max(height) > side {
        picture = picture.resize(side, side, FilterType::Triangle);
    }
    let png = written(&picture, ImageFormat::Png)?;
    if png.len() <= limits.image_bytes {
        return Ok(Image::encode("image/png", &png));
    }
    let flat = DynamicImage::ImageRgb8(picture.to_rgb8());
    Ok(Image::encode(
        "image/jpeg",
        &written(&flat, ImageFormat::Jpeg)?,
    ))
}

fn written(picture: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    picture.write_to(&mut out, format).map_err(unreadable)?;
    Ok(out.into_inner())
}

fn unreadable(error: impl std::fmt::Display) -> Error {
    Error::Tool(format!("cannot read image: {error}"))
}
