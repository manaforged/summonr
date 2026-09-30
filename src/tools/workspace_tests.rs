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
fn an_edit_names_each_changed_region_and_nothing_it_did_not_touch() {
    let before: String = (1..400).map(|n| format!("line {n}\n")).collect();
    let after = before
        .replace("line 100\n", "alpha\nalpha extra\n")
        .replace("line 250\n", "beta\n");
    let spans = super::spans::changed(&before, &after);
    assert_eq!(
        super::spans::summary(&before, &after, &spans),
        "changed lines 100-101, line 251 (+1 line, 400 lines now)"
    );
}
