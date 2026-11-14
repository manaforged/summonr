const SIGIL: char = '$';
const ESCAPE: char = '\\';
const ALL: &str = "ARGUMENTS";
const OPEN: char = '[';
const CLOSE: char = ']';
const LABEL: &str = "\n\nARGUMENTS: ";

enum Slot {
    All,
    At(usize),
    Named(usize),
}

/// `parts` filled as one template with `$ARGUMENTS`, `$ARGUMENTS[N]`, `$N`, and declared `$name`; unused arguments go after the last part.
pub(super) fn fill_parts(parts: &[&str], arguments: &str, names: &[String]) -> Vec<String> {
    let words = crate::policy::shell_words(arguments);
    let mut received = false;
    let mut out: Vec<String> = parts
        .iter()
        .map(|part| fill_one(part, (arguments, words.as_slice()), names, &mut received))
        .collect();
    if !received
        && !arguments.is_empty()
        && let Some(last) = out.last_mut()
    {
        last.push_str(LABEL);
        last.push_str(arguments);
    }
    out
}

fn fill_one(
    template: &str,
    (arguments, words): (&str, &[String]),
    names: &[String],
    received: &mut bool,
) -> String {
    let mut out = String::with_capacity(template.len() + arguments.len());
    let mut rest = template;
    while let Some(at) = rest.find(SIGIL) {
        let before = &rest[..at];
        let tail = &rest[at + SIGIL.len_utf8()..];
        let Some((slot, length)) = slot(tail, names) else {
            out.push_str(before);
            out.push(SIGIL);
            rest = tail;
            continue;
        };
        let token = &tail[..length];
        rest = &tail[length..];
        if escaped(before) {
            out.push_str(&before[..before.len() - ESCAPE.len_utf8()]);
            out.push(SIGIL);
            out.push_str(token);
            continue;
        }
        out.push_str(before);
        let value = match slot {
            Slot::All => Some(arguments),
            Slot::At(index) => words.get(index).map(String::as_str),
            Slot::Named(index) => Some(words.get(index).map_or("", String::as_str)),
        };
        if let Some(value) = value {
            out.push_str(value);
            *received = true;
        } else {
            out.push(SIGIL);
            out.push_str(token);
        }
    }
    out.push_str(rest);
    out
}

fn escaped(before: &str) -> bool {
    before
        .strip_suffix(ESCAPE)
        .is_some_and(|rest| !rest.ends_with(ESCAPE))
}

fn slot(tail: &str, names: &[String]) -> Option<(Slot, usize)> {
    if let Some(after) = tail.strip_prefix(ALL) {
        let index = after
            .strip_prefix(OPEN)
            .and_then(|inner| inner.split_once(CLOSE))
            .and_then(|(digits, _)| number(digits).map(|index| (index, digits.len())));
        return Some(match index {
            Some((index, digits)) => (Slot::At(index), ALL.len() + digits + 2),
            None => (Slot::All, ALL.len()),
        });
    }
    let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        return number(&tail[..digits]).map(|index| (Slot::At(index), digits));
    }
    names
        .iter()
        .enumerate()
        .filter(|(_, name)| {
            tail.strip_prefix(name.as_str()).is_some_and(|after| {
                !after.starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
            })
        })
        .max_by_key(|(_, name)| name.len())
        .map(|(index, name)| (Slot::Named(index), name.len()))
}

fn number(digits: &str) -> Option<usize> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}
