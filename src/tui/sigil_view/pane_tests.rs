use super::{Layout, Pane};

#[test]
fn layouts_parse_print_and_split() {
    assert_eq!(Layout::parse("chat"), Some(Layout::default()));
    let run = Layout::parse("run").expect("preset");
    assert_eq!(run.secondary, Some(Pane::Ledger));
    assert_eq!(Layout::parse("chat | ledger 30%").expect("share").share, 30);
    assert_eq!(
        Layout::parse("chat | ledger 90").expect("clamped").share,
        60
    );
    assert_eq!(Layout::parse("ledger | chat"), None);
    assert_eq!(Layout::parse("chat | chat"), None);
    assert_eq!(run.text(), "chat | ledger 40%");
    assert_eq!(Layout::parse(&run.text()), Some(run));
    assert_eq!(run.split(120), (71, 48));
    assert_eq!(run.split(60), (60, 0), "too narrow to split");
    assert_eq!(Layout::default().split(120), (120, 0));
    assert_eq!(run.nudge(-30).share, 20);
    assert_eq!(run.nudge(30).share, 60);
}
