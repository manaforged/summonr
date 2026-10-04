use super::*;

pub(super) fn expands_to_shell(var: &str) -> bool {
    let name: String = var
        .trim_start_matches('{')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return true;
    }
    match std::env::var(&name) {
        Ok(value) => shell(&value),
        Err(_) => true,
    }
}

pub(super) fn shell(program: &str) -> bool {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['\'', '"'])
        .to_ascii_lowercase();
    matches!(
        name.strip_suffix(".exe").unwrap_or(&name),
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "fish" | "cmd" | "powershell" | "pwsh"
    )
}

pub(super) const WRAPPERS: [&str; 15] = [
    "env", "sudo", "doas", "nice", "nohup", "exec", "command", "builtin", "time", "xargs",
    "timeout", "stdbuf", "setsid", "flock", "chronic",
];

pub(super) fn pipes_to_shell(command: &str) -> bool {
    unquoted(command, &["|"])
        .into_iter()
        .skip(1)
        .any(|segment| {
            let words: Vec<&str> = segment
                .trim_start_matches('&')
                .split_whitespace()
                .map(|word| word.rsplit('/').next().unwrap_or(word))
                .collect();
            words
                .iter()
                .position(|word| {
                    !WRAPPERS.contains(word)
                        && !word.starts_with('-')
                        && !word.contains('=')
                        && !word.chars().all(|c| c.is_ascii_digit())
                })
                .is_some_and(|at| {
                    let word = words[at];
                    let bare: String = word
                        .chars()
                        .filter(|c| !matches!(c, '"' | '\'' | '\\'))
                        .collect();
                    if let Some(var) = bare.strip_prefix('$') {
                        return expands_to_shell(var);
                    }
                    let name = bare
                        .trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
                        .to_ascii_lowercase();
                    word.starts_with('`')
                        || ((shell(&name) || interpreter(&name)) && reads_stdin(&words[at + 1..]))
                })
        })
}

pub(super) fn interpreter(name: &str) -> bool {
    matches!(
        name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.'),
        "python" | "node" | "perl" | "ruby" | "php" | "deno" | "bun"
    )
}

pub(super) fn reads_stdin(args: &[&str]) -> bool {
    args.contains(&"-")
        || args.iter().all(|arg| match arg.strip_prefix("--") {
            Some(long) => !["eval", "print", "command"]
                .iter()
                .any(|flag| long.starts_with(flag)),
            None => arg.starts_with('-') && !arg.contains(['c', 'e', 'E', 'm']),
        })
}

pub(super) fn runs_substitution(segments: &[String]) -> bool {
    segments.iter().any(|segment| {
        let mut words = segment.split_whitespace();
        words.next().is_some_and(|verb| {
            let name = verb.rsplit('/').next().unwrap_or(verb).to_ascii_lowercase();
            shell(&name) || interpreter(&name) || matches!(name.as_str(), "source" | ".")
        }) && words.any(|word| word.starts_with("<("))
    })
}

pub(super) fn builtin_deny(command: &str, segments: &[String]) -> bool {
    let folded = collapse(command);
    wipes_root(&folded)
        || folded.contains("git push --force")
        || folded.contains("git push -f")
        || folded.contains("git push --mirror")
        || pipes_to_shell(command)
        || runs_substitution(segments)
        || folded.contains("mkfs")
        || folded.contains(":(){ :|:& };:")
        || escalates(command)
        || powers_off(segments)
        || folded.contains("chmod -r 777 /")
        || (folded.contains("dd if=") && folded.contains("of=/dev/"))
}

fn escalates(command: &str) -> bool {
    !running_as_root()
        && unquoted(command, &["&&", "||", "|", ";", "\n", "&"])
            .iter()
            .any(|segment| {
                segment
                    .split_whitespace()
                    .map(|word| word.rsplit('/').next().unwrap_or(word))
                    .find(|word| {
                        !word.contains('=') && !word.starts_with('-') && !WRAPPERS.contains(word)
                            || matches!(*word, "sudo" | "doas")
                    })
                    .is_some_and(|word| matches!(word, "sudo" | "doas"))
            })
}

#[cfg(unix)]
#[expect(unsafe_code, reason = "std has no effective-uid query")]
pub(crate) fn running_as_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
pub(crate) fn running_as_root() -> bool {
    false
}

pub(super) fn powers_off(segments: &[String]) -> bool {
    segments.iter().any(|segment| {
        segment
            .split_whitespace()
            .find(|word| !word.contains('='))
            .map(|word| word.rsplit('/').next().unwrap_or(word).to_ascii_lowercase())
            .is_some_and(|verb| {
                matches!(verb.as_str(), "shutdown" | "reboot" | "halt" | "poweroff")
            })
    })
}
