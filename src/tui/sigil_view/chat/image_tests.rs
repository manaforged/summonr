use std::io::Cursor;

use super::render;

fn png(pixels: &[[u8; 4]], width: u32, height: u32) -> Vec<u8> {
    let raw: Vec<u8> = pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect();
    let image = image::RgbaImage::from_raw(width, height, raw).expect("well-formed RGBA image");
    let dynamic = image::DynamicImage::ImageRgba8(image);
    let mut bytes = Vec::new();
    dynamic
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("PNG must encode");
    bytes
}

#[test]
fn render_lays_two_pixel_rows_per_cell() {
    // 4x4: four rows of solid colours, so each output cell stacks two rows.
    let image = png(
        &[
            [255, 0, 0, 255],
            [255, 0, 0, 255],
            [255, 0, 0, 255],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 255, 0, 255],
            [0, 255, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
        ],
        4,
        4,
    );

    let rows = render(&image, 80).expect("4x4 PNG must render");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 4);
    assert_eq!(rows[0][0].text, "▀");
    // Top row stacks red above green.
    assert_eq!(rows[0][0].fg.r, 255);
    assert_eq!(rows[0][0].fg.g, 0);
    assert_eq!(rows[0][0].bg.g, 255);
}

#[test]
fn render_does_not_upscale_past_the_source_width() {
    let image = png(&[[255, 0, 0, 255]; 16], 4, 4);
    let rows = render(&image, 80).expect("4x4 PNG must render");
    assert_eq!(rows[0].len(), 4);
}

#[test]
fn render_caps_a_tall_image_to_a_bounded_block() {
    // A 2x200 image becomes a very tall block; it must stop at MAX_ROWS.
    let mut pixels = Vec::new();
    for _ in 0..400 {
        pixels.push([10, 20, 30, 255]);
    }
    let image = png(&pixels, 2, 200);
    let rows = render(&image, 40).expect("tall image must render");
    assert_eq!(rows.len(), 24);
    assert_eq!(rows[0].len(), 2);
}

#[test]
fn render_rejects_undecodable_bytes() {
    assert!(render(b"not an image", 40).is_err());
    assert!(render(b"", 40).is_err());
}

#[test]
fn render_handles_an_odd_height_without_reading_out_of_bounds() {
    // 2x3: the final cell has no lower pixel row, so its background is the
    // chat background rather than a sampled pixel.
    let image = png(
        &[
            [255, 0, 0, 255],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
        ],
        2,
        3,
    );
    let rows = render(&image, 10).expect("odd-height PNG must render");
    assert_eq!(rows.len(), 2);
    let base = super::super::BASE;
    assert_eq!(rows[1][0].bg.r, base.r);
    assert_eq!(rows[1][0].bg.g, base.g);
    assert_eq!(rows[1][0].bg.b, base.b);
    assert_eq!(rows[1][0].bg.a, base.a);
}
