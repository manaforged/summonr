use super::*;

#[test]
fn markdown_svg_data_uri_is_not_dumped() {
    let (lines, pictures) = markdown(
        "![tiny bar chart](data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=)\n",
        80,
        palette().accent,
    );
    assert!(pictures.is_empty());
    assert_eq!(lines[0].spans[0].text, "[tiny bar chart]");
}
