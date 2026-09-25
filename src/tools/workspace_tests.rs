use super::edit::{Patch, apply_patches};

#[test]
fn a_patch_past_the_last_newline_does_not_panic() {
    let original = "a\nb\n";
    let (text, _) = apply_patches(
        original,
        vec![Patch {
            start: 0,
            end: 8,
            anchor_start: 0,
            anchor_end: 2,
            replacement: vec!["x".to_string()],
            relaxed: false,
        }],
    )
    .unwrap();
    assert_eq!(text, "x\n");
}

#[test]
fn an_edit_preview_ends_with_a_unified_hunk() {
    let hunk = super::write::unified_hunk("a\nb\nc\nd\n", "a\nb\nx\ny\nd\n");
    assert_eq!(hunk, "@@ -2,3 +2,4 @@\n b\n-c\n+x\n+y\n d");
}
