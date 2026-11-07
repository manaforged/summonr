use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::IgnoredAny;

use super::{Place, RunMode};

#[derive(Clone, Default, Deserialize)]
pub(super) struct Table {
    setup: Option<Field>,
    run: Option<Run>,
    archive: Option<Field>,
    run_mode: Option<RunMode>,
    environment_variables: Option<BTreeMap<String, Value>>,
    file_include_globs: Option<String>,
}

#[derive(Default, Deserialize)]
pub(super) struct Settings {
    #[serde(default)]
    scripts: Table,
    environment_variables: Option<BTreeMap<String, Value>>,
    file_include_globs: Option<String>,
}

impl From<Settings> for Table {
    fn from(settings: Settings) -> Self {
        Self {
            environment_variables: settings.environment_variables,
            file_include_globs: settings.file_include_globs,
            ..settings.scripts
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum Field {
    Text(String),
    Other(IgnoredAny),
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum Run {
    Text(String),
    Actions(BTreeMap<String, Action>),
    Other(IgnoredAny),
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum Value {
    Text(String),
    Table(BTreeMap<String, String>),
    Other(IgnoredAny),
}

#[derive(Clone, Default, Deserialize)]
pub(super) struct Action {
    command: Option<String>,
    args: Option<Vec<String>>,
    options: Option<Options>,
    default: Option<bool>,
    icon: Option<String>,
    hide: Option<bool>,
    available_in: Option<Places>,
}

#[derive(Clone, Default, Deserialize)]
struct Options {
    cwd: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum Places {
    One(Place),
    Many(Vec<Place>),
}

impl Field {
    fn text(self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text),
            Self::Other(_) => None,
        }
    }
}

impl Run {
    fn actions(self) -> BTreeMap<String, Action> {
        match self {
            Self::Text(command) => BTreeMap::from([(
                String::new(),
                Action {
                    command: Some(command),
                    ..Action::default()
                },
            )]),
            Self::Actions(actions) => actions,
            Self::Other(_) => BTreeMap::new(),
        }
    }
}

impl Action {
    fn over(self, under: Self) -> Self {
        Self {
            command: self.command.or(under.command),
            args: self.args.or(under.args),
            options: self.options.or(under.options),
            default: self.default.or(under.default),
            icon: self.icon.or(under.icon),
            hide: self.hide.or(under.hide),
            available_in: self.available_in.or(under.available_in),
        }
    }

    fn into_action(self, id: String) -> Option<super::Action> {
        let places = match self.available_in {
            None => vec![Place::Local, Place::Cloud],
            Some(Places::One(place)) => vec![place],
            Some(Places::Many(places)) => places,
        };
        Some(super::Action {
            id,
            command: self.command?,
            args: self.args.unwrap_or_default(),
            cwd: self.options.and_then(|options| options.cwd),
            default: self.default.unwrap_or(false),
            icon: self.icon,
            hide: self.hide.unwrap_or(false),
            places,
        })
    }
}

impl Table {
    pub(super) fn over(self, under: Self) -> Self {
        Self {
            setup: self.setup.or(under.setup),
            run: Some(Run::Actions(merge(
                self.run.map(Run::actions).unwrap_or_default(),
                under.run.map(Run::actions).unwrap_or_default(),
                Action::over,
            ))),
            archive: self.archive.or(under.archive),
            run_mode: self.run_mode.or(under.run_mode),
            environment_variables: Some(merge(
                self.environment_variables.unwrap_or_default(),
                under.environment_variables.unwrap_or_default(),
                Value::over,
            )),
            file_include_globs: self.file_include_globs.or(under.file_include_globs),
        }
    }

    pub(super) fn into_parts(self) -> Parts {
        let mut actions: Vec<super::Action> = self
            .run
            .map(Run::actions)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(id, action)| action.into_action(id))
            .collect();
        actions.sort_by_key(|action| !action.default);
        Parts {
            setup: self.setup.and_then(Field::text),
            archive: self.archive.and_then(Field::text),
            actions,
            run_mode: self.run_mode.unwrap_or_default(),
            env: env(self.environment_variables.unwrap_or_default()),
            globs: self.file_include_globs,
        }
    }
}

impl Value {
    fn over(self, under: Self) -> Self {
        match (self, under) {
            (Self::Table(mut top), Self::Table(bottom)) => {
                for (name, value) in bottom {
                    top.entry(name).or_insert(value);
                }
                Self::Table(top)
            }
            (top, _) => top,
        }
    }
}

pub(super) struct Parts {
    pub(super) setup: Option<String>,
    pub(super) archive: Option<String>,
    pub(super) actions: Vec<super::Action>,
    pub(super) run_mode: RunMode,
    pub(super) env: super::Env,
    pub(super) globs: Option<String>,
}

fn merge<T>(
    mut top: BTreeMap<String, T>,
    bottom: BTreeMap<String, T>,
    over: fn(T, T) -> T,
) -> BTreeMap<String, T> {
    for (key, under) in bottom {
        let merged = match top.remove(&key) {
            Some(value) => over(value, under),
            None => under,
        };
        top.insert(key, merged);
    }
    top
}

fn env(table: BTreeMap<String, Value>) -> super::Env {
    let mut env = super::Env::default();
    for (name, value) in table {
        match (name.as_str(), value) {
            ("local", Value::Table(vars)) => env.local = vars,
            ("cloud", Value::Table(vars)) => env.cloud = vars,
            (_, Value::Text(text)) => drop(env.shared.insert(name, text)),
            _ => {}
        }
    }
    env
}
