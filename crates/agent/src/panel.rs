use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

pub mod cli;
mod run;
mod stream;

pub(crate) use run::output;
pub use run::{action, run};
pub(crate) use stream::Stream;

/// How a panel fills its tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fill {
    /// The command prints one JSON document, which the app draws.
    #[default]
    Data,
    /// The command is a program shown in a terminal.
    Terminal,
    /// The command keeps running: each line it prints is a whole surface, and each event goes to its stdin as a line.
    Stream,
}

/// Where a panel shows in the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// A tab in the right panel.
    #[default]
    Right,
    /// A section of the sidebar, under the repositories.
    Side,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    title: Option<String>,
    #[serde(default)]
    kind: Fill,
    #[serde(default)]
    place: Place,
    icon: Option<String>,
    command: Vec<String>,
    every_secs: Option<u64>,
}

/// One tab a person or an agent added to the right panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    /// The file's name without its extension.
    pub name: String,
    /// The tab's label.
    pub title: String,
    /// How the panel fills its tab.
    pub kind: Fill,
    /// Where the panel shows.
    pub place: Place,
    /// The Lucide name of the icon for the panel's rail button.
    pub icon: Option<String>,
    /// The program and its arguments.
    pub command: Vec<String>,
    /// How often the open panel runs again, if at all.
    pub every_secs: Option<u64>,
    /// The plugin directory that a relative program sits under.
    pub root: Option<PathBuf>,
    /// Why the file could not be read.
    pub problem: Option<String>,
}

impl Panel {
    /// The wait between runs of the open panel.
    #[must_use]
    pub fn every(&self) -> Option<Duration> {
        let least = crate::limits::limits().panel_every_min_secs;
        self.every_secs
            .map(|secs| Duration::from_secs(secs.max(least)))
    }
}

/// The form of a chart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plot {
    /// Connected points.
    #[default]
    Line,
    /// Upright bars.
    Bar,
    /// A filled line.
    Area,
    /// Slices of a whole.
    Pie,
}

/// One named row of chart values.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Series {
    /// The legend text.
    #[serde(default)]
    pub name: String,
    /// One value per label.
    pub values: Vec<f32>,
}

/// The meaning of a stat, which the app colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    /// No judgment.
    #[default]
    Neutral,
    /// Good.
    Success,
    /// Needs a look.
    Warning,
    /// Bad.
    Danger,
}

/// One labelled number or word.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stat {
    /// What it measures.
    pub label: String,
    /// The formatted value.
    pub value: String,
    /// What the value means.
    #[serde(default)]
    pub tone: Mood,
}

/// What clicking a list row does; extra fields, such as more A2UI event context, are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Starts a new agent with this prompt.
    Start {
        /// The first message.
        prompt: String,
    },
    /// Opens the agent with this session id.
    Open {
        /// The session id.
        session: String,
    },
}

/// One row of a list.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// The row's text.
    pub title: String,
    /// A short line after the title.
    #[serde(default)]
    pub meta: Option<String>,
    /// What the meta line means, which the app colors.
    #[serde(default)]
    pub tone: Mood,
    /// What clicking the row does.
    #[serde(default)]
    pub action: Option<Action>,
}

/// What a data panel's command prints.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "view", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    /// Values over labels.
    Chart {
        /// The form of the chart.
        #[serde(default)]
        kind: Plot,
        /// One label per value.
        #[serde(default)]
        labels: Vec<String>,
        /// The rows of values.
        series: Vec<Series>,
    },
    /// A row of labelled values.
    Stat {
        /// The values.
        items: Vec<Stat>,
    },
    /// Rows under column names.
    Table {
        /// The column names.
        columns: Vec<String>,
        /// One cell per column in each row.
        rows: Vec<Vec<String>>,
    },
    /// Plain text.
    Text {
        /// The text.
        text: String,
    },
    /// Rows that each start or open an agent.
    List {
        /// The rows.
        rows: Vec<Item>,
    },
    /// An A2UI surface, kept as printed for the renderer to read.
    #[serde(skip)]
    Surface(String),
}

impl Content {
    fn parse(output: &str) -> Result<Self, String> {
        let document: serde_json::Value =
            serde_json::from_str(output).map_err(|error| format!("invalid output: {error}"))?;
        if document.get("view").is_none() {
            return Ok(Self::Surface(output.trim().to_string()));
        }
        let content: Self =
            serde_json::from_value(document).map_err(|error| format!("invalid output: {error}"))?;
        content.mismatch().map_or(Ok(content), Err)
    }

    fn mismatch(&self) -> Option<String> {
        match self {
            Self::Chart { labels, series, .. } if !labels.is_empty() => series
                .iter()
                .find(|row| row.values.len() != labels.len())
                .map(|row| {
                    format!(
                        "series `{}` has {} values for {} labels",
                        row.name,
                        row.values.len(),
                        labels.len()
                    )
                }),
            Self::Table { columns, rows } => rows
                .iter()
                .find(|row| row.len() != columns.len())
                .map(|row| {
                    format!(
                        "a row has {} cells for {} columns",
                        row.len(),
                        columns.len()
                    )
                }),
            Self::Chart { .. }
            | Self::Stat { .. }
            | Self::Text { .. }
            | Self::List { .. }
            | Self::Surface(_) => None,
        }
    }
}

/// The panels of a workspace, where a project's panel replaces a user panel of the same name.
#[must_use]
pub fn list(workspace: &Path, trust: bool) -> Vec<Panel> {
    let mut found = BTreeMap::new();
    for plugin in crate::plugins::enabled() {
        read_dir(&plugin.dir.join("panels"), Some(&plugin.dir), &mut found);
    }
    if let Ok(home) = summonr_core::dirs::summonr_home() {
        read_dir(&home.join("panels"), None, &mut found);
    }
    if trust {
        read_dir(&workspace.join(".summonr").join("panels"), None, &mut found);
    }
    found
        .into_values()
        .take(crate::limits::limits().panels_max)
        .collect()
}

fn read_dir(dir: &Path, root: Option<&Path>, found: &mut BTreeMap<String, Panel>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let max = crate::limits::limits().panels_max;
    for path in entries.flatten().map(|entry| entry.path()).take(max) {
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
            && let Some(panel) = read_file(&path)
        {
            let root = root.map(Path::to_path_buf);
            found.insert(panel.name.clone(), Panel { root, ..panel });
        }
    }
}

fn read_file(path: &Path) -> Option<Panel> {
    let name = path.file_stem()?.to_string_lossy().into_owned();
    Some(match parse_file(path) {
        Ok(file) => Panel {
            title: file.title.unwrap_or_else(|| name.clone()),
            name,
            kind: file.kind,
            place: file.place,
            icon: file.icon,
            command: file.command,
            every_secs: file.every_secs,
            root: None,
            problem: None,
        },
        Err(problem) => Panel {
            title: name.clone(),
            name,
            kind: Fill::Data,
            place: Place::Right,
            icon: None,
            command: Vec::new(),
            every_secs: None,
            root: None,
            problem: Some(problem),
        },
    })
}

fn parse_file(path: &Path) -> Result<File, String> {
    let max = crate::limits::limits().panel_output_bytes;
    let meta = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() || meta.len() > max as u64 {
        return Err(format!("not a file of at most {max} bytes"));
    }
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let file: File = toml::from_str(&text).map_err(|error| error.to_string())?;
    if file.command.is_empty() {
        return Err("`command` is empty".to_string());
    }
    Ok(file)
}

fn program(word: &str, base: &Path) -> PathBuf {
    let path = Path::new(word);
    if path.is_relative() && path.components().count() > 1 {
        base.join(path)
    } else {
        path.to_path_buf()
    }
}

/// The shell line that starts a terminal panel's program.
#[must_use]
pub fn shell_line(panel: &Panel, workspace: &Path) -> String {
    let base = panel.root.as_deref().unwrap_or(workspace);
    panel
        .command
        .iter()
        .enumerate()
        .map(|(index, word)| {
            if index == 0 {
                summonr_core::shell::quote(&program(word, base).to_string_lossy())
            } else {
                summonr_core::shell::quote(word)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
