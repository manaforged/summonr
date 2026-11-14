use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use super::{Plugin, Source, enabled};
use crate::mcp::{Offer, OfferKind};
use crate::skills::{self, Body, Meta, Piece, SEPARATOR, Scope};
use summonr_core::error::Result;
use summonr_core::text::Frontmatter;

pub(crate) use super::expand::content;

const DIR: &str = "commands";
const KEY: &str = "commands";
const SOURCE: &str = "source";
const CONTENT: &str = "content";
const DESCRIPTION: &str = "description";
const HINT: &str = "argumentHint";
const MODEL: &str = "model";
const TOOLS: &str = "allowedTools";
const MARKDOWN: &str = "md";
const PROJECT: &str = "project";
const USER: &str = "user";

#[derive(Clone, Debug)]
pub(crate) struct Command {
    pub(crate) plugin: Plugin,
    pub(crate) name: String,
    pub(crate) meta: Meta,
    pub(crate) template: String,
    folder: bool,
}

impl Command {
    fn new(plugin: &Plugin, name: String, front: &Frontmatter<'_>) -> Option<Self> {
        name.split(SEPARATOR).all(skills::valid).then(|| Self {
            plugin: plugin.clone(),
            name,
            meta: Meta::parse(front),
            template: front.body.trim().to_string(),
            folder: false,
        })
    }

    pub(crate) fn qualified(&self) -> String {
        format!("/{}", self.plugin.scoped(&self.name))
    }

    pub(crate) fn render(&self, arguments: &str, workspace: &Path, session: Option<&str>) -> Body {
        let plugin = (!self.folder).then_some(&self.plugin);
        Body::render(
            &self.template,
            Some((arguments, self.meta.arguments.as_slice())),
            Scope::of(&self.plugin.dir, workspace),
            |text| content(text, plugin, None, workspace, session),
        )
    }
}

pub(crate) fn commands(workspace: &Path) -> Vec<Command> {
    let mut seen = HashSet::new();
    let mut all: Vec<Command> = folders(workspace)
        .iter()
        .flat_map(load)
        .map(|command| Command {
            folder: true,
            ..command
        })
        .filter(|command| seen.insert(command.name.clone()))
        .collect();
    all.extend(enabled().iter().flat_map(load));
    all.retain(|command| command.meta.user_invocable);
    all
}

fn folders(workspace: &Path) -> Vec<Plugin> {
    let mut folders = vec![
        Plugin::at(
            PROJECT.to_string(),
            summonr_core::dirs::workspace(workspace),
        ),
        Plugin::at(PROJECT.to_string(), summonr_core::dirs::claude(workspace)),
    ];
    if let Ok(home) = summonr_core::dirs::summonr_home() {
        folders.push(Plugin::at(USER.to_string(), home));
    }
    if let Some(home) = summonr_core::dirs::user_home() {
        folders.push(Plugin::at(
            USER.to_string(),
            summonr_core::dirs::claude(&home),
        ));
    }
    folders
}

pub(crate) fn find(head: &str, workspace: &Path) -> Option<Command> {
    let head = head.strip_prefix('/')?;
    let all = commands(workspace);
    if let Some((plugin, name)) = head.split_once(SEPARATOR)
        && let Some(command) = all
            .iter()
            .find(|command| command.plugin.name == plugin && command.name == name)
    {
        return Some(command.clone());
    }
    bare(&all, head)
        .then(|| all.into_iter().find(|command| command.name == head))
        .flatten()
}

pub(crate) fn named(line: &str, workspace: &Path) -> bool {
    line.split_whitespace()
        .next()
        .and_then(|head| find(head, workspace))
        .is_some()
}

/// Expands a slash command or skill for `session`, its text scrubbed; with no arguments, `previous` is the task.
pub(crate) fn expand(
    input: &str,
    workspace: &Path,
    previous: Option<&str>,
    session: Option<&str>,
) -> Result<Option<Body>> {
    let text = input.trim_start();
    let (head, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    if crate::parse_command(head).is_some() {
        return Ok(None);
    }
    let Some(name) = head.strip_prefix('/') else {
        return Ok(None);
    };
    let task = match rest.trim() {
        "" => previous.unwrap_or_default().trim(),
        given => given,
    };
    let mut body = match (skills::typed(workspace, name), find(head, workspace)) {
        (Some(skill), Some(command)) if skill.alias => command.render(task, workspace, session),
        (Some(skill), _) => skill.render(task, workspace, session)?,
        (None, Some(command)) => command.render(task, workspace, session),
        (None, None) => return Ok(None),
    };
    for piece in &mut body.pieces {
        if let Piece::Text(text) = piece {
            *text = crate::secret::shield::scrub_found(text, workspace);
        }
    }
    Ok(Some(body))
}

pub(crate) fn offers(workspace: &Path) -> Vec<Offer> {
    let skills = skills::entries(workspace);
    let skill = |alias: bool| {
        skills
            .iter()
            .filter(move |entry| entry.alias == alias)
            .map(|entry| (format!("/{}", entry.name), entry.offer.clone()))
            .filter(|(insert, _)| crate::parse_command(insert).is_none())
    };
    let all = commands(workspace);
    let mut wanted: Vec<(String, String)> = skill(false).collect();
    for command in &all {
        let about = command.meta.offer();
        if bare(&all, &command.name) {
            wanted.push((format!("/{}", command.name), about.clone()));
        }
        wanted.push((command.qualified(), about));
    }
    wanted.extend(skill(true));
    let mut seen = HashSet::new();
    wanted
        .into_iter()
        .filter(|(insert, _)| seen.insert(insert.clone()))
        .map(|(insert, about)| Offer {
            kind: OfferKind::Prompt,
            insert,
            about,
        })
        .collect()
}

fn bare(all: &[Command], name: &str) -> bool {
    all.iter().filter(|command| command.name == name).count() == 1
        && crate::parse_command(&format!("/{name}")).is_none()
}

/// The commands `plugin` ships: the manifest `commands` key when set, else every file under `commands/`.
pub(super) fn load(plugin: &Plugin) -> Vec<Command> {
    let build = |name: String, front: Frontmatter<'_>| Command::new(plugin, name, &front);
    let mut commands: Vec<Command> = match plugin.declared(KEY) {
        Some(sources) => sources
            .into_iter()
            .flat_map(|source| match source {
                Source::File(path) => listed(&path, build),
                Source::Inline(Value::Object(map)) => map
                    .iter()
                    .filter_map(|(name, entry)| mapped(plugin, name, entry))
                    .collect(),
                Source::Inline(_) | Source::Outside(_) => Vec::new(),
            })
            .collect(),
        None => skills::markdown_tree(&plugin.dir.join(DIR), build),
    };
    commands.sort_by(|left, right| left.name.cmp(&right.name));
    commands.dedup_by(|left, right| left.name == right.name);
    commands
}

fn listed(path: &Path, build: impl Fn(String, Frontmatter<'_>) -> Option<Command>) -> Vec<Command> {
    if path.is_dir() {
        return skills::markdown_tree(path, build);
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|_| path.extension().is_some_and(|ext| ext == MARKDOWN))
        .and_then(|stem| skills::markdown_file(path, stem.to_owned(), build))
        .into_iter()
        .collect()
}

fn mapped(plugin: &Plugin, name: &str, entry: &Value) -> Option<Command> {
    let field = |key: &str| entry.get(key).and_then(Value::as_str);
    let text = match (field(SOURCE), field(CONTENT)) {
        (Some(source), None) => std::fs::read_to_string(plugin.inside(source)?).ok()?,
        (None, Some(content)) => content.to_owned(),
        (Some(_), Some(_)) | (None, None) => return None,
    };
    let mut command = Command::new(plugin, name.to_owned(), &Frontmatter::parse(&text))?;
    if let Some(about) = field(DESCRIPTION) {
        about.clone_into(&mut command.meta.about);
    }
    if let Some(hint) = field(HINT) {
        hint.clone_into(&mut command.meta.hint);
    }
    if let Some(model) = field(MODEL) {
        command.meta.model = Some(model.to_owned());
    }
    if let Some(tools) = entry.get(TOOLS).and_then(Value::as_array) {
        command.meta.allowed_tools = tools
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    Some(command)
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
