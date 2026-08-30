use super::{Kind, Pair, pairs};

fn pair(left: Option<&str>, right: Option<&str>, kind: Kind) -> Pair {
    Pair {
        left: left.map(str::to_string),
        right: right.map(str::to_string),
        kind,
    }
}

#[test]
fn removed_and_added_runs_pair_up_index_by_index() {
    let text =
        "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n keep\n-old one\n-old two\n+new one\n tail\n+added\n";
    assert_eq!(
        pairs(text),
        vec![
            pair(Some("--- a/x"), None, Kind::Meta),
            pair(Some("+++ b/x"), None, Kind::Meta),
            pair(Some("@@ -1,3 +1,3 @@"), None, Kind::Hunk),
            pair(Some("keep"), Some("keep"), Kind::Context),
            pair(Some("old one"), Some("new one"), Kind::Change),
            pair(Some("old two"), None, Kind::Change),
            pair(Some("tail"), Some("tail"), Kind::Context),
            pair(None, Some("added"), Kind::Change),
        ]
    );
}

#[test]
fn an_empty_patch_has_no_rows() {
    assert!(pairs("").is_empty());
}
