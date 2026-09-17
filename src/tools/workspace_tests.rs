use super::*;

#[test]
fn a_patch_past_the_last_newline_does_not_panic() {
    let original = "a\nb\n";
    let text = apply_patches(
        original,
        vec![Patch {
            start: 0,
            end: 8,
            anchor_start: 0,
            anchor_end: 2,
            replacement: vec!["x".to_string()],
        }],
    )
    .unwrap();
    assert_eq!(text, "x\n");
}
