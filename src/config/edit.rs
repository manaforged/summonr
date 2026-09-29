use super::*;

pub(crate) fn save_prefs(path: &Path, prefs: &Prefs) -> Result<()> {
    update(path, |document| {
        document["theme"] = value(&prefs.theme);
        document["splash"] = value(prefs.splash);
        document["animations"] = value(prefs.animations);
        document["fps"] = value(i64::from(prefs.fps));
        document["reasoning"] = value(prefs.reasoning.name());
        document["receipts"] = value(prefs.receipts.name());
        document["tools"] = value(prefs.tools.name());
        document["diff_layout"] = value(prefs.diff_layout.name());
        document["mail"] = value(prefs.mail);
        document.as_table_mut().remove("bus");
        document["ban_comments"] = value(prefs.ban_comments);
        document["ban_tests"] = value(prefs.ban_tests);
        document["plan_mode"] = value(prefs.plan_mode);
        document["layout"] = value(&prefs.layout);
        let mut hotkeys = toml_edit::InlineTable::new();
        for (action, binding) in prefs.hotkeys.entries() {
            hotkeys.insert(action.name(), binding.into());
        }
        document["hotkeys"] = toml_edit::Item::Value(toml_edit::Value::InlineTable(hotkeys));
        match &prefs.last_endpoint {
            Some(name) => document["last_endpoint"] = value(name),
            None => {
                document.as_table_mut().remove("last_endpoint");
            }
        }
        Ok(true)
    })
    .map(drop)
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

pub(crate) fn save_module(path: &Path, name: &str, on: bool) -> Result<()> {
    update(path, |document| {
        document["modules"][name] = value(on);
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
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
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

pub(crate) fn sandbox_entry(path: &Path) -> String {
    let entry = tilde(path);
    if entry == "~" {
        "~/".to_string()
    } else {
        entry
    }
}

pub(crate) fn save_sandbox_write(path: &Path, dir: &str) -> Result<()> {
    let entry = if Path::new(dir).is_absolute() {
        sandbox_entry(Path::new(dir))
    } else {
        dir.to_string()
    };
    update(path, |document| {
        let existing = document
            .get("sandbox")
            .and_then(|sandbox| sandbox.get("write"));
        if existing.is_some_and(|item| !item.is_none() && item.as_array().is_none()) {
            return Err(Error::Config(
                "config.toml: [sandbox] write must be an array".to_string(),
            ));
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
        document["sandbox"]["write"] = toml_edit::Item::Value(toml_edit::Value::Array(array));
        if let Some(table) = document["sandbox"].as_table_mut() {
            table.set_implicit(true);
        }
        Ok(true)
    })
    .map(drop)
}
