use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::process::{Cap, Env, Spec, Tail};

pub mod cli;

/// How a panel fills its tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fill {
    /// The command prints one JSON document, which the app draws.
    #[default]
    Data,
    /// The command is a program shown in a terminal.
    Terminal,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    title: Option<String>,
    #[serde(default)]
    kind: Fill,
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
}

impl Content {
    fn parse(output: &str) -> Result<Self, String> {
        let content: Self =
            serde_json::from_str(output).map_err(|error| format!("invalid output: {error}"))?;
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
            Self::Chart { .. } | Self::Stat { .. } | Self::Text { .. } => None,
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
            command: file.command,
            every_secs: file.every_secs,
            root: None,
            problem: None,
        },
        Err(problem) => Panel {
            title: name.clone(),
            name,
            kind: Fill::Data,
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

/// Runs a data panel's command in the workspace and reads what it printed.
pub async fn run(panel: &Panel, workspace: &Path) -> Result<Content, String> {
    if let Some(problem) = &panel.problem {
        return Err(problem.clone());
    }
    let (word, args) = panel
        .command
        .split_first()
        .ok_or_else(|| "`command` is empty".to_string())?;
    let spec = Spec {
        cwd: Some(workspace.to_path_buf()),
        base: Env::Clean,
        ..Spec::new(
            program(word, panel.root.as_deref().unwrap_or(workspace)),
            args,
        )
    };
    let process = crate::process::start(spec).map_err(|error| format!("did not start: {error}"))?;
    let limits = crate::limits::limits();
    let max = limits.panel_output_bytes;
    let mut child = process.child;
    let group = process.group;
    drop(process.stdin);
    let mut stdout = Tail::spawn(Some(process.stdout), Cap::Head(max + 1));
    let mut stderr = Tail::spawn(process.stderr, Cap::Tail(limits.mcp_stderr_tail_bytes));
    let work = async {
        let status = child.wait().await.map_err(|error| error.to_string())?;
        drop(stdout.settle().await);
        drop(stderr.settle().await);
        Ok::<_, String>(status)
    };
    let mut finished = None;
    crate::supervise::within(limits.panel_timeout(), async {
        finished = Some(work.await)
    })
    .await;
    drop(group);
    let status = finished.ok_or_else(|| format!("ran past {}s", limits.panel_timeout_secs))??;
    if !status.success() {
        let reason = stderr.text();
        return Err(match reason.trim() {
            "" => format!("exited with {:?}", status.code()),
            reason => reason.to_string(),
        });
    }
    let output = stdout.text();
    if output.len() > max {
        return Err(format!("printed more than {max} bytes"));
    }
    Content::parse(&output)
}
