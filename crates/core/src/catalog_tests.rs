use super::*;
use serde::Deserialize;

#[derive(Deserialize, Debug, PartialEq)]
struct TestRow {
    name: String,
    #[serde(default)]
    old: Vec<String>,
}

impl Row for TestRow {
    fn name(&self) -> &str {
        &self.name
    }

    fn retired(&self) -> &[String] {
        &self.old
    }
}

fn rows() -> Vec<TestRow> {
    vec![
        TestRow {
            name: "new".to_string(),
            old: vec!["old".to_string()],
        },
        TestRow {
            name: "plain".to_string(),
            old: Vec::new(),
        },
    ]
}

#[test]
fn a_name_finds_its_row_and_an_alias_finds_its_replacement() {
    let catalog = Catalog::from_rows("test", rows()).unwrap();
    assert_eq!(catalog.get("new").map(|row| row.name.as_str()), Some("new"));
    assert_eq!(
        catalog.get("plain").map(|row| row.name.as_str()),
        Some("plain")
    );
    assert_eq!(catalog.get("old").map(|row| row.name.as_str()), Some("new"));
    assert!(catalog.get("missing").is_none());
    assert_eq!(catalog.rows().len(), 2);
}

#[test]
fn a_name_or_alias_listed_twice_is_refused() {
    let twice = Catalog::from_rows(
        "test",
        vec![
            TestRow {
                name: "x".to_string(),
                old: Vec::new(),
            },
            TestRow {
                name: "x".to_string(),
                old: Vec::new(),
            },
        ],
    );
    assert_eq!(twice.unwrap_err(), "`x` is listed twice in the test table");

    let alias = Catalog::from_rows(
        "test",
        vec![
            TestRow {
                name: "x".to_string(),
                old: Vec::new(),
            },
            TestRow {
                name: "y".to_string(),
                old: vec!["x".to_string()],
            },
        ],
    );
    assert_eq!(alias.unwrap_err(), "`x` is listed twice in the test table");
}
