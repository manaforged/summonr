use sigil::prelude::{NodeBuilder, Tone, notice_band};

use super::super::parts::{section, select};
use super::{Page, first_line};
use summonr_agent::workflow::{self, ModelSelection, Role, Workflow};

const ROLES: [(Role, &str); 5] = [
    (Role::Scout, "Scout"),
    (Role::Implement, "Implement"),
    (Role::Judge, "Judge"),
    (Role::Summary, "Summary"),
    (Role::Title, "Title"),
];
const PRIMARY: &str = "Primary model";
const ENDPOINT_MODEL: &str = "Default model";
const MODEL: &str = "Model";

pub(super) fn editor(page: Page, name: &str) -> NodeBuilder {
    let saved = match workflow::load(name) {
        Ok(saved) => saved,
        Err(error) => return notice_band(first_line(&error.to_string()), Tone::Error),
    };
    let accounts = accounts();
    let mut rows = Vec::new();
    for (role, label) in ROLES {
        let chosen = saved.selection(role).cloned();
        rows.push(endpoint_row(page, &saved, role, label, &accounts));
        if let Some(chosen) = chosen {
            rows.push(model_row(page, &saved, role, &chosen));
        }
    }
    section(Some(name), rows)
}

fn endpoint_row(
    page: Page,
    saved: &Workflow,
    role: Role,
    label: &str,
    accounts: &[String],
) -> NodeBuilder {
    let mut choices = accounts.to_vec();
    let chosen = saved.selection(role).map(|chosen| chosen.endpoint.clone());
    if let Some(chosen) = chosen.as_ref().filter(|chosen| !choices.contains(chosen)) {
        choices.push(chosen.clone());
    }
    let at = chosen
        .and_then(|chosen| choices.iter().position(|name| *name == chosen))
        .map_or(0, |at| at + 1);
    let options = std::iter::once(PRIMARY.to_owned())
        .chain(choices.iter().map(|name| account(name)))
        .collect();
    let name = saved.name.clone();
    select(label, None, options, at, move |index| {
        let selection = index
            .checked_sub(1)
            .and_then(|at| choices.get(at))
            .map(|endpoint| ModelSelection {
                endpoint: endpoint.clone(),
                model: None,
            });
        page.wrote(assign(&name, role, selection));
    })
}

fn model_row(page: Page, saved: &Workflow, role: Role, chosen: &ModelSelection) -> NodeBuilder {
    let mut models = models(&chosen.endpoint);
    if let Some(model) = chosen
        .model
        .as_ref()
        .filter(|model| !models.contains(model))
    {
        models.push(model.clone());
    }
    let at = chosen
        .model
        .as_ref()
        .and_then(|model| models.iter().position(|seen| seen == model))
        .map_or(0, |at| at + 1);
    let options = std::iter::once(ENDPOINT_MODEL.to_owned())
        .chain(models.iter().cloned())
        .collect();
    let (name, endpoint) = (saved.name.clone(), chosen.endpoint.clone());
    select(MODEL, None, options, at, move |index| {
        let selection = ModelSelection {
            endpoint: endpoint.clone(),
            model: index.checked_sub(1).and_then(|at| models.get(at)).cloned(),
        };
        page.wrote(assign(&name, role, Some(selection)));
    })
}

fn assign(name: &str, role: Role, selection: Option<ModelSelection>) -> summonr_agent::Result<()> {
    let mut saved = workflow::load(name)?;
    saved.assign(role, selection);
    workflow::save(&saved)
}

fn accounts() -> Vec<String> {
    summonr_agent::catalog::cached()
        .keys()
        .filter(|name| {
            summonr_agent::endpoint::auth_info(name)
                .is_ok_and(|info| info.configured || info.keyless())
        })
        .cloned()
        .collect()
}

fn models(endpoint: &str) -> Vec<String> {
    summonr_agent::catalog::listing(endpoint)
        .map(|listing| {
            listing
                .models
                .into_iter()
                .filter(summonr_agent::catalog::Model::usable)
                .map(|model| model.id)
                .collect()
        })
        .unwrap_or_default()
}

fn account(endpoint: &str) -> String {
    summonr_agent::endpoint::label(endpoint).map_or_else(|| endpoint.to_owned(), str::to_owned)
}
