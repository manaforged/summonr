use sigil::prelude::{NodeBuilder, button, derived};

use super::super::App;
use super::super::style::{Text, text};
use super::parts::{actions, kept_note, row, section, switch};
use summonr_agent::modules::{ALL, Module, ModuleId};

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    let builtin = ALL
        .iter()
        .filter(|module| module.id != ModuleId::Browser)
        .map(|module| module_switch(app, module))
        .collect();
    vec![
        kept_note(),
        section(Some("Built-in tools"), builtin),
        browser(app),
    ]
}

fn module_switch(app: &App, module: &Module) -> NodeBuilder {
    let (label, about) = describe(module.id);
    let on = app
        .modules
        .peek()
        .get(&module.id)
        .copied()
        .unwrap_or(module.default);
    let id = module.id;
    let flip = app.clone();
    switch(label, Some(about), on, move |now| {
        let current = flip
            .modules
            .peek()
            .get(&id)
            .copied()
            .unwrap_or(ALL[id as usize].default);
        if current != now {
            flip.toggle_module(id);
        }
    })
}

fn browser(app: &App) -> NodeBuilder {
    let model = app.model;
    let program = derived(move || {
        drop(model.get());
        summonr_agent::browser::chrome().map_or_else(
            || "Not found".to_owned(),
            |path| summonr_agent::browser::display_name(&path),
        )
    });
    let choose = app.clone();
    let control = actions()
        .child(text(Text::Label, program))
        .child(button("Choose…").activate(move || choose.choose_chrome()));
    section(
        Some("Browser"),
        vec![
            module_switch(app, &ALL[ModuleId::Browser as usize]),
            row(
                "Browser app",
                Some("The Chrome-style browser agents control"),
                control,
            ),
        ],
    )
}

const fn describe(id: ModuleId) -> (&'static str, &'static str) {
    match id {
        ModuleId::Search => ("Web search", "Search the web and read pages"),
        ModuleId::Lsp => (
            "Code intelligence",
            "Find errors and definitions in your code",
        ),
        ModuleId::Jobs => (
            "Background commands",
            "Run long commands while the agent keeps working",
        ),
        ModuleId::Task => (
            "Helper agents",
            "Let an agent hand parts of the work to helpers",
        ),
        ModuleId::Skills => ("Skills", "Use your installed skills when they fit"),
        ModuleId::Mcp => ("MCP tools", "Use tools from your MCP servers"),
        ModuleId::Mail => ("Agent mail", "Let agents send messages to each other"),
        ModuleId::Browser => (
            "Browser control",
            "Let agents click, type and read in a browser",
        ),
        ModuleId::Memory => ("Memory", "Load your memory notes into every agent"),
    }
}
