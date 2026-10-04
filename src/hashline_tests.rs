use super::*;

#[test]
fn line_tags_come_from_the_shared_stable_hash() {
    assert_eq!(crate::text::fnv64(b"a"), "af63dc4c8601ec8c");
    assert_eq!(line_hash("a"), "3r4c");
}
