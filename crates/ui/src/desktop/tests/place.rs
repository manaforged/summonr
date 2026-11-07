use std::path::PathBuf;

use sigil::prelude::Role;

use super::harness::{Step, graded, settings, steps};
use super::in_window;
use summonr_agent::hub::Open;
use summonr_agent::hub::{Cmd, Fact, Msg, Snapshot};
use summonr_agent::session::SessionInfo;

fn shop() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("root");
    let folder = root.path().join("shop");
    std::fs::create_dir_all(folder.join(".git")).expect("git dir");
    std::fs::write(folder.join(".git/HEAD"), "ref: refs/heads/feature-x\n").expect("head");
    (root, folder)
}

fn opened(sent: &[Cmd]) -> Vec<Open> {
    sent.iter()
        .filter_map(|cmd| match cmd {
            Cmd::Open(open) => Some(open.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_header_names_the_chats_folder_and_branch() {
    if !in_window("place::the_header_names_the_chats_folder_and_branch") {
        return;
    }
    let (_root, folder) = shop();
    let seen = steps(
        settings(Snapshot {
            workspace: folder,
            ..graded()
        }),
        Vec::new(),
        |_| (),
    );
    assert_eq!(
        seen.named(Role::Button, "Repository").len(),
        1,
        "{:?}",
        seen.controls
    );
    assert!(
        seen.shows(|text| text == "shop · feature-x"),
        "{}",
        seen.text()
    );
}

#[test]
fn the_folder_menu_starts_a_chat_on_a_new_branch_of_this_folder() {
    if !in_window("place::the_folder_menu_starts_a_chat_on_a_new_branch_of_this_folder") {
        return;
    }
    let (_root, folder) = shop();
    let seen = steps(
        settings(Snapshot {
            workspace: folder.clone(),
            ..graded()
        }),
        vec![
            Step::Click("Repository"),
            Step::Click(super::super::thread::place_branch()),
        ],
        |_| (),
    );
    assert_eq!(opened(&seen.sent), vec![Open::Worktree(folder)]);
}

fn chat(id: &str, cwd: &str, preview: &str) -> (SessionInfo, bool) {
    (
        SessionInfo {
            id: id.to_owned(),
            path: PathBuf::new(),
            cwd: Some(cwd.to_owned()),
            model: None,
            preview: preview.to_owned(),
            name: None,
            parent: None,
            depth: 0,
            interactive: true,
            agent: None,
            topic: None,
            modified: None,
        },
        false,
    )
}

#[test]
fn agents_group_by_repository_in_name_order_and_new_agent_is_the_only_start() {
    if !in_window("place::agents_group_by_repository_in_name_order_and_new_agent_is_the_only_start")
    {
        return;
    }
    let place = std::rc::Rc::new(std::cell::RefCell::new(PathBuf::new()));
    let here = place.clone();
    let seen = steps(
        move |app| {
            *here.borrow_mut() = app.workspace();
            app.window.sidebar.set(true);
            app.update(Msg::Fact(Fact::Sessions(Ok(vec![
                chat(
                    "01a1000000000000000000000000bbbb",
                    "/work/beta",
                    "Fix the beta build",
                ),
                chat(
                    "01a1000000000000000000000000aaaa",
                    "/work/alpha",
                    "Tidy the alpha docs",
                ),
            ]))));
        },
        vec![Step::Click("New agent")],
        |_| (),
    );
    let text = seen.text();
    let at = |wanted: &str| {
        text.find(wanted)
            .unwrap_or_else(|| panic!("{wanted}: {text}"))
    };
    assert!(
        at("alpha") < at("Tidy the alpha docs") && at("Tidy the alpha docs") < at("beta"),
        "{text}"
    );
    assert!(at("beta") < at("Fix the beta build"), "{text}");
    assert!(
        !seen
            .controls
            .iter()
            .any(|(_, name, _)| name.starts_with("New chat in")),
        "{:?}",
        seen.controls
    );
    assert_eq!(
        opened(&seen.sent),
        vec![Open::Worktree(place.borrow().clone())]
    );
}

#[test]
fn the_folder_menu_lists_known_projects_and_switches_to_one() {
    if !in_window("place::the_folder_menu_lists_known_projects_and_switches_to_one") {
        return;
    }
    let (_root, folder) = shop();
    let seen = steps(
        |app| {
            app.window.sidebar.set(false);
            app.update(Msg::Fact(Fact::Settings(Snapshot {
                workspace: folder.clone(),
                ..graded()
            })));
            app.update(Msg::Fact(Fact::Sessions(Ok(vec![chat(
                "01a1000000000000000000000000aaaa",
                "/work/alpha",
                "Tidy the alpha docs",
            )]))));
        },
        vec![Step::Click("Repository"), Step::Click("alpha")],
        |_| (),
    );
    assert_eq!(
        opened(&seen.sent),
        vec![Open::Folder(PathBuf::from("/work/alpha"))]
    );
}

#[test]
fn the_sidebar_shows_projects_without_being_opened() {
    if !in_window("place::the_sidebar_shows_projects_without_being_opened") {
        return;
    }
    let seen = steps(
        |app| {
            app.update(Msg::Fact(Fact::Sessions(Ok(vec![chat(
                "01a1000000000000000000000000aaaa",
                "/work/alpha",
                "Tidy the alpha docs",
            )]))));
        },
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "Tidy the alpha docs"),
        "{}",
        seen.text()
    );
}

#[test]
fn the_folder_menu_offers_a_branch_only_in_a_repository() {
    if !in_window("place::the_folder_menu_offers_a_branch_only_in_a_repository") {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let seen = steps(
        settings(Snapshot {
            workspace: root.path().to_path_buf(),
            ..graded()
        }),
        vec![Step::Click("Repository")],
        |_| (),
    );
    let branch = super::super::thread::place_branch();
    assert!(!seen.shows(|text| text == branch), "{}", seen.text());
}
