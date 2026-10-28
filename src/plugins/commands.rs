use std::collections::HashSet;
use std::path::Path;

use super::{Plugin, enabled};
use crate::mcp::{Offer, OfferKind};
use crate::skills;
use summonr_core::error::Result;
use summonr_core::text::Frontmatter;

const DIR: &str = "commands";
const EXTENSION: &str = "md";
const DESCRIPTION: &str = "description";
const HINT: &str = "argument-hint";
const SIGIL: char = '$';
const ARGUMENTS: &str = "ARGUMENTS";
const FIRST: u8 = b'1';
const LAST: u8 = b'9';
const PROJECT: &str = "project";
const USER: &str = "user";

#[derive(Clone, Debug)]
pub(crate) struct Command {
    pub(crate) plugin: Plugin,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) hint: String,
    pub(crate) template: String,
}

impl Command {
    pub(crate) fn qualified(&self) -> String {
        format!("/{}:{}", self.plugin.name, self.name)
    }

    pub(crate) fn render(&self, arguments: &str, workspace: &Path) -> String {
        let template = self.plugin.expand(&self.template, workspace);
        let words = crate::policy::shell_words(arguments);
        let mut out = String::with_capacity(template.len() + arguments.len());
        let mut rest = template.as_str();
        while let Some(at) = rest.find(SIGIL) {
            out.push_str(&rest[..at]);
            let tail = &rest[at + SIGIL.len_utf8()..];
            if let Some(after) = tail.strip_prefix(ARGUMENTS) {
                out.push_str(arguments);
                rest = after;
                continue;
            }
            match tail.as_bytes().first() {
                Some(&digit @ FIRST..=LAST) => {
                    let index = usize::from(digit - FIRST);
                    out.push_str(words.get(index).map_or("", String::as_str));
                    rest = &tail[1..];
                }
                Some(_) | None => {
                    out.push(SIGIL);
                    rest = tail;
                }
            }
        }
        out.push_str(rest);
        out
    }

    fn about(&self) -> String {
        if self.hint.is_empty() {
            return self.description.clone();
        }
        format!("{} {}", self.description, self.hint)
    }
}

pub(crate) fn commands(workspace: &Path) -> Vec<Command> {
    let mut seen = HashSet::new();
    let mut all: Vec<Command> = folders(workspace)
        .iter()
        .flat_map(load)
        .filter(|command| seen.insert(command.name.clone()))
        .collect();
    all.extend(enabled().iter().flat_map(load));
    all
}

fn folders(workspace: &Path) -> Vec<Plugin> {
    let mut folders = vec![
        Plugin::at(PROJECT.to_string(), workspace.join(".summonr")),
        Plugin::at(PROJECT.to_string(), workspace.join(".claude")),
    ];
    if let Ok(home) = summonr_core::dirs::summonr_home() {
        folders.push(Plugin::at(USER.to_string(), home));
    }
    if let Some(home) = summonr_core::dirs::user_home() {
        folders.push(Plugin::at(USER.to_string(), home.join(".claude")));
    }
    folders
}

pub(crate) fn find(head: &str, workspace: &Path) -> Option<Command> {
    let head = head.strip_prefix('/')?;
    let all = commands(workspace);
    if let Some((plugin, name)) = head.split_once(':') {
        return all
            .into_iter()
            .find(|command| command.plugin.name == plugin && command.name == name);
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

pub(crate) fn expand(input: &str, workspace: &Path) -> Result<Option<String>> {
    let text = input.trim_start();
    let (head, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    if crate::parse_command(head).is_some() {
        return Ok(None);
    }
    if let Some(command) = find(head, workspace) {
        let text = command.render(rest.trim(), workspace);
        return Ok(Some(crate::secret::shield::scrub_found(&text, workspace)));
    }
    let Some(name) = head.strip_prefix('/').filter(|name| {
        skills::list(workspace)
            .iter()
            .any(|(known, _)| known.as_str() == *name)
    }) else {
        return Ok(None);
    };
    let body = skills::load(workspace, name)?;
    let body = crate::secret::shield::scrub_found(&body, workspace);
    Ok(Some(if rest.trim().is_empty() {
        body
    } else {
        format!("{body}\n\nSkill arguments:\n{}", rest.trim())
    }))
}

pub(crate) fn offers(workspace: &Path) -> Vec<Offer> {
    let all = commands(workspace);
    let mut offers = Vec::new();
    for command in &all {
        let about = command.about();
        if bare(&all, &command.name) {
            offers.push(Offer {
                kind: OfferKind::Prompt,
                insert: format!("/{}", command.name),
                about: about.clone(),
            });
        }
        offers.push(Offer {
            kind: OfferKind::Prompt,
            insert: command.qualified(),
            about,
        });
    }
    let offered: HashSet<_> = offers.iter().map(|offer| offer.insert.clone()).collect();
    for (name, about) in skills::list(workspace) {
        let insert = format!("/{name}");
        if crate::parse_command(&insert).is_none() && !offered.contains(&insert) {
            offers.push(Offer {
                kind: OfferKind::Prompt,
                insert,
                about,
            });
        }
    }
    offers
}

fn bare(all: &[Command], name: &str) -> bool {
    all.iter().filter(|command| command.name == name).count() == 1
        && crate::parse_command(&format!("/{name}")).is_none()
}

pub(super) fn load(plugin: &Plugin) -> Vec<Command> {
    let mut commands: Vec<Command> = summonr_core::walk::children(&plugin.dir.join(DIR))
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == EXTENSION))
        .filter_map(|path| read(plugin, &path))
        .collect();
    commands.sort_by(|left, right| left.name.cmp(&right.name));
    commands
}

fn read(plugin: &Plugin, path: &Path) -> Option<Command> {
    let name = path.file_stem()?.to_str()?.to_string();
    if !skills::valid(&name) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let front = Frontmatter::parse(&text);
    Some(Command {
        plugin: plugin.clone(),
        name,
        description: front.get(DESCRIPTION).unwrap_or_default().to_string(),
        hint: front.get(HINT).unwrap_or_default().to_string(),
        template: front.body.trim().to_string(),
    })
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
