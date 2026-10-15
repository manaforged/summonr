pub(crate) const GIT_DIR: &str = ".git";
pub(crate) const GIT_MODULES: &str = "modules";
pub(crate) const GIT_GUARDS: &[&str] = &[
    "hooks",
    "config",
    "commondir",
    "HEAD",
    "index",
    "refs",
    "packed-refs",
    "logs",
];

pub(super) fn guarded_git_part(parts: &[&str]) -> bool {
    parts.iter().enumerate().any(|(at, part)| {
        *part == GIT_DIR
            && match parts.get(at + 1..).unwrap_or_default() {
                [first, ..] if GIT_GUARDS.contains(first) => true,
                [first, rest @ ..] if *first == GIT_MODULES => {
                    rest.iter().skip(1).any(|part| GIT_GUARDS.contains(part))
                }
                _ => false,
            }
    })
}
