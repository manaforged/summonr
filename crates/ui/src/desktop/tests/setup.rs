use super::super::friends::{Seen, State};
use super::harness::{Step, graded, settings, steps};
use super::in_window;
use summonr_agent::endpoint::{Friend, Local};
use summonr_agent::hub::{Cmd, Snapshot};

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
            settings(Snapshot {
                unconfigured: true,
                ..graded()
            })(app);
            app.window.local.set(vec![claude()]);
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
    let routed = summonr_agent::endpoint::resolve(
        Some("anthropic"),
        Some("default"),
        summonr_agent::endpoint::Requirement::ModelAndKey,
    );
    assert_eq!(
        routed.map(|endpoint| endpoint.name).ok().as_deref(),
        Some("claude")
    );
    assert!(
        seen.shows(|text| text == "Connect a model"),
        "{}",
        seen.text()
    );
}

fn seen(state: State) -> Seen {
    Seen {
        friend: Friend::of("claude").expect("the Claude Code preset"),
        state,
    }
}

fn claude_model() -> &'static str {
    let models = Friend::of("claude")
        .expect("the Claude Code preset")
        .models();
    let first = models.first().expect("Claude models").clone();
    assert!(first.starts_with("claude-"), "{first}");
    Box::leak(first.into_boxed_str())
}

#[test]
fn claude_code_is_listed_as_its_models_from_anthropic() {
    if !in_window("setup::claude_code_is_listed_as_its_models_from_anthropic") {
        return;
    }
    let shown = steps(
        |app| claude_friend(app, State::SignedIn),
        vec![Step::Click("Model")],
        |_| (),
    );
    assert!(
        shown.shows(|text| text == claude_model()),
        "{}",
        shown.text()
    );
    assert!(shown.shows(|text| text == "anthropic"), "{}", shown.text());
    assert!(
        shown
            .named(sigil::prelude::Role::MenuItem, "Claude Code")
            .is_empty(),
        "{:?}",
        shown.controls
    );
}

fn claude_friend(app: &super::super::App, state: State) {
    app.window.sidebar.set(true);
    app.window.friends.set(vec![seen(state)]);
}

#[test]
fn picking_a_signed_in_friend_moves_the_chat_to_its_route() {
    if !in_window("setup::picking_a_signed_in_friend_moves_the_chat_to_its_route") {
        return;
    }
    let moved = steps(
        |app| {
            claude_friend(app, State::SignedIn);
            super::super::friends::start(app, seen(State::SignedIn), claude_model().to_owned());
        },
        Vec::new(),
        |_| (),
    );
    let picked: Vec<_> = moved
        .sent
        .iter()
        .filter_map(|cmd| match cmd {
            Cmd::SelectModel(endpoint, model) => Some((endpoint.clone(), model.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        picked,
        vec![("claude".to_owned(), claude_model().to_owned())]
    );
}

#[test]
fn the_model_list_offers_an_installed_friend_and_no_sidebar_row() {
    if !in_window("setup::the_model_list_offers_an_installed_friend_and_no_sidebar_row") {
        return;
    }
    let shown = steps(
        |app| claude_friend(app, State::SignedOut),
        vec![Step::Click("Model")],
        |_| (),
    );
    assert_eq!(
        shown
            .named(sigil::prelude::Role::MenuItem, claude_model())
            .len(),
        1,
        "{:?}",
        shown.controls
    );
    assert!(
        !shown.shows(|text| text == "New Claude Code"),
        "{}",
        shown.text()
    );
}

#[test]
fn a_signed_out_friend_opens_its_own_sign_in_in_a_terminal() {
    if !in_window("setup::a_signed_out_friend_opens_its_own_sign_in_in_a_terminal") {
        return;
    }
    let runtime = sigil::tasks::AsyncRuntime::new().expect("runtime");
    runtime.install_current();
    let shown = steps(
        |app| claude_friend(app, State::SignedOut),
        Vec::new(),
        |app| {
            super::super::friends::start(app, seen(State::SignedOut), claude_model().to_owned());
            app.window
                .terminals
                .list
                .peek()
                .iter()
                .map(|term| term.label())
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(shown.read, vec!["Claude Code".to_owned()]);
    assert!(!shown.sent.iter().any(|cmd| matches!(cmd, Cmd::Open(_))),);
}

#[test]
fn settings_signs_in_to_a_friend_through_its_own_terminal() {
    if !in_window("setup::settings_signs_in_to_a_friend_through_its_own_terminal") {
        return;
    }
    let runtime = sigil::tasks::AsyncRuntime::new().expect("runtime");
    runtime.install_current();
    let shown = steps(
        |app| {
            claude_friend(app, State::SignedOut);
            app.settings_open.set(true);
        },
        vec![Step::Click("Accounts"), Step::Click("Sign in, Claude Code")],
        |app| {
            app.window
                .terminals
                .list
                .peek()
                .iter()
                .map(|term| term.label())
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(shown.read, vec!["Claude Code".to_owned()]);
}

#[test]
fn clicking_a_friend_in_the_model_menu_acts_after_the_menu_closes() {
    if !in_window("setup::clicking_a_friend_in_the_model_menu_acts_after_the_menu_closes") {
        return;
    }
    let runtime = sigil::tasks::AsyncRuntime::new().expect("runtime");
    runtime.install_current();
    let runtime = std::cell::RefCell::new(runtime);
    let shown = steps(
        |app| claude_friend(app, State::SignedOut),
        vec![Step::Click("Model"), Step::Click(claude_model())],
        |app| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let acted = || app.settings_open.peek() || !app.window.terminals.list.peek().is_empty();
            while !acted() && std::time::Instant::now() < deadline {
                runtime.borrow_mut().pump();
                std::thread::park_timeout(std::time::Duration::from_millis(10));
            }
            acted()
        },
    );
    let moved = shown
        .sent
        .iter()
        .any(|cmd| matches!(cmd, Cmd::SelectModel(endpoint, _) if endpoint == "claude"));
    assert!(shown.read || moved, "{}", shown.text());
}

#[test]
fn a_missing_friend_is_not_in_the_model_list() {
    if !in_window("setup::a_missing_friend_is_not_in_the_model_list") {
        return;
    }
    let shown = steps(
        |app| claude_friend(app, State::Missing),
        vec![Step::Click("Model")],
        |_| (),
    );
    assert!(
        shown
            .named(sigil::prelude::Role::MenuItem, claude_model())
            .is_empty(),
        "{:?}",
        shown.controls
    );
}

#[test]
fn an_offer_found_after_the_window_opens_still_shows() {
    if !in_window("setup::an_offer_found_after_the_window_opens_still_shows") {
        return;
    }
    let seen = steps(
        settings(Snapshot {
            unconfigured: true,
            ..graded()
        }),
        vec![
            Step::Wait(Box::new(|app| app.window.local.set(vec![claude()]))),
            Step::Click("Use Claude Code"),
        ],
        |_| (),
    );
    assert!(
        seen.sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::SelectModel(endpoint, _) if endpoint == "anthropic"))
    );
}
