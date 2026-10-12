use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};

const TOOL: &str = "browser";
const EXTRAS: [&str; 6] = ["snapshot", "read", "screenshot", "tabs", "login", "close"];

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub(super) enum Extra {
    Snapshot {
        #[serde(default)]
        interactive_only: bool,
        max_chars: Option<usize>,
    },
    Read {
        #[serde(default)]
        format: ReadFormat,
        max_chars: Option<usize>,
    },
    Screenshot {
        #[serde(default)]
        full_page: bool,
    },
    Tabs,
    Login {
        url: String,
    },
    Close,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub(super) enum ReadFormat {
    #[default]
    Markdown,
    Text,
}

impl ReadFormat {
    pub(super) fn charm(self) -> charm::TextFormat {
        match self {
            Self::Markdown => charm::TextFormat::Markdown,
            Self::Text => charm::TextFormat::Text,
        }
    }
}

pub(super) enum Call {
    Extra(Extra),
    Act(charm::Action, charm::ActReport),
}

pub(super) fn parse(arguments: &str) -> Result<Call> {
    let value = crate::tools::parse_args(TOOL, arguments)?;
    let name = value
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Tool(format!("{TOOL} requires action")))?;
    if EXTRAS.contains(&name) {
        return serde_json::from_value(value)
            .map(Call::Extra)
            .map_err(invalid);
    }
    let report = match value.get("report") {
        Some(report) => serde_json::from_value(report.clone()).map_err(invalid)?,
        None => charm::ActReport::default(),
    };
    serde_json::from_value(value)
        .map(|action| Call::Act(action, report))
        .map_err(invalid)
}

pub(super) async fn guard(
    action: charm::Action,
    resolve: &(dyn Fn(&str) -> Result<PathBuf> + Send + Sync),
) -> Result<charm::Action> {
    use charm::{Action, TabAction};
    Ok(match action {
        Action::Navigate { url } => Action::Navigate {
            url: crate::web::allowed_url(&url).await?,
        },
        Action::Tab(TabAction::New(Some(url))) => {
            Action::Tab(TabAction::New(Some(crate::web::allowed_url(&url).await?)))
        }
        Action::Upload { target, files } => Action::Upload {
            target,
            files: files
                .iter()
                .map(|file| resolve(&file.to_string_lossy()))
                .collect::<Result<Vec<PathBuf>>>()?,
        },
        other => other,
    })
}

fn invalid(error: serde_json::Error) -> Error {
    Error::Tool(format!("{TOOL} arguments are invalid: {error}"))
}
