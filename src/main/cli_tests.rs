use super::*;

#[test]
fn old_verbs_are_prompts_not_commands() {
    let args = parse_from(["auth"].map(String::from)).unwrap();
    assert!(!matches!(args.verb, Some(Verb::Login)));
    assert_eq!(args.prompt, ["auth"]);
    let args = parse_from(["logs"].map(String::from)).unwrap();
    assert!(!args.logs);
}

#[test]
fn goal_and_steps_together_are_refused() {
    let error = parse_from(["--goal", "x", "--steps", "y"].map(String::from))
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("--goal") && error.contains("--steps"),
        "{error}"
    );
}

#[test]
fn noun_verbs_and_old_words_reach_the_same_command() {
    let parse = |words: &[&str]| parse_from(words.iter().map(|word| word.to_string())).unwrap();
    let pairs: [(&[&str], &[&str]); 6] = [
        (&["session", "list"], &["sessions"]),
        (&["session", "diff", "src"], &["diff", "src"]),
        (&["endpoint", "list"], &["endpoints"]),
        (&["worktree", "prune"], &["worktrees", "prune"]),
        (&["plugin", "remove", "x"], &["uninstall", "x"]),
        (
            &["endpoint", "remove", "anthropic"],
            &["logout", "anthropic", "--remove"],
        ),
    ];
    for (new, old) in pairs {
        let (new, old) = (parse(new), parse(old));
        assert_eq!(
            format!("{:?}", new.verb),
            format!("{:?}", old.verb),
            "{new:?}"
        );
        assert_eq!(new.prune_worktrees, old.prune_worktrees);
        assert_eq!(new.remove, old.remove);
        assert_eq!(new.prompt, old.prompt);
    }
    let search = parse(&["session", "search", "needle"]);
    assert!(matches!(search.verb, Some(Verb::SessionList)));
    assert_eq!(search.grep.as_deref(), Some("needle"));
    assert!(parse(&["plugin", "list"]).add_list);
    let tool = parse(&["tool", "run", "read"]);
    assert_eq!(
        format!("{:?}", tool.verb),
        format!("{:?}", parse(&["tool", "read"]).verb)
    );
    let error = parse_from(["session".to_string()])
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("takes one of: list, search"), "{error}");
    let add = parse(&["endpoint", "add", "srv", "--url", "https://x.example"]);
    assert!(add.add);
    assert_eq!(add.endpoint.as_deref(), Some("srv"));
    let error = parse_from(["endpoint".to_string(), "add".to_string()])
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("endpoint add NAME --url"), "{error}");
}

#[test]
fn help_renders_exactly_the_old_verb_sections() {
    assert_eq!(super::help::verbs(), include_str!("cli/help_golden.txt"));
}

#[test]
fn every_help_line_starts_with_its_command_name() {
    for row in super::commands::COMMANDS
        .iter()
        .chain(super::commands::subcommands())
    {
        if row.help.is_empty() {
            continue;
        }
        assert!(
            row.help.starts_with(row.name),
            "the help line of `{}` does not start with its name",
            row.name
        );
    }
}

#[test]
fn help_sections_claim_every_visible_row_exactly_once() {
    let rows: Vec<&str> = super::commands::COMMANDS
        .iter()
        .chain(super::commands::subcommands())
        .map(|row| row.name)
        .collect();
    let mut claimed = Vec::new();
    for (title, names) in super::commands::SECTIONS {
        for name in names {
            assert!(
                rows.contains(name),
                "the help section `{title}` names the unknown command `{name}`"
            );
            assert!(
                !claimed.contains(name),
                "`{name}` is claimed twice by the help sections"
            );
            claimed.push(*name);
        }
    }
    for name in rows {
        let visible = super::commands::COMMANDS
            .iter()
            .chain(super::commands::subcommands())
            .find(|row| row.name == name)
            .is_some_and(|row| !row.help.is_empty());
        assert_eq!(
            claimed.contains(&name),
            visible,
            "`{name}` is claimed by a help section only when it has help text"
        );
    }
}
