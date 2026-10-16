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
            (
                "animations",
                before.animations != prefs.animations,
                value(prefs.animations),
            ),
            ("fps", before.fps != prefs.fps, value(i64::from(prefs.fps))),
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
        for key in ["plan_mode", "ban_comments", "ban_tests", "last_endpoint"] {
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
        for (role, seat) in &workflow.seats {
            table[role.name()] = match &seat.model {
                None => value(&seat.endpoint),
                Some(model) => {
                    let mut row = toml_edit::InlineTable::new();
                    row.insert("endpoint", seat.endpoint.as_str().into());
                    row.insert("model", model.as_str().into());
                    toml_edit::Item::Value(toml_edit::Value::InlineTable(row))
                }
            };
        }
        document["workflows"][workflow.name.as_str()] = toml_edit::Item::Table(table);
        if let Some(parent) = document["workflows"].as_table_mut() {
            parent.set_implicit(true);
        }
        Ok(true)
    })
    .map(drop)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Mcp,
    Endpoint,
}

impl Section {
    const fn key(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Endpoint => "endpoint",
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
        if !document.get(key).is_some_and(toml_edit::Item::is_table) {
            let mut tables = toml_edit::Table::new();
            tables.set_implicit(true);
            document[key] = toml_edit::Item::Table(tables);
        }
        document[key][name] = table;
        Ok(true)
    })
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
        for (key, setting) in fields {
            document["provider"][name][*key] = value(*setting);
        }
        if let Some(table) = document["provider"].as_table_mut() {
            table.set_implicit(true);
        }
        Ok(true)
    })
    .map(drop)
}

pub(crate) fn save_module(path: &Path, id: crate::modules::ModuleId, on: bool) -> Result<()> {
    update(path, |document| {
        document["modules"][id.name()] = value(on);
        if let Some(table) = document["modules"].as_table_mut() {
            table.set_implicit(true);
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

pub(crate) fn save_default_seat(path: &Path, seat: Option<(&str, &str)>) -> Result<()> {
    update(path, |document| {
        match seat {
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
    let home = super::home_dir();
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(&home)
    {
        let rest = rest.to_string_lossy();
        if rest.is_empty() {
            return "~".to_string();
        }
        return format!("~/{rest}");
    }
    path.display().to_string()
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
