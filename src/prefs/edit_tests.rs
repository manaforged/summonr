use super::banned_edit;
use std::path::Path;

#[test]
fn agents_md_owns_comments_and_tests_not_the_harness() {
    let dest = Path::new("src/foo_tests.rs");
    let after = "#[test]\nfn x() {}\n// a comment\n";
    assert_eq!(banned_edit(dest, "", after, &[]), None);
}
