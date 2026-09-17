use super::*;

#[test]
fn busy_thinking_has_a_row_above_it() {
    let prompt = prompt_row(80, 40, "");
    let body = body_rows(80, 40, "", true);
    let last = 2 + body - 1;
    let status = prompt - 1;
    assert!(
        last + 1 < status,
        "last={last} status={status} prompt={prompt} body={body}"
    );
}
