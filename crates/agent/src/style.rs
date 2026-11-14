use std::path::{Path, PathBuf};

use summonr_core::text::Frontmatter;

use crate::plugins::agents::Origin;
use crate::plugins::{Plugin, Source};

const DIR: &str = "output-styles";
/// The plugin manifest key that replaces the default `output-styles/` folder.
pub(crate) const MANIFEST_KEY: &str = "outputStyles";
const NAME: &str = "name";
const DESCRIPTION: &str = "description";
const KEEP_CODING: &str = "keep-coding-instructions";
const FORCE: &str = "force-for-plugin";
/// The name `/output-style` lists for the built-in default.
pub(crate) const DEFAULT: &str = "default";
const DEFAULT_ABOUT: &str = "no output style";

/// One custom output style read from a Markdown file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Style {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) instructions: String,
    pub(crate) keep_coding: bool,
    pub(crate) force: bool,
    pub(crate) origin: Origin,
}

/// The output style a session runs with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    Default,
    Custom(Style),
}

impl Pick {
    /// The name `/output-style` shows for this choice.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Default => DEFAULT,
            Self::Custom(style) => &style.name,
        }
    }

    /// The custom style, or `None` for the default.
    pub(crate) fn style(&self) -> Option<&Style> {
        match self {
            Self::Default => None,
            Self::Custom(style) => Some(style),
        }
    }
}

/// The directories and plugins output styles are read from.
pub(crate) struct Roots {
    pub(crate) project: Vec<PathBuf>,
    pub(crate) user: Option<PathBuf>,
    pub(crate) plugins: Vec<Plugin>,
}

impl Roots {
    /// The roots for `workspace`; project directories count only in a trusted workspace.
    pub(crate) fn of(workspace: &Path, trust: bool) -> Self {
        let project = if trust {
            crate::plugins::agents::project_dirs(workspace)
                .into_iter()
                .map(|dir| dir.join(DIR))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            project,
            user: summonr_core::dirs::user_home()
                .map(|home| summonr_core::dirs::claude(&home).join(DIR)),
            plugins: crate::plugins::enabled(),
        }
    }

    /// Every style: project directories closest first, then the user, then plugins; a taken name is skipped.
    pub(crate) fn styles(&self) -> Vec<Style> {
        let mut found: Vec<Style> = self
            .project
            .iter()
            .flat_map(|dir| of_dir(dir, &Origin::Project))
            .collect();
        found.extend(self.user.iter().flat_map(|dir| of_dir(dir, &Origin::User)));
        found.extend(self.plugins.iter().flat_map(of_plugin));
        let mut kept: Vec<Style> = Vec::new();
        for style in found {
            if !kept.iter().any(|seen| seen.name == style.name) {
                kept.push(style);
            }
        }
        kept
    }
}

/// The style a session starts with: the first forcing plugin style, else the exact configured name, else the default.
pub(crate) fn chosen(styles: &[Style], configured: Option<&str>) -> Pick {
    let forced = styles
        .iter()
        .find(|style| style.force && matches!(style.origin, Origin::Plugin(_)));
    forced
        .or_else(|| {
            styles
                .iter()
                .find(|style| Some(style.name.as_str()) == configured)
        })
        .map_or(Pick::Default, |style| Pick::Custom(style.clone()))
}

/// The style `typed` names with case ignored, as `/output-style NAME` matches it.
pub(crate) fn named(styles: &[Style], typed: &str) -> Option<Pick> {
    let typed = typed.trim();
    if typed.eq_ignore_ascii_case(DEFAULT) {
        return Some(Pick::Default);
    }
    styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(typed))
        .map(|style| Pick::Custom(style.clone()))
}

/// The `output_style` key: a trusted workspace `.summonr/config.toml` over `~/.summonr/config.toml`, then imported Claude Code `outputStyle`.
pub(crate) fn configured(workspace: &Path, trust: bool) -> Option<String> {
    let project = trust
        .then(|| crate::config::workspace(workspace).ok())
        .flatten()
        .and_then(|file| file.output_style.clone());
    project
        .or_else(|| crate::config::file().ok()?.output_style.clone())
        .or_else(|| crate::config::claude::load(workspace, trust).style)
}

/// The style a new session in `workspace` starts with.
pub(crate) fn active(workspace: &Path, trust: bool) -> Pick {
    let styles = Roots::of(workspace, trust).styles();
    chosen(&styles, configured(workspace, trust).as_deref())
}

/// The `/output-style` list: the default and each style, with `*` on `current`.
pub(crate) fn listing(styles: &[Style], current: &str) -> String {
    let rows = std::iter::once((DEFAULT, DEFAULT_ABOUT)).chain(
        styles
            .iter()
            .map(|style| (style.name.as_str(), style.description.as_str())),
    );
    rows.map(|(name, about)| {
        let mark = if name == current { '*' } else { ' ' };
        format!("{mark} {name}  {about}").trim_end().to_string()
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn of_dir(dir: &Path, origin: &Origin) -> Vec<Style> {
    let mut files: Vec<PathBuf> = summonr_core::walk::children(dir)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && crate::skills::is_markdown(path))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|path| read(path, origin.clone()))
        .collect()
}

/// Each output style `plugin` ships, by its manifest paths or its `output-styles/` folder.
pub(crate) fn of_plugin(plugin: &Plugin) -> Vec<Style> {
    let origin = Origin::Plugin(plugin.name.clone());
    plugin.components(
        MANIFEST_KEY,
        DIR,
        |dir| of_dir(dir, &origin),
        |source| match source {
            Source::File(path) if path.is_dir() => of_dir(&path, &origin),
            Source::File(path) => read(&path, origin.clone()).into_iter().collect(),
            Source::Inline(_) | Source::Outside(_) => Vec::new(),
        },
    )
}

fn read(path: &Path, origin: Origin) -> Option<Style> {
    let stem = path.file_stem()?.to_str()?.to_string();
    crate::skills::markdown_file(path, stem, |stem, front: Frontmatter<'_>| {
        let plugin = matches!(origin, Origin::Plugin(_));
        Some(Style {
            name: front.get(NAME).map_or(stem, str::to_string),
            description: front.get(DESCRIPTION).unwrap_or_default().to_string(),
            instructions: front.body.trim().to_string(),
            keep_coding: crate::skills::flag(&front, KEEP_CODING, false),
            force: plugin && crate::skills::flag(&front, FORCE, false),
            origin: origin.clone(),
        })
    })
}

/// A session's mid-session switch: the chosen style and the note the next turn carries.
#[derive(Debug, Default)]
pub(crate) struct Switch {
    chosen: Option<Pick>,
    note: Option<String>,
}

impl Switch {
    /// Chooses `pick` from the next turn, which carries its instructions as a note.
    pub(crate) fn choose(&mut self, pick: Pick) {
        self.note = Some(crate::prompt::style_note(&pick));
        self.chosen = Some(pick);
    }

    /// The note for the next turn, given once.
    pub(crate) fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// The name of the style in force: the chosen one, else the one `system` carries.
    pub(crate) fn current(&self, system: &str) -> String {
        self.chosen.as_ref().map_or_else(
            || {
                crate::prompt::style_name(system)
                    .unwrap_or(DEFAULT)
                    .to_string()
            },
            |pick| pick.name().to_string(),
        )
    }

    /// `system` rebuilt for the chosen style once history is rewritten, or `None` with no switch.
    pub(crate) fn rewritten(&mut self, system: &str) -> Option<String> {
        let pick = self.chosen.as_ref()?;
        self.note = None;
        Some(crate::prompt::restyle(system, pick.style()))
    }
}

#[cfg(test)]
#[path = "style_tests.rs"]
mod tests;
