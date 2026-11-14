use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use summonr_core::dirs::summonr_home;
use summonr_core::error::{Error, Result};

use super::manage::{Plugin, copy_tree};
use crate::durable::store::{self, Durability};
use catalog::{Catalog, Entry, Source, allowed, id_part};
use origin::{Origin, REPO};

mod catalog;
mod fetch;
mod merge;
mod origin;

const RECORD: &str = "record.json";
const FETCH: &str = "fetch";
const FRESH: &str = ".repo.fresh";
const PREVIOUS: &str = ".repo.previous";

/// A `summonr plugin marketplace` subcommand.
#[derive(Clone, Debug)]
pub enum Command {
    /// `marketplace add SOURCE`.
    Add(String),
    /// `marketplace list`.
    List,
    /// `marketplace remove NAME`.
    Remove(String),
    /// `marketplace update [NAME]`.
    Update(Option<String>),
}

/// Runs a marketplace subcommand and returns what it prints.
pub async fn run(command: &Command, json: bool) -> Result<String> {
    match command {
        Command::Add(source) => Marketplace::add(source).await,
        Command::List => Marketplace::list(json),
        Command::Remove(name) => Marketplace::remove(name),
        Command::Update(name) => Marketplace::update(name.as_deref()).await,
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    source: Origin,
    #[serde(default)]
    plugins: Vec<String>,
}

/// A plugin catalog added under `~/.summonr/marketplaces/<name>/`.
#[derive(Debug)]
pub struct Marketplace {
    /// The name the catalog gives itself, typed after `@` in a plugin id.
    pub name: String,
    dir: PathBuf,
    record: Record,
}

impl Marketplace {
    /// Adds the marketplace that `input` names and returns the line to print.
    pub async fn add(input: &str) -> Result<String> {
        let origin = Origin::parse(input)?;
        let home = home()?;
        std::fs::create_dir_all(&home)?;
        let _lock = hold(&home)?;
        let stage = home.join(format!(".adding-{}", std::process::id()));
        clear(&stage)?;
        std::fs::create_dir_all(&stage)?;
        let added = Self::place(&home, &stage, origin).await;
        clear(&stage)?;
        added
    }

    async fn place(home: &Path, stage: &Path, origin: Origin) -> Result<String> {
        origin.fetch(&stage.join(REPO)).await?;
        let catalog = Catalog::read(&origin.catalog(&origin.root(stage)))?;
        let name = catalog.name.clone();
        allowed(&name, &origin)?;
        if let Ok(have) = Self::open(&name) {
            if have.record.source == origin {
                return Ok(format!("marketplace `{name}` is already added"));
            }
            return Err(Error::Config(format!(
                "a marketplace named `{name}` is already added from {}; `summonr plugin marketplace remove {name}` first",
                have.record.source
            )));
        }
        let record = Record {
            source: origin,
            plugins: Vec::new(),
        };
        save(stage, &record)?;
        std::fs::rename(stage, home.join(&name))?;
        Ok(format!(
            "added marketplace {name} ({} plugins); install one with `summonr plugin install PLUGIN@{name}`",
            catalog.plugins.len()
        ))
    }

    /// Lists added marketplaces, one line each, or a JSON array.
    pub fn list(json: bool) -> Result<String> {
        let all = Self::all()?;
        if json {
            let rows: Vec<serde_json::Value> = all.iter().map(Self::row).collect();
            return Ok(serde_json::to_string_pretty(&rows)?);
        }
        if all.is_empty() {
            return Ok("no marketplaces added".to_string());
        }
        Ok(all
            .iter()
            .map(|market| {
                let plugins = market.catalog().map_or_else(
                    |error| format!("unreadable: {error}"),
                    |catalog| format!("{} plugins", catalog.plugins.len()),
                );
                format!(
                    "{}\t{}\t{}\t{plugins}",
                    market.name,
                    market.record.source.kind(),
                    market.record.source
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn row(&self) -> serde_json::Value {
        let source = &self.record.source;
        let mut row = json!({
            "name": self.name,
            "source": source.kind(),
            "installLocation": self.root(),
        });
        let (key, place) = match source {
            Origin::Github { repo, .. } => ("repo", repo.clone()),
            Origin::Git { url, .. } | Origin::Url { url } => ("url", url.clone()),
            Origin::Directory { path } | Origin::File { path } => {
                ("path", path.display().to_string())
            }
        };
        row[key] = json!(place);
        if let Some(reference) = source.reference() {
            row["ref"] = json!(reference);
        }
        row
    }

    /// Removes a marketplace and every plugin installed from it.
    pub fn remove(name: &str) -> Result<String> {
        let home = home()?;
        let _lock = hold(&home)?;
        let market = Self::open(name)?;
        let mut removed = Vec::new();
        for plugin in &market.record.plugins {
            if Plugin::remove(plugin)? {
                removed.push(plugin.as_str());
            }
        }
        std::fs::remove_dir_all(&market.dir)?;
        Ok(if removed.is_empty() {
            format!("removed marketplace {name}")
        } else {
            format!(
                "removed marketplace {name} and its plugins: {}; restart summonr to unload them",
                removed.join(", ")
            )
        })
    }

    /// Fetches one marketplace again, or every one when `name` is `None`.
    pub async fn update(name: Option<&str>) -> Result<String> {
        let home = home()?;
        let _lock = hold(&home)?;
        let markets = match name {
            Some(name) => vec![Self::open(name)?],
            None => Self::all()?,
        };
        for market in &markets {
            market.refresh().await?;
        }
        Ok(match name {
            Some(name) => format!("updated marketplace {name}"),
            None => format!("updated {} marketplaces", markets.len()),
        })
    }

    async fn refresh(&self) -> Result<()> {
        let source = &self.record.source;
        if !source.fetched() {
            return self.catalog().map(drop);
        }
        let fresh = self.dir.join(FRESH);
        clear(&fresh)?;
        let fetched = self.fetch_fresh(&fresh).await;
        if fetched.is_err() {
            clear(&fresh)?;
        }
        fetched?;
        let (current, previous) = (self.dir.join(REPO), self.dir.join(PREVIOUS));
        clear(&previous)?;
        if current.exists() {
            std::fs::rename(&current, &previous)?;
        }
        std::fs::rename(&fresh, &current)?;
        clear(&previous)
    }

    async fn fetch_fresh(&self, fresh: &Path) -> Result<()> {
        let source = &self.record.source;
        source.fetch(fresh).await?;
        let catalog = Catalog::read(&source.catalog(fresh))?;
        if catalog.name != self.name {
            return Err(Error::Config(format!(
                "marketplace `{}` now names itself `{}`; remove it and add it again",
                self.name, catalog.name
            )));
        }
        Ok(())
    }

    /// Installs `PLUGIN@MARKETPLACE`: fetches the entry's source, then installs it like a path.
    pub fn install(id: &str) -> Result<Plugin> {
        let Some((plugin, market)) = id.rsplit_once('@').filter(|(plugin, _)| id_part(plugin))
        else {
            return Err(Error::Config(format!(
                "`{id}` is not a plugin id; write PLUGIN@MARKETPLACE"
            )));
        };
        let home = home()?;
        let _lock = hold(&home)?;
        let mut market = Self::open(market)?;
        let catalog = market.catalog()?;
        let entry = catalog.entry(plugin)?;
        let work = market.dir.join(FETCH).join(&entry.name);
        clear(&work)?;
        let installed = market
            .fetch(&catalog, &entry, &work)
            .and_then(|dir| merge::write(&entry, &dir).map(|()| dir))
            .and_then(|dir| Plugin::install(&dir));
        clear(&work)?;
        let installed = installed?;
        for mut other in Self::all()? {
            if other.name != market.name && other.record.plugins.contains(&installed.name) {
                other.record.plugins.retain(|name| name != &installed.name);
                save(&other.dir, &other.record)?;
            }
        }
        if !market.record.plugins.contains(&installed.name) {
            market.record.plugins.push(installed.name.clone());
            save(&market.dir, &market.record)?;
        }
        Ok(installed)
    }

    fn fetch(&self, catalog: &Catalog, entry: &Entry, work: &Path) -> Result<PathBuf> {
        let dir = match &entry.source {
            Source::Relative(written) => {
                if !self.record.source.has_files() {
                    return Err(Error::Config(format!(
                        "plugin `{}`: marketplace `{}` is a single file from a URL, so the relative source `{written}` cannot resolve",
                        entry.name, self.name
                    )));
                }
                let from = catalog.relative(written, &self.root())?;
                fetch::contained(&from)?;
                let dir = work.join(&entry.name);
                copy_tree(&from, &dir)?;
                dir
            }
            Source::Remote(remote) => remote.fetch(&entry.name, work)?,
        };
        fetch::contained(&dir)?;
        Ok(dir)
    }

    fn open(name: &str) -> Result<Self> {
        let missing = || {
            Error::Config(format!(
                "no marketplace named `{name}`; `summonr plugin marketplace list` lists them"
            ))
        };
        if !id_part(name) {
            return Err(missing());
        }
        let dir = home()?.join(name);
        let record = super::read_parsed(&dir.join(RECORD), |text: &str| {
            serde_json::from_str::<Record>(text)
        })?
        .ok_or_else(missing)?;
        Ok(Self {
            name: name.to_string(),
            dir,
            record,
        })
    }

    fn all() -> Result<Vec<Self>> {
        let mut all: Vec<Self> = summonr_core::walk::children(&home()?)
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| id_part(name))
            .filter_map(|name| Self::open(&name).ok())
            .collect();
        all.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(all)
    }

    fn root(&self) -> PathBuf {
        self.record.source.root(&self.dir)
    }

    fn catalog(&self) -> Result<Catalog> {
        Catalog::read(&self.record.source.catalog(&self.root()))
    }
}

fn home() -> Result<PathBuf> {
    Ok(summonr_home()?.join("marketplaces"))
}

fn hold(home: &Path) -> Result<std::fs::File> {
    crate::config::hold(&home.join(".lock"))
}

fn save(dir: &Path, record: &Record) -> Result<()> {
    store::write(
        &dir.join(RECORD),
        serde_json::to_vec_pretty(record)?,
        Durability::Renamed,
    )?;
    Ok(())
}

fn clear(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}
