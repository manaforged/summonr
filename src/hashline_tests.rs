use super::*;

#[test]
fn hash_is_four_chars_and_stable() {
    let hash = line_hash("hello");
    assert_eq!(hash.len(), 4);
    assert_eq!(hash, line_hash("hello"));
    assert_ne!(hash, line_hash("world"));
}

#[test]
fn diff_hunk_marks_removed_and_added_lines() {
    let replaced = diff_hunk("a\nb\nc\n", "a\nB\nc\n").expect("replace");
    assert_eq!(replaced, "@@ -1,3 +1,3 @@\n a\n-b\n+B\n c");
    let inserted = diff_hunk("a\nc\n", "a\nb\nc\n").expect("insert");
    assert_eq!(inserted, "@@ -1,2 +1,3 @@\n a\n+b\n c");
    let deleted = diff_hunk("a\nb\nc\n", "a\nc\n").expect("delete");
    assert_eq!(deleted, "@@ -1,3 +1,2 @@\n a\n-b\n c");
    assert!(diff_hunk("same\n", "same\n").is_none());
}

#[test]
fn hash_only_replace_survives_insert_above() -> Result<()> {
    let original = "hello\nworld\n";
    let hello = line_hash("hello");
    let world = line_hash("world");
    let inserted = apply_insert(original, &hello, "middle")?;
    assert_eq!(inserted, "hello\nmiddle\nworld\n");
    let replaced = apply_replace(&inserted, &world, None, "earth")?;
    assert_eq!(replaced, "hello\nmiddle\nearth\n");
    Ok(())
}

#[test]
fn numbered_tag_is_stale_after_insert() -> Result<()> {
    let original = "hello\nworld\n";
    let numbered = format!("2:{}", line_hash("world"));
    let inserted = apply_insert(original, &line_hash("hello"), "mid")?;
    apply_replace(&inserted, &numbered, None, "x").unwrap_err();
    Ok(())
}

#[test]
fn multiline_replace_and_delete() -> Result<()> {
    let original = "a\nb\nc\nd\n";
    let replaced = apply_replace(original, &line_hash("b"), Some(&line_hash("c")), "x\ny")?;
    assert_eq!(replaced, "a\nx\ny\nd\n");
    let deleted = apply_delete(&replaced, &line_hash("x"), Some(&line_hash("y")))?;
    assert_eq!(deleted, "a\nd\n");
    Ok(())
}

#[test]
fn duplicate_hash_requires_line_number() -> Result<()> {
    let content = "same\nsame\n";
    let hash = line_hash("same");
    locate(content, &hash).unwrap_err();
    assert_eq!(locate(content, &format!("2:{hash}"))?, 1);
    Ok(())
}

#[test]
fn copied_display_tags_are_valid_edit_anchors() -> Result<()> {
    let content = "hello\nworld\n";
    let hello = format!("1:{}|hello", line_hash("hello"));
    let world = format!("2:{}|world", line_hash("world"));

    let replaced = apply_replace(content, &hello, Some(&world), "done")?;

    assert_eq!(replaced, "done\n");
    Ok(())
}

#[test]
fn copied_hash_and_text_is_a_valid_edit_anchor() -> Result<()> {
    let content = "hello\nworld\n";
    let hello = format!("{}|hello", line_hash("hello"));

    let replaced = apply_replace(content, &hello, None, "done")?;

    assert_eq!(replaced, "done\nworld\n");
    Ok(())
}

#[test]
fn a_non_tag_is_named_as_such_not_as_stale() {
    let error = parse_tag("}").expect_err("not a tag").to_string();
    assert!(
        error.contains("is not a tag") && error.contains("N:hash"),
        "{error}"
    );
}
