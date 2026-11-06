use super::{TodoStatus, todos_from_args};

#[test]
fn todo_unknown_status_is_refused_instead_of_becoming_pending() {
    let error = todos_from_args(r#"{"items":[{"text":"drop the cache","status":"cancelled"}]}"#)
        .expect_err("an unknown status is refused");
    assert!(error.to_string().contains("cancelled"), "{error}");
}

#[test]
fn todo_list_allows_one_active_item() {
    let two = r#"{"items":[{"text":"a","status":"active"},{"text":"b","status":"in_progress"}]}"#;
    todos_from_args(two).expect_err("two active items are refused");
    let one = todos_from_args(r#"{"items":[{"text":"a","status":"active"},{"text":"b"}]}"#)
        .expect("one active item");
    assert_eq!(
        one.iter().map(|todo| todo.status).collect::<Vec<_>>(),
        [TodoStatus::Active, TodoStatus::Pending]
    );
}
