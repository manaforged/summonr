use super::*;

fn written(workspace: &Path, name: &str, width: u32, height: u32, format: ::image::ImageFormat) {
    let picture = ::image::RgbaImage::from_fn(width, height, |x, y| {
        ::image::Rgba([(x % 251) as u8, (y % 241) as u8, 90, 255])
    });
    picture
        .save_with_format(workspace.join(name), format)
        .unwrap();
}

fn sent(image: &Image) -> (String, u32, u32) {
    let picture = ::image::load_from_memory(&image.bytes().unwrap()).unwrap();
    (image.media_type.clone(), picture.width(), picture.height())
}

#[test]
fn a_wide_screenshot_is_attached_at_most_2000px_on_its_long_side() {
    let workspace = tempfile::tempdir().unwrap();
    written(
        workspace.path(),
        "wide.png",
        3000,
        1000,
        ::image::ImageFormat::Png,
    );
    let input = attach(workspace.path(), "look @wide.png").unwrap();
    assert_eq!(sent(&input.images[0]), ("image/png".to_string(), 2000, 667));
}

#[test]
fn a_webp_attachment_reaches_the_model_as_png() {
    let workspace = tempfile::tempdir().unwrap();
    written(
        workspace.path(),
        "shot.webp",
        40,
        30,
        ::image::ImageFormat::WebP,
    );
    let input = attach(workspace.path(), "look @shot.webp").unwrap();
    assert_eq!(sent(&input.images[0]), ("image/png".to_string(), 40, 30));
}
