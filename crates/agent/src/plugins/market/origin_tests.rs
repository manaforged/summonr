use super::*;

#[test]
fn a_dot_segment_cannot_borrow_the_official_owner() {
    for input in [
        "https://github.com/anthropics/../owner/repo",
        "git@github.com:anthropics/../owner/repo.git",
        "https://github.com/anthropics/./repo.git",
    ] {
        let parsed = Origin::parse(input);
        assert!(parsed.is_err(), "{input}: {parsed:?}");
    }
    let stored = Origin::Git {
        url: "https://github.com/anthropics/../owner/repo.git".to_string(),
        reference: None,
    };
    assert!(!stored.official(), "a stored source is checked again");
    let nested = Origin::Github {
        repo: "anthropics/x/../../owner/repo".to_string(),
        reference: None,
    };
    assert!(!nested.official());
}

#[test]
fn anthropic_sources_stay_official() {
    for input in [
        "anthropics/claude-plugins-official",
        "https://github.com/anthropics/claude-plugins-official",
        "https://github.com/anthropics/claude-plugins-official.git",
        "git@github.com:anthropics/claude-plugins-official.git",
    ] {
        let origin = Origin::parse(input).expect("an Anthropic source parses");
        assert!(origin.official(), "{input}: {origin:?}");
    }
    let other = Origin::parse("https://github.com/owner/repo").expect("a GitHub URL parses");
    assert!(!other.official());
}
