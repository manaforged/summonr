use sigil::prelude::{NodeBuilder, button};

use super::super::App;
use super::parts::{choice, row, section, switch};
use crate::prefs::Notify;

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    vec![agents(app), alerts(app)]
}

fn agents(app: &App) -> NodeBuilder {
    let model = app.model.get();
    let snapshot = &model.snapshot;
    let check = snapshot.verify.clone();
    let about = check.as_deref().map_or_else(
        || "Agents stop when they think they are done".to_owned(),
        |command| format!("Agents keep working until {command} passes"),
    );
    let verify = app.clone();
    let flow = app.clone();
    let mail = app.clone();
    let workflow = snapshot.workflow.clone().unwrap_or_else(|| "None".to_owned());
    section(
        Some("Agents"),
        vec![
            switch("Check before finishing", Some(&about), check.is_some(), move |_| verify.toggle_verify()),
            row(
                "Workflow",
                Some("A saved plan of steps agents follow"),
                button(workflow).activate(move || flow.open_workflow()).into(),
            ),
            switch("Wake on mail", Some("New mail starts the agent's next turn"), model.prefs.mail, move |_| mail.toggle_mail()),
        ],
    )
}

fn alerts(app: &App) -> NodeBuilder {
    let notify = app.model.get().prefs.notify;
    let at = match notify {
        Notify::Off => 0,
        Notify::Turn => 1,
        Notify::All => 2,
    };
    let set = app.clone();
    section(
        Some("Notifications"),
        vec![choice(
            "Notify me",
            Some("When Summonr shows a system notification"),
            &["Never", "When done", "Always"],
            at,
            move |index| {
                let value = [Notify::Off, Notify::Turn, Notify::All][index.min(2)];
                set.flip(|prefs| prefs.notify = value);
            },
        )],
    )
}
