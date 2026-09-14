use super::*;

fn sample() -> Splash<'static> {
    Splash {
        version: "0.1.0",
        model: "glm-5.3",
        relay: Some("open.bigmodel.cn"),
        session: "1786992",
        resumed: false,
        history: 0,
        workspace: Path::new("/tmp/quest"),
        plan: false,
        strict: true,
        trust: false,
    }
}

#[test]
fn host_of_strips_scheme_and_path() {
    assert_eq!(
        host_of(Some("https://open.bigmodel.cn/api/coding/paas/v4")),
        Some("open.bigmodel.cn")
    );
    assert_eq!(
        host_of(Some("http://127.0.0.1:8081/v1")),
        Some("127.0.0.1:8081")
    );
    assert_eq!(host_of(Some("open.bigmodel.cn")), Some("open.bigmodel.cn"));
    assert_eq!(host_of(Some("")), None);
    assert_eq!(host_of(None), None);
}

#[test]
fn abbreviate_shortens_home_prefix() {
    assert_eq!(
        abbreviate(Path::new("/home/tom/dev"), Some(Path::new("/home/tom"))),
        "~/dev"
    );
    assert_eq!(
        abbreviate(Path::new("/home/tom"), Some(Path::new("/home/tom"))),
        "~"
    );
    assert_eq!(
        abbreviate(Path::new("/etc"), Some(Path::new("/home/tom"))),
        "/etc"
    );
    assert_eq!(abbreviate(Path::new("/etc"), None), "/etc");
}

#[test]
fn logo_rows_are_the_same_width() {
    let width = logo_width();
    for row in logo() {
        assert_eq!(row.chars().count(), width, "{row}");
    }
}

#[test]
fn plain_render_has_no_ansi_and_lists_facts() {
    let splash = sample();
    let text = render(&splash, &PLAIN);
    assert!(!text.contains('\x1b'), "no escape bytes in plain mode");
    assert!(text.contains("glm-5.3"));
    assert!(text.contains("open.bigmodel.cn"));
    assert!(text.contains("1786992"));
    assert!(text.contains("strict"));
    assert!(text.contains("✦ /help"));
}

#[test]
fn render_stacks_info_under_logo() {
    let text = render(&sample(), &PLAIN);
    let first = text.lines().next().unwrap_or_default();
    assert!(first.contains("███╗"), "logo starts the first line");
    assert!(
        !first.contains("summonr 0.1.0"),
        "facts sit under the wordmark, not beside it"
    );
    assert!(text.contains("  summonr 0.1.0"), "info is indented below");
}

#[test]
fn plan_row_appears_only_in_plan_mode() {
    let splash = sample();
    assert!(!render(&splash, &PLAIN).contains("read-only"));
    let mut splash = splash;
    splash.plan = true;
    assert!(render(&splash, &PLAIN).contains("plan · read-only"));
}

#[test]
fn resumed_shows_history_and_skips_it_when_new() {
    let mut splash = sample();
    splash.resumed = true;
    splash.history = 12;
    assert!(render(&splash, &PLAIN).contains("12 messages"));
    splash.resumed = false;
    let fresh = render(&splash, &PLAIN);
    assert!(!fresh.contains("messages"));
    assert!(fresh.contains("new"));
}
