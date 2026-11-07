use super::*;

pub(crate) fn save_prefs(path: &Path, before: &Prefs, prefs: &Prefs) -> Result<()> {
    update(path, |document| {
        let changed = [
            ("theme", before.theme != prefs.theme, value(&prefs.theme)),
            (
                "background",
                before.background != prefs.background,
                value(prefs.background.name()),
            ),
            (
                "notify",
                before.notify != prefs.notify,
                value(prefs.notify.name()),
            ),
            ("splash", before.splash != prefs.splash, value(prefs.splash)),
            ("will", before.will != prefs.will, value(prefs.will)),
            (
                "will_tint",
                before.will_tint != prefs.will_tint,
                value(prefs.will_tint.name()),
            ),
            (
                "will_spot",
                before.will_spot != prefs.will_spot,
                toml_edit::Item::Value(toml_edit::Value::Array(
                    prefs.will_spot.iter().map(|at| i64::from(*at)).collect(),
                )),
            ),
            (
                "animations",
                before.animations != prefs.animations,
                value(prefs.animations),
            ),
            (
                "spinner",
                before.spinner != prefs.spinner,
                value(prefs.spinner.name()),
            ),
            ("fps", before.fps != prefs.fps, value(i64::from(prefs.fps))),
            (
                "spacing",
                before.spacing != prefs.spacing,
                value(i64::from(prefs.spacing)),
            ),
            (
                "reasoning",
                before.reasoning != prefs.reasoning,
                value(prefs.reasoning.name()),
            ),
            (
                "receipts",
                before.receipts != prefs.receipts,
                value(prefs.receipts.name()),
            ),
            (
                "tools",
                before.tools != prefs.tools,
                value(prefs.tools.name()),
            ),
            (
                "diff_layout",
                before.diff_layout != prefs.diff_layout,
                value(prefs.diff_layout.name()),
            ),
            ("mail", before.mail != prefs.mail, value(prefs.mail)),
            (
                "layout",
                before.layout != prefs.layout,
                value(&prefs.layout),
            ),
            ("hotkeys", before.hotkeys != prefs.hotkeys, hotkeys(prefs)),
            ("sidebar", before.sidebar != prefs.sidebar, sidebar(prefs)),
        ];
        let mut wrote = false;
        for (key, differs, item) in changed {
            if differs {
                document[key] = item;
                wrote = true;
            }
        }
        if document.as_table_mut().remove("bus").is_some() {
            document["mail"] = value(prefs.mail);
            wrote = true;
        }
        for key in super::decode::RETIRED {
            wrote |= document.as_table_mut().remove(key).is_some();
        }
        Ok(wrote)
    })
    .map(drop)
}

fn hotkeys(prefs: &Prefs) -> toml_edit::Item {
    let mut hotkeys = toml_edit::InlineTable::new();
    for (action, binding) in prefs.hotkeys.entries() {
        hotkeys.insert(action.name(), binding.into());
    }
    toml_edit::Item::Value(toml_edit::Value::InlineTable(hotkeys))
}

fn sidebar(prefs: &Prefs) -> toml_edit::Item {
    let names = prefs.sidebar.iter().map(|action| action.name());
    toml_edit::Item::Value(toml_edit::Value::Array(names.collect()))
}

pub(crate) fn remove_key(path: &Path, name: &str) -> Result<bool> {
    update(path, |document| {
        Ok(document["endpoint"][name]
            .as_table_like_mut()
            .is_some_and(|row| row.remove("api_key").is_some()))
    })
}

pub(crate) fn save_workflow(path: &Path, workflow: &Workflow) -> Result<()> {
    update(path, |document| {
        let mut table = toml_edit::Table::new();
        for (role, selection) in &workflow.selections {
            table[role.name()] = match &selection.model {
                None => value(&selection.endpoint),
                Some(model) => {
                    let mut row = toml_edit::InlineTable::new();
                    row.insert("endpoint", selection.endpoint.as_str().into());
                    row.insert("model", model.as_str().into());
                    toml_edit::Item::Value(toml_edit::Value::InlineTable(row))
                }
            };
        }
        let Some(workflows) = tables_at(document, "workflows") else {
            return Ok(false);
        };
        workflows[workflow.name.as_str()] = toml_edit::Item::Table(table);
        Ok(true)
    })
    .map(drop)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Mcp,
    Endpoint,
    Workflow,
}

impl Section {
    const fn key(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Endpoint => "endpoint",
            Self::Workflow => "workflows",
        }
    }
}

pub(crate) fn save_named(
    path: &Path,
    section: Section,
    name: &str,
    table: toml_edit::Item,
) -> Result<bool> {
    let key = section.key();
    update(path, |document| {
        if document
            .get(key)
            .and_then(|tables| tables.get(name))
            .is_some_and(|existing| !existing.is_none())
        {
            return Ok(false);
        }
        let Some(tables) = tables_at(document, key) else {
            return Ok(false);
        };
        tables[name] = table;
        Ok(true)
    })
}

fn tables_at<'d>(document: &'d mut DocumentMut, key: &str) -> Option<&'d mut toml_edit::Table> {
    if !document.get(key).is_some_and(toml_edit::Item::is_table) {
        let table = document
            .get(key)
            .and_then(toml_edit::Item::as_inline_table)
            .map_or_else(
                || {
                    let mut table = toml_edit::Table::new();
                    table.set_implicit(true);
                    table
                },
                |inline| inline.clone().into_table(),
            );
        document[key] = toml_edit::Item::Table(table);
    }
    document[key].as_table_mut()
}

pub(crate) fn remove_named(path: &Path, section: Section, name: &str) -> Result<bool> {
    update(path, |document| {
        Ok(document[section.key()]
            .as_table_mut()
            .is_some_and(|tables| tables.remove(name).is_some()))
    })
}

pub(crate) fn save_provider(path: &Path, name: &str, fields: &[(&str, &str)]) -> Result<()> {
    update(path, |document| {
        let Some(providers) = tables_at(document, "provider") else {
            return Ok(false);
        };
        for (key, setting) in fields {
            providers[name][*key] = value(*setting);
        }
        Ok(true)
    })
    .map(drop)
}

pub(crate) fn save_module(
    path: &Path,
    id: summonr_types::modules::ModuleId,
    on: bool,
) -> Result<()> {
    update(path, |document| {
        let Some(modules) = tables_at(document, "modules") else {
            return Ok(false);
        };
        modules[id.name()] = value(on);
        Ok(true)
    })
    .map(drop)
}

pub(crate) fn save_browser_chrome(path: &Path, chrome: Option<&Path>) -> Result<()> {
    update(path, |document| {
        let Some(browser) = tables_at(document, "browser") else {
            return Ok(false);
        };
        match chrome {
            Some(chrome) => browser["chrome"] = value(chrome.display().to_string()),
            None => drop(browser.remove("chrome")),
        }
        Ok(true)
    })
    .map(drop)
}

pub(crate) fn save_default_workflow(path: &Path, name: &str) -> Result<()> {
    update(path, |document| {
        document["workflow"] = value(name);
        Ok(true)
    })
    .map(drop)
}

pub(crate) fn clear_default_workflow(path: &Path) -> Result<()> {
    update(path, |document| Ok(document.remove("workflow").is_some())).map(drop)
}

pub(crate) fn save_default_skills(path: &Path, name: Option<&str>) -> Result<()> {
    update(path, |document| {
        let Some(skills) = tables_at(document, "skills") else {
            return Ok(false);
        };
        match name {
            Some(name) => skills["default"] = value(name),
            None => drop(skills.remove("default")),
        }
        Ok(true)
    })
    .map(drop)
}

pub(crate) fn save_default_model(path: &Path, selection: Option<(&str, &str)>) -> Result<()> {
    update(path, |document| {
        match selection {
            Some((endpoint, model)) => {
                document["default"] = value(endpoint);
                document["default_model"] = value(model);
            }
            None => {
                document.as_table_mut().remove("default");
                document.as_table_mut().remove("default_model");
            }
        }
        Ok(true)
    })
    .map(drop)
}

pub fn tilde(path: &Path) -> String {
    under(path, summonr_core::dirs::home_dir().as_deref())
}

fn under(path: &Path, home: Option<&Path>) -> String {
    let resolved = home.and_then(|home| std::fs::canonicalize(home).ok());
    let rest = home
        .map(Path::to_path_buf)
        .into_iter()
        .chain(resolved)
        .find_map(|home| path.strip_prefix(&home).ok().map(Path::to_path_buf));
    match rest {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.to_string_lossy()),
        None => path.display().to_string(),
    }
}

const WRITE_ROOTS: &str = "write_roots";

pub(crate) fn write_root_entry(path: &Path) -> String {
    let entry = tilde(path);
    if entry == "~" {
        "~/".to_string()
    } else {
        entry
    }
}

pub(crate) fn save_write_root(path: &Path, dir: &str) -> Result<()> {
    let entry = if Path::new(dir).is_absolute() {
        write_root_entry(Path::new(dir))
    } else {
        dir.to_string()
    };
    update(path, |document| {
        let existing = document.get(WRITE_ROOTS);
        if existing.is_some_and(|item| !item.is_none() && item.as_array().is_none()) {
            return Err(Error::Config(format!(
                "config.toml: {WRITE_ROOTS} must be an array"
            )));
        }
        let mut array = existing
            .and_then(toml_edit::Item::as_array)
            .cloned()
            .unwrap_or_else(toml_edit::Array::new);
        if !array
            .iter()
            .any(|value| value.as_str() == Some(entry.as_str()))
        {
            array.push(entry.as_str());
        }
        document[WRITE_ROOTS] = toml_edit::Item::Value(toml_edit::Value::Array(array));
        Ok(true)
    })
    .map(drop)
}

pub(crate) fn add_plugin_off(path: &Path, name: &str) -> Result<bool> {
    update(path, |document| {
        let not_a_list = || Error::Config("`plugins.off` in config.toml is not a list".to_owned());
        let plugins = document
            .entry("plugins")
            .or_insert_with(toml_edit::table)
            .as_table_mut()
            .ok_or_else(not_a_list)?;
        let off = plugins
            .entry("off")
            .or_insert_with(|| toml_edit::value(toml_edit::Array::new()))
            .as_array_mut()
            .ok_or_else(not_a_list)?;
        if off.iter().any(|value| value.as_str() == Some(name)) {
            return Ok(false);
        }
        off.push(name);
        Ok(true)
    })
}

pub(crate) fn drop_plugin_off(path: &Path, name: &str) -> Result<bool> {
    update(path, |document| {
        let Some(array) = document
            .get_mut("plugins")
            .and_then(|item| item.get_mut("off"))
            .and_then(toml_edit::Item::as_array_mut)
        else {
            return Ok(false);
        };
        let before = array.len();
        array.retain(|value| value.as_str() != Some(name));
        Ok(array.len() != before)
    })
}

#[cfg(test)]
#[path = "tilde_tests.rs"]
mod tilde_tests;
