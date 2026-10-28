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

pub(super) fn exit_help(verb: Option<&str>) -> ! {
    let endpoints = summonr::endpoint_names().map_or_else(
        |_| "the built-in endpoint names".to_string(),
        |names| names.join(", "),
    );
    let text = include_str!("../help.txt")
        .replace("{options}", &super::FLAGS.help_lines())
        .replace("{endpoints}", &endpoints)
        .replace("{commands}", &interactive())
        .replace("{sets}", &summonr::classify::sets::NAMES.join(", "));
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
