use summonr_agent::child::{Access, ChildRecord, ChildState};
use summonr_agent::tools::{Hold, Kept};

fn completed(kept: Option<Kept>) -> ChildRecord {
    let record = ChildRecord {
        version: 1,
        id: "01a104ac000000000000000000001111".parse().expect("child id"),
        parent: "01a104ac000000000000000000002222".parse().expect("parent id"),
        assignment: "Edit the parser".to_owned(),
        model: "m".to_owned(),
        name: String::new(),
        agent: String::new(),
        workspace: "/w".into(),
        depth: 1,
        access: Access::Implement,
        background: false,
        call: Some("t1".to_owned()),
        base: String::new(),
        state: ChildState::Running,
    };
    let ChildState::Interrupted(mut result) = record.interrupt("unused").state else {
        panic!("interrupted result")
    };
    result.text = "edited".to_owned();
    result.kept = kept;
    ChildRecord {
        state: ChildState::Completed(result),
        ..record
    }
}

#[test]
fn a_child_whose_edits_were_kept_off_the_checkout_does_not_read_done() {
    let conflict = completed(Some(Kept {
        checkout: "/w-children/1111".into(),
        hold: Hold::Conflict,
    }));
    assert_eq!(super::finished(&conflict).word, "Conflict");
    assert_eq!(super::finished(&completed(None)).word, "Done");
}
