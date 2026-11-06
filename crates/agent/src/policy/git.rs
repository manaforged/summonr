use super::rules;

pub(crate) const GIT_DIR: &str = ".git";

pub(super) fn guarded_git_part(parts: &[&str]) -> bool {
    let git = &rules().git;
    let guarded = |part: &str| git.guards.iter().any(|guard| guard == part);
    parts.iter().enumerate().any(|(at, part)| {
        *part == GIT_DIR
            && match parts.get(at + 1..).unwrap_or_default() {
                [first, ..] if guarded(first) => true,
                [first, rest @ ..] if *first == git.modules => {
                    rest.iter().skip(1).any(|part| guarded(part))
                }
                _ => false,
            }
    })
}
