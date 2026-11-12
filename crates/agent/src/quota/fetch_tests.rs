use super::*;

#[test]
fn the_claude_code_plan_reads_the_file_its_readings_are_saved_to() {
    let plan = crate::quota::shipped()
        .plan
        .iter()
        .find(|plan| plan.name == "Claude Code")
        .expect("the Claude Code plan ships");
    let read = path(plan.file.as_deref().expect("the plan reads a file"));
    let saved = crate::quota::refresh::folder()
        .unwrap()
        .join(format!("{}.json", crate::quota::CLAUDE_CODE));
    assert_eq!(read, saved);
}
