pub(in crate::tui::sigil_view) fn item(line: &str) -> (char, &str) {
    let body = line.trim_start();
    for (prefix, mark) in [
        ("[x] ", '✓'),
        ("✓ ", '✓'),
        ("[/] ", '●'),
        ("● ", '●'),
        ("[ ] ", '○'),
        ("○ ", '○'),
        ("[!] ", '!'),
    ] {
        if let Some(rest) = body.strip_prefix(prefix) {
            return (mark, rest);
        }
    }
    (' ', body)
}
