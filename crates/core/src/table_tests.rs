use super::*;

#[derive(serde::Deserialize, Debug, PartialEq)]
struct Row {
    name: String,
    limit: u32,
}

#[test]
fn a_built_in_table_parses_into_its_type() {
    let row: Row = embedded("sample", r#"{"name":"read","limit":3}"#);
    assert_eq!(
        row,
        Row {
            name: "read".to_string(),
            limit: 3
        }
    );
}

#[test]
#[should_panic(expected = "the built-in sample table is invalid")]
fn an_invalid_built_in_table_names_the_table() {
    let _row: Row = embedded("sample", r#"{"name":"read","limit":"three"}"#);
}
