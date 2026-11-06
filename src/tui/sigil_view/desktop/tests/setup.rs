use super::harness::{Step, graded, settings, steps};
use super::in_window;
use crate::endpoint::Local;
use crate::tui::{Cmd, Snapshot};

fn claude() -> Local {
    Local {
        route: "claude".to_owned(),
        endpoint: "anthropic".to_owned(),
        name: "Claude Code".to_owned(),
        model: "default".to_owned(),
    }
}

#[test]
fn a_first_run_offers_the_claude_code_it_found_in_one_click() {
    if !in_window("setup::a_first_run_offers_the_claude_code_it_found_in_one_click") {
        return;
    }
    let seen = steps(
        |app| {
            settings(Snapshot { unconfigured: true, ..graded() })(app);
            app.local.set(vec![claude()]);
        },
        vec![Step::Click("Use Claude Code")],
        |_| (),
    );
    let picked: Vec<_> = seen
        .sent
        .iter()
        .filter_map(|cmd| match cmd {
            Cmd::SelectModel(endpoint, model) => Some((endpoint.clone(), model.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(picked, vec![("anthropic".to_owned(), "default".to_owned())]);
    let routed = crate::endpoint::resolve(Some("anthropic"), Some("default"), crate::endpoint::Requirement::ModelAndKey);
    assert_eq!(routed.map(|endpoint| endpoint.name).ok().as_deref(), Some("claude"));
    assert!(seen.shows(|text| text == "Connect a model"), "{}", seen.text());
}
