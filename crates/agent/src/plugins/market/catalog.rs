use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};
use summonr_core::error::{Error, Result};

use super::origin::Origin;

pub(super) const CATALOG: &str = ".claude-plugin/marketplace.json";

const RESERVED: [&str; 19] = [
    "claude-code-marketplace",
    "claude-code-plugins",
    "claude-plugins-official",
    "anthropic-marketplace",
    "anthropic-plugins",
    "agent-skills",
    "anthropic-agent-skills",
    "life-sciences",
    "knowledge-work-plugins",
    "claude-for-legal",
    "claude-for-financial-services",
    "financial-services-plugins",
    "first-party-plugins",
    "claude-tag-plugins",
    "claude-community",
    "claude-plugins-community",
    "healthcare",
    "anthropic-plugin-directory",
    "claude-plugin-directory",
];

const NOT_MARKETPLACES: [&str; 12] = [
    "inline",
    "builtin",
    "skills-dir",
    "synced",
    "claude-plugin-test",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
];

const HOSTED_PREFIX: &str = "claudeai-";

/// A parsed `.claude-plugin/marketplace.json`.
#[derive(Debug, Deserialize)]
pub(super) struct Catalog {
    pub(super) name: String,
    owner: Owner,
    pub(super) plugins: Vec<Value>,
    #[serde(default)]
    metadata: Metadata,
}

#[derive(Debug, Deserialize)]
struct Owner {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    plugin_root: Option<String>,
}

/// One plugin entry of a catalog.
#[derive(Debug)]
pub(super) struct Entry {
    pub(super) name: String,
    pub(super) source: Source,
    pub(super) strict: bool,
    pub(super) fields: Map<String, Value>,
}

/// Where a plugin entry says to fetch the plugin.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum Source {
    Relative(String),
    Remote(Remote),
}

/// A plugin source object, named by its own `source` key.
#[derive(Debug, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub(super) enum Remote {
    Github {
        repo: String,
        #[serde(rename = "ref")]
        reference: Option<String>,
        sha: Option<String>,
    },
    Url {
        url: String,
        #[serde(rename = "ref")]
        reference: Option<String>,
        sha: Option<String>,
    },
    GitSubdir {
        url: String,
        path: String,
        #[serde(rename = "ref")]
        reference: Option<String>,
        sha: Option<String>,
    },
    Npm {},
    Archive {},
    Command {},
}

impl Catalog {
    pub(super) fn read(path: &Path) -> Result<Self> {
        let catalog =
            super::super::read_parsed(path, |text: &str| serde_json::from_str::<Self>(text))?
                .ok_or_else(|| {
                    Error::Config(format!("{}: no marketplace file here", path.display()))
                })?;
        if catalog.owner.name.trim().is_empty() {
            return Err(Error::Config(format!(
                "{}: owner.name: Author name cannot be empty",
                path.display()
            )));
        }
        if !id_part(&catalog.name) {
            return Err(Error::Config(format!(
                "summonr cannot install plugins from marketplace \"{}\": each part of a plugin id (plugin@marketplace) may use only letters, digits, \".\", \"_\" and \"-\", and must start with a letter or digit",
                catalog.name
            )));
        }
        Ok(catalog)
    }

    pub(super) fn entry(&self, plugin: &str) -> Result<Entry> {
        let raw = self
            .plugins
            .iter()
            .find(|raw| raw["name"] == plugin)
            .ok_or_else(|| {
                Error::Config(format!(
                    "Plugin \"{plugin}\" not found in marketplace \"{}\"",
                    self.name
                ))
            })?;
        Entry::parse(raw)
    }

    /// The directory a relative source names, kept inside `root` through any link.
    pub(super) fn relative(&self, written: &str, root: &Path) -> Result<PathBuf> {
        let refused = |why: &str| Error::Config(format!("source `{written}`: {why}"));
        let path = match self.metadata.plugin_root.as_deref() {
            Some(base) if !written.contains('/') && written != "." => {
                format!("{}/{written}", base.trim_end_matches('/'))
            }
            Some(_) | None if written == "." || written.starts_with("./") => written.to_string(),
            Some(_) | None => return Err(refused("Invalid string: must start with \"./\"")),
        };
        if cfg!(not(windows)) && path.contains('\\') {
            return Err(refused("write the path with forward slashes"));
        }
        let joined = super::super::inside(root, &path)
            .ok_or_else(|| refused(&format!("Path contains \"..\": {written}")))?;
        dir_under(&joined, root)?
            .ok_or_else(|| refused("leaves the marketplace or is not a directory"))
    }
}

/// `path` resolved through links when it is a directory under `root`, or `None` when it leaves.
pub(super) fn dir_under(path: &Path, root: &Path) -> Result<Option<PathBuf>> {
    let real = path.canonicalize().map_err(|_missing| {
        Error::Config(format!("Source path does not exist: {}", path.display()))
    })?;
    Ok((real.starts_with(root.canonicalize()?) && real.is_dir()).then_some(real))
}

impl Entry {
    fn parse(raw: &Value) -> Result<Self> {
        let Value::Object(fields) = raw else {
            return Err(Error::Config("a plugin entry is not an object".to_string()));
        };
        let name = fields
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !id_part(&name) {
            return Err(Error::Config(format!(
                "summonr cannot install plugin \"{name}\": a plugin name may use only letters, digits, \".\", \"_\" and \"-\", and must start with a letter or digit"
            )));
        }
        let source = fields
            .get("source")
            .cloned()
            .map(serde_json::from_value::<Source>)
            .and_then(std::result::Result::ok)
            .ok_or_else(|| Error::Config(format!("plugin `{name}`: source: Invalid input")))?;
        Ok(Self {
            strict: fields
                .get("strict")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            name,
            source,
            fields: fields.clone(),
        })
    }
}

/// Whether `name` is a valid part of a `plugin@marketplace` id.
pub(super) fn id_part(name: &str) -> bool {
    name.starts_with(|first: char| first.is_ascii_alphanumeric())
        && summonr_core::text::is_name(name, "._-")
        && !name.contains("..")
}

/// Refuses a marketplace name Claude Code reserves, unless the source is Anthropic's own.
pub(super) fn allowed(name: &str, origin: &Origin) -> Result<()> {
    if RESERVED.contains(&name) && !origin.official() {
        return Err(Error::Config(format!(
            "The name '{name}' is reserved for official Anthropic marketplaces"
        )));
    }
    if NOT_MARKETPLACES.contains(&name) {
        return Err(Error::Config(format!(
            "`{name}` is reserved and cannot name a marketplace"
        )));
    }
    if name.starts_with(HOSTED_PREFIX) {
        return Err(Error::Config(format!(
            "Cannot add marketplace \"{name}\": names starting with \"{HOSTED_PREFIX}\" are reserved for marketplaces hosted on claude.ai"
        )));
    }
    Ok(())
}
