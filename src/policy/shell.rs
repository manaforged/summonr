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
    rules()
        .shell
        .shells
        .contains(name.strip_suffix(".exe").unwrap_or(&name))
}

pub(super) fn wrapper(name: &str) -> bool {
    rules().wrappers.contains_key(name)
}

pub(super) fn command_pieces(segment: &str) -> Vec<String> {
    let Some(body) = arm_body(segment.trim()) else {
        return vec![segment.to_string()];
    };
    let body = body.trim();
    if body.is_empty() || body == "esac" {
        Vec::new()
    } else {
        vec![body.to_string()]
    }
}

fn arm_body(segment: &str) -> Option<&str> {
    let (before, after) = segment.split_once(')')?;
    let pattern = before.trim();
    if pattern.is_empty() {
        return None;
    }
    let mut words = pattern.split_whitespace();
    let head = words.next().unwrap_or("");
    let name = head
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(head)
        .to_ascii_lowercase();
    if name == "case" {
        return Some(after);
    }
    if words.next().is_some() || pattern.contains(['$', '`', '(']) {
        return None;
    }
    Some(after)
}

pub(super) fn skip_wrapper_args<'a>(wrapper: &str, mut text: &'a str) -> &'a str {
    loop {
        let (word, rest) = text.split_once([' ', '\t']).unwrap_or((text, ""));
        if word.is_empty()
            || !(word.starts_with('-')
                || word.contains('=')
                || word.chars().all(|c| c.is_ascii_digit()))
        {
            return text;
        }
        text = rest.trim_start();
        let (flag, attached) = word
            .split_once('=')
            .map_or((word, false), |(flag, _)| (flag, true));
        if !attached && wrapper_takes_value(wrapper, flag) {
            let (_value, rest) = text.split_once([' ', '\t']).unwrap_or((text, ""));
            text = rest.trim_start();
        }
    }
}

fn wrapper_takes_value(wrapper: &str, flag: &str) -> bool {
    rules()
        .wrappers
        .get(wrapper)
        .is_some_and(|flags| flags.contains(flag))
}

fn xargs_runs_input(words: &[&str], at: usize) -> bool {
    let xargs = &rules().xargs;
    let Some(from) = words[..at].iter().position(|word| *word == xargs.name) else {
        return false;
    };
    let options = &words[from + 1..at];
    let token = options.iter().enumerate().find_map(|(index, word)| {
        let flag = xargs
            .replace
            .iter()
            .find(|flag| word.starts_with(flag.as_str()))?;
        let rest = &word[flag.len()..];
        Some(if !rest.is_empty() {
            rest
        } else if xargs.token_flags.contains(flag) {
            options
                .get(index + 1)
                .copied()
                .unwrap_or(xargs.default_token.as_str())
        } else {
            xargs.default_token.as_str()
        })
    });
    let rest = &words[at + 1..];
    let Some(flag) = rest
        .iter()
        .position(|word| word.starts_with('-') && !word.starts_with("--") && word.contains('c'))
    else {
        return false;
    };
    match rest[flag + 1..].iter().find(|word| **word != "--") {
        None => true,
        Some(script) => token.is_some_and(|token| script.replace(['\'', '"'], "") == token),
    }
}

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
            let xargs = &rules().xargs.name;
            let value_flags = rules().wrappers.get(xargs);
            let mut value = false;
            words
                .iter()
                .enumerate()
                .position(|(index, word)| {
                    let skipped = value;
                    // complexity: bounded by one BTreeSet lookup per word
                    value = value_flags.is_some_and(|flags| flags.contains(*word))
                        && words[..index].contains(&xargs.as_str());
                    !skipped
                        && !wrapper(word)
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
                        || ((shell(&name) || interpreter(&name))
                            && (reads_stdin(&words[at + 1..]) || xargs_runs_input(&words, at)))
                })
        })
}

pub(super) fn interpreter(name: &str) -> bool {
    rules()
        .shell
        .interpreters
        .contains(name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.'))
}

pub(super) fn reads_stdin(args: &[&str]) -> bool {
    args.contains(&"-")
        || args.iter().all(|arg| match arg.strip_prefix("--") {
            Some(long) => !rules()
                .shell
                .code_flags
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
            // complexity: bounded by the fixed [shell] table in rules.toml
            shell(&name) || interpreter(&name) || rules().shell.sourcing.contains(&name)
        }) && words.any(|word| word.starts_with("<("))
    })
}

pub(super) fn builtin_deny(command: &str, segments: &[String]) -> bool {
    let folded = collapse(command);
    catastrophic(command)
        || rules()
            .shell
            .denied_text
            .iter()
            // complexity: bounded by the fixed [shell] table in rules.toml
            .any(|text| folded.contains(text.as_str()))
        || pipes_to_shell(command)
        || runs_substitution(segments)
        || escalates(command)
        || powers_off(segments)
}

pub(super) fn catastrophic(command: &str) -> bool {
    let folded = collapse(command);
    let shell = &rules().shell;
    wipes_root(&folded)
        || shell
            .catastrophic
            .iter()
            // complexity: bounded by the fixed [shell] table in rules.toml
            .any(|text| folded.contains(text.as_str()))
        || shell
            .device_write
            .iter()
            // complexity: bounded by the fixed [shell] table in rules.toml
            .all(|text| folded.contains(text.as_str()))
}

fn escalates(command: &str) -> bool {
    !running_as_root()
        && unquoted(command, &["&&", "||", "|", ";", "\n", "&"])
            .iter()
            .any(|segment| command_word(segment, true).is_some_and(escalator))
}

pub(super) fn command_word(segment: &str, stop_at_escalator: bool) -> Option<&str> {
    segment
        .split_whitespace()
        .map(|word| word.rsplit('/').next().unwrap_or(word))
        .find(|word| {
            !word.contains('=') && !word.starts_with('-') && !wrapper(word)
                || stop_at_escalator && escalator(word)
        })
}

fn escalator(word: &str) -> bool {
    rules().shell.escalators.contains(word)
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
            // complexity: bounded by the fixed [shell] table in rules.toml
            .is_some_and(|verb| rules().shell.power_off.contains(&verb))
    })
}
