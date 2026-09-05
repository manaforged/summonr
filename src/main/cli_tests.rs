use super::{Args, parse_from};

fn args(tokens: &[&str]) -> Args {
    parse_from(tokens.iter().map(|token| (*token).to_string())).expect("parse")
}

#[test]
fn workspace_instructions_load_by_default() {
    assert!(args(&[]).trust);
    assert!(!args(&["--no-trust"]).trust);
}

#[test]
fn command_containment_is_the_default() {
    let defaults = args(&[]);
    assert!(!defaults.no_sandbox);
    assert!(!defaults.network);
    assert!(args(&["--no-sandbox"]).no_sandbox);
    assert!(args(&["--net"]).network);
    assert!(!args(&["--sandbox", "--no-net"]).network);
}

#[test]
fn first_token_named_endpoint_is_the_target() {
    let parsed = args(&["grok", "-p", "hello"]);
    assert_eq!(parsed.endpoint.as_deref(), Some("grok"));
    assert!(parsed.print);
    assert_eq!(parsed.prompt, ["hello"]);
}

#[test]
fn zai_regions_are_direct_commands() {
    for name in ["zai", "zai-cn"] {
        let parsed = args(&[name]);
        assert_eq!(parsed.endpoint.as_deref(), Some(name));
        assert!(parsed.prompt.is_empty());
    }
}

#[test]
fn anthropic_token_is_a_named_seat() {
    let parsed = args(&["anthropic", "-p", "hi"]);
    assert_eq!(parsed.endpoint.as_deref(), Some("anthropic"));
    assert!(parsed.print);
}

#[test]
fn kimi_token_matches_minus_e() {
    let parsed = args(&["kimi"]);
    assert_eq!(parsed.endpoint.as_deref(), Some("kimi"));
    assert!(parsed.prompt.is_empty());
}

#[test]
fn minus_e_wins_over_later_name() {
    let parsed = args(&["-e", "kimi", "grok"]);
    assert_eq!(parsed.endpoint.as_deref(), Some("kimi"));
    assert_eq!(parsed.prompt, ["grok"]);
}

#[test]
fn ordinary_prompt_stays_a_prompt() {
    let parsed = args(&["summarize-this-repo"]);
    assert!(parsed.endpoint.is_none());
    assert_eq!(parsed.prompt, ["summarize-this-repo"]);
}

#[test]
fn login_subcommand_is_not_an_endpoint() {
    let parsed = args(&["login", "grok"]);
    assert!(parsed.login);
    assert_eq!(parsed.endpoint.as_deref(), Some("grok"));
}

#[test]
fn login_copilot_is_the_device_preset() {
    let parsed = args(&["login", "copilot"]);
    assert!(parsed.login);
    assert_eq!(parsed.endpoint.as_deref(), Some("copilot"));
}

#[test]
fn sessions_subcommand_is_not_an_endpoint() {
    let parsed = args(&["sessions"]);
    assert!(parsed.list_sessions);
    assert!(parsed.endpoint.is_none());
}

#[test]
fn diagnostics_subcommands_are_not_prompts() {
    let logs = args(&["logs"]);
    assert!(logs.logs);
    assert!(logs.endpoint.is_none());
    let doctor = args(&["doctor"]);
    assert!(doctor.doctor);
    assert!(doctor.endpoint.is_none());
    parse_from(["logs", "extra"].map(str::to_string)).expect_err("logs reject extra arguments");
}

#[test]
fn worktrees_prune_is_not_an_endpoint() {
    let parsed = args(&["worktrees", "prune"]);
    assert!(parsed.list_worktrees);
    assert!(parsed.prune_worktrees);
}

#[test]
fn here_and_worktree_conflict() {
    let error = super::parse_from(["--here", "--worktree"].into_iter().map(str::to_string))
        .expect_err("conflict");
    assert!(error.to_string().contains("--here"));
}

#[test]
fn workflow_flag_and_subcommand() {
    let parsed = args(&["-w", "cheap", "-p", "hi"]);
    assert_eq!(parsed.workflow.as_deref(), Some("cheap"));
    assert_eq!(args(&["--workflow", "x"]).workflow.as_deref(), Some("x"));
    let listed = args(&["workflows"]);
    assert!(listed.list_workflows);
    assert!(listed.endpoint.is_none());
}

#[test]
fn effort_flag_takes_a_level_and_rejects_the_rest() {
    assert_eq!(
        args(&["--effort", "low"]).effort,
        Some(summoner::Effort::Low)
    );
    assert!(args(&[]).effort.is_none());
    let error = parse_from(["--effort", "turbo"].map(str::to_string))
        .expect_err("bad level")
        .to_string();
    assert!(error.contains("turbo"), "{error}");
}

#[test]
fn a_bare_unknown_name_is_an_error_not_a_prompt() {
    let error = parse_from(["nosuch", "-p"].map(str::to_string))
        .expect_err("unknown endpoint")
        .to_string();
    assert!(error.contains("unknown endpoint `nosuch`"), "{error}");
    let parsed = args(&["-p", "nosuch"]);
    assert_eq!(parsed.prompt, ["nosuch"]);
    let parsed = args(&["-p", "fix", "the", "tests"]);
    assert_eq!(parsed.prompt, ["fix", "the", "tests"]);
}

#[test]
fn a_bare_unknown_name_before_goal_or_steps_is_an_error() {
    let error = parse_from(["nosuch", "--goal", "fix it"].map(str::to_string))
        .expect_err("unknown endpoint")
        .to_string();
    assert!(error.contains("unknown endpoint `nosuch`"), "{error}");
    let parsed = args(&["--goal", "fix it"]);
    assert!(parsed.prompt.is_empty());
}

#[test]
fn steps_requires_a_real_plan_path() {
    let missing = parse_from(["--steps"].map(str::to_string))
        .expect_err("missing path")
        .to_string();
    assert!(
        missing.contains("--steps requires a plan path"),
        "{missing}"
    );

    let option = parse_from(["--steps", "--plan"].map(str::to_string))
        .expect_err("option is not a path")
        .to_string();
    assert!(option.contains("not option `--plan`"), "{option}");

    assert_eq!(
        args(&["--steps", "PLAN.md"]).steps.as_deref(),
        Some("PLAN.md")
    );
}

#[test]
fn help_lists_every_interactive_command() {
    let text = super::interactive();
    for spec in summoner::command_specs() {
        assert!(
            text.contains(spec.name),
            "{} missing from {text}",
            spec.name
        );
    }
    assert!(text.lines().all(|line| line.len() <= 72), "{text}");
}

#[test]
fn export_and_name_parse() {
    let parsed = args(&["export", "18d0"]);
    assert!(parsed.export);
    assert_eq!(parsed.prompt, ["18d0"]);
    let parsed = args(&["--name", "widget spike", "-p", "hi"]);
    assert_eq!(parsed.name.as_deref(), Some("widget spike"));
    assert_eq!(parsed.prompt, ["hi"]);
    let parsed = args(&["-p", "export"]);
    assert!(!parsed.export, "after -p, export is a prompt");
}

#[test]
fn max_turns_zero_means_unlimited() {
    assert_eq!(args(&["--max-turns", "0"]).max_turns, Some(0));
    assert_eq!(args(&["--max-turns", "3"]).max_turns, Some(3));
}

#[test]
fn login_key_and_logout_parse() {
    let parsed = args(&["login", "glm", "--key"]);
    assert!(parsed.login && parsed.key);
    assert_eq!(parsed.endpoint.as_deref(), Some("glm"));
    let parsed = args(&["logout", "glm"]);
    assert!(parsed.logout);
    assert_eq!(parsed.endpoint.as_deref(), Some("glm"));
}

#[test]
fn diff_takes_a_path_and_stat() {
    let parsed = args(&["diff", "src/lib.rs", "--stat"]);
    assert!(parsed.diff);
    assert!(parsed.stat);
    assert_eq!(parsed.prompt, vec!["src/lib.rs".to_string()]);
    assert!(
        !args(&["-p", "diff"]).diff,
        "a prompt word is not the subcommand"
    );
    assert_eq!(
        args(&["diff", "--turn", "last"]).turn.as_deref(),
        Some("last")
    );
}

#[test]
fn login_key_never_takes_the_key_as_an_argument() {
    let parsed = args(&["login", "glm", "--key", "sk-on-argv"]);
    assert!(parsed.key);
    assert_eq!(
        parsed.prompt,
        vec!["sk-on-argv".to_string()],
        "startup refuses this"
    );
}
