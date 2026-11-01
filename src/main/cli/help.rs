fn interactive() -> String {
    let mut lines = vec![String::new()];
    for spec in summonr::command_specs().iter().filter(|spec| spec.shown()) {
        let last = lines.last_mut().expect("one line");
        if last.len() + spec.name.len() + 3 > 70 {
            lines.push(String::new());
        }
        let last = lines.last_mut().expect("one line");
        if !last.is_empty() {
            last.push_str("   ");
        }
        last.push_str(spec.name);
    }
    lines
        .iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The verb sections, rendered from the command rows.
pub(super) fn verbs() -> String {
    let rows: std::collections::BTreeMap<&str, &super::commands::Command> =
        super::commands::COMMANDS
            .iter()
            .chain(super::commands::SUBCOMMANDS.iter())
            .map(|row| (row.name, row))
            .collect();
    let mut out = String::new();
    for (title, names) in super::commands::SECTIONS {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push('\n');
        out.push_str(title);
        out.push(':');
        for name in names {
            let Some(row) = rows.get(name) else {
                panic!("the help section `{title}` names the unknown command `{name}`");
            };
            for line in row.help.split('\n') {
                out.push_str("\n  ");
                out.push_str(line);
            }
        }
    }
    out
}

/// The full `summonr --help` text.
pub(super) fn text() -> String {
    include_str!("../help.txt")
        .replace("{options}", &super::FLAGS.help_lines())
        .replace("{commands}", &interactive())
        .replace("{verbs}", &verbs())
        .replace("{sets}", &summonr::classify::sets::NAMES.join(", "))
}

pub(super) fn exit_help(verb: Option<&str>) -> ! {
    let text = text();
    let lines: Vec<&str> = verb.map_or_else(Vec::new, |verb| {
        text.lines()
            .filter(|line| {
                let line = line.trim_start();
                line.strip_prefix(verb)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '|']))
                    || line.starts_with(&format!("summonr {verb} "))
            })
            .collect()
    });
    if lines.is_empty() {
        print!("{text}");
    } else {
        println!("{}", lines.join("\n"));
    }
    std::process::exit(0)
}

pub(super) fn exit_version() -> ! {
    println!(
        "{} {} ({})",
        option_env!("CARGO_BIN_NAME").unwrap_or("summonr"),
        env!("CARGO_PKG_VERSION"),
        summonr::revision()
    );
    std::process::exit(0)
}
