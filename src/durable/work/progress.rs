use super::*;

pub(crate) fn roman(number: usize) -> String {
    const DIGITS: [(usize, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut rest = number;
    let mut text = String::new();
    for (value, glyph) in DIGITS {
        while rest >= value {
            text.push_str(glyph);
            rest -= value;
        }
    }
    text
}

pub(crate) fn checks(
    markdown: &str,
    policy: WorkKind,
    commands: &[String],
) -> Result<Vec<Check>, WorkError> {
    let mut checks: Vec<Check> = commands
        .iter()
        .map(|command| Check {
            command: command.clone(),
            observation: None,
        })
        .collect();
    if policy != WorkKind::Steps {
        return Ok(checks);
    }
    let mut push = |command: String, observation: Option<Observation>| {
        if !checks
            .iter()
            .any(|known| known.command == command && known.observation == observation)
        {
            checks.push(Check {
                command,
                observation,
            });
        }
    };
    for command in crate::steps::parse(markdown)?
        .iter()
        .flat_map(|step| &step.checks)
    {
        push(command.clone(), None);
    }
    for item in crate::steps::contract(markdown)?.items {
        match item {
            crate::steps::Item::File(path) => {
                push(
                    format!("file exists: {path}"),
                    Some(Observation::File(path)),
                );
            }
            crate::steps::Item::Stdout { command, expected } => {
                push(command, Some(Observation::Stdout(expected)));
            }
        }
    }
    Ok(checks)
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Persisted {
    Single,
    Steps,
    Goal {},
}

pub(super) fn persist<S: serde::Serializer>(kind: &WorkKind, to: S) -> Result<S::Ok, S::Error> {
    match kind {
        WorkKind::Single => Persisted::Single,
        WorkKind::Steps => Persisted::Steps,
        WorkKind::Goal => Persisted::Goal {},
    }
    .serialize(to)
}

pub(super) fn restore<'de, D: serde::Deserializer<'de>>(from: D) -> Result<WorkKind, D::Error> {
    Ok(match Persisted::deserialize(from)? {
        Persisted::Single => WorkKind::Single,
        Persisted::Steps => WorkKind::Steps,
        Persisted::Goal {} => WorkKind::Goal,
    })
}
