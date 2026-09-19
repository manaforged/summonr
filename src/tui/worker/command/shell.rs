pub(super) fn shell_prefix(line: &str) -> Option<(bool, &str)> {
    let rest = line.strip_prefix('!')?;
    let (send, command) = match rest.strip_prefix('!') {
        Some(quiet) => (false, quiet),
        None => (true, rest),
    };
    let command = command.trim();
    (!command.is_empty()).then_some((send, command))
}

pub(super) fn shell_prompt(command: &str, output: &str) -> String {
    format!("$ {command}\n{output}")
}
