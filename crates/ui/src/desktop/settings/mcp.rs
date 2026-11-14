use std::time::Duration;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, ControlSize, FontFamily, JustifyContent, NodeBuilder, Paint, Signal, Tone, Widget,
    button, chip, derived, empty_state, field, icon, interval, notice_band, signal, text_field,
};
use sigil::reactive::{defer, on_cleanup};

use super::super::App;
use super::super::style::{ICON, L, M, S, Text, XS, text};
use super::parts::{actions, confirm_remove, first_line, item, section};
use summonr_agent::hub::Cmd;
use summonr_agent::mcp::{McpState, McpTransport, ServerStatus};
use summonr_agent::supervise::ServerKind;

mod launch;

const POLL: Duration = Duration::from_secs(2);

#[derive(Clone, PartialEq)]
struct Server {
    name: String,
    transport: Option<McpTransport>,
    state: McpState,
    tools: usize,
    launch: Option<String>,
    detail: Option<String>,
}

impl From<&ServerStatus> for Server {
    fn from(status: &ServerStatus) -> Self {
        Self {
            name: status.name.clone(),
            transport: status.transport,
            state: status.state,
            tools: status.tools.len(),
            launch: status.launch.clone(),
            detail: status.detail.clone(),
        }
    }
}

#[derive(Clone, Copy)]
struct Page {
    adding: Signal<bool>,
    armed: Signal<Option<String>>,
    error: Signal<Option<String>>,
    name: Signal<String>,
    target: Signal<String>,
    extra: Signal<String>,
}

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    refresh(app);
    let poll = app.clone();
    let timer = interval(POLL, move || refresh(&poll));
    on_cleanup(move || timer.cancel());
    let model = app.model;
    let servers = derived(move || {
        model
            .get()
            .mcp
            .iter()
            .filter(|status| status.kind == ServerKind::Mcp)
            .map(Server::from)
            .collect::<Vec<_>>()
    });
    let page = Page {
        adding: signal(false),
        armed: signal(None),
        error: signal(None),
        name: signal(String::new()),
        target: signal(String::new()),
        extra: signal(String::new()),
    };
    let app = app.clone();
    vec![
        sigil::prelude::column()
            .fill_width()
            .gap(M)
            .align_items(AlignItems::Stretch)
            .children_reactive(move || {
                let notice = page
                    .error
                    .get()
                    .filter(|_| !page.adding.get())
                    .map(|error| notice_band(error, Tone::Error));
                notice
                    .into_iter()
                    .chain([body(&app, page, &servers.get())])
                    .collect::<Vec<_>>()
            }),
    ]
}

fn body(app: &App, page: Page, servers: &[Server]) -> NodeBuilder {
    let adding = page.adding.get();
    if servers.is_empty() && !adding {
        return section(None, vec![empty(page)]);
    }
    let armed = page.armed.get();
    let mut rows: Vec<NodeBuilder> = servers
        .iter()
        .map(|server| {
            server_row(
                app,
                page,
                server,
                armed.as_deref() == Some(server.name.as_str()),
            )
        })
        .collect();
    rows.push(if adding {
        form(app, page)
    } else {
        add_row(page)
    });
    section(None, rows)
}

fn empty(page: Page) -> NodeBuilder {
    empty_state("No MCP servers yet", "").child(add_button(page).primary())
}

fn add_row(page: Page) -> NodeBuilder {
    sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .align_items(sigil::prelude::AlignItems::Stretch)
        .fill_width()
        .padding_x(L)
        .padding_y(M)
        .child(add_button(page).ghost())
}

fn add_button(page: Page) -> sigil::widgets::ButtonBuilder {
    button("Add server").icon(Icon::Plus).activate(move || {
        page.error.set(None);
        defer(move || page.adding.set(true));
    })
}

fn server_row(app: &App, page: Page, server: &Server, armed: bool) -> NodeBuilder {
    let mut words = sigil::prelude::column()
        .gap(XS)
        .child(text(Text::Body, server.name.clone()))
        .child(text(Text::Meta, summary(server)));
    if let Some(launch) = &server.launch {
        words = words.child(text(Text::Meta, launch.clone()).font_family(FontFamily::Mono));
    }
    if armed {
        words = words.child(text(Text::Meta, "Agents lose its tools"));
    } else if let Some(detail) = server
        .detail
        .as_deref()
        .filter(|_| server.state == McpState::Failed)
    {
        words = words.child(text(Text::Meta, first_line(detail)));
    }
    let trailing = if armed {
        confirm(app, page, &server.name)
    } else {
        status(page, server)
    };
    item(icon(Icon::Server).px(ICON).into(), words, trailing)
}

fn status(page: Page, server: &Server) -> NodeBuilder {
    let (label, tone) = state_chip(server.state);
    let name = server.name.clone();
    actions().child(chip(label, tone).dot()).child(
        button("Remove")
            .ghost()
            .tone(Tone::Error)
            .size(ControlSize::Compact)
            .accessible(format!("Remove {}", server.name))
            .activate(move || {
                let name = name.clone();
                defer(move || page.armed.set(Some(name)));
            }),
    )
}

fn confirm(app: &App, page: Page, name: &str) -> NodeBuilder {
    let app = app.clone();
    let target = name.to_owned();
    confirm_remove(
        name,
        move || defer(move || page.armed.set(None)),
        move || {
            let (app, target) = (app.clone(), target.clone());
            defer(move || remove(&app, page, &target));
        },
    )
}

fn form(app: &App, page: Page) -> NodeBuilder {
    let submit = app.clone();
    let add = app.clone();
    let ready = move || !page.name.get().trim().is_empty() && !page.target.get().trim().is_empty();
    sigil::prelude::column()
        .fill_width()
        .align_items(AlignItems::Stretch)
        .gap(M)
        .padding_x(L)
        .padding_y(M)
        .child(field("Name", text_field(page.name).placeholder("github")))
        .child(field(
            "Command or URL",
            text_field(page.target)
                .placeholder("npx -y server-name")
                .submit(move |_| create(&submit, page)),
        ))
        .child(field(
            "Environment or headers",
            text_field(page.extra).placeholder("API_KEY=value, OTHER=value"),
        ))
        .child(
            sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .fill_width()
                .align_items(AlignItems::Stretch)
                .children_reactive(move || {
                    page.error
                        .get()
                        .map(|error| notice_band(error, Tone::Error))
                }),
        )
        .child(
            sigil::prelude::row()
                .align_items(sigil::prelude::AlignItems::Stretch)
                .fill_width()
                .gap(S)
                .justify_content(JustifyContent::End)
                .child(button("Cancel").ghost().activate(move || close(page)))
                .child(
                    button("Add server")
                        .primary()
                        .disabled(move || !ready())
                        .activate(move || create(&add, page)),
                ),
        )
}

fn create(app: &App, page: Page) {
    let name = page.name.peek().trim().to_owned();
    let target = page.target.peek().trim().to_owned();
    let extra = page.extra.peek();
    let added = launch::launch(&target, &extra).and_then(|launch| {
        summonr_agent::mcp::add(&name, &launch).map_err(|error| error.to_string())
    });
    match added {
        Ok(_) => {
            close(page);
            refresh(app);
        }
        Err(error) => page.error.set(Some(first_line(&error))),
    }
}

fn close(page: Page) {
    page.name.set(String::new());
    page.target.set(String::new());
    page.extra.set(String::new());
    page.error.set(None);
    defer(move || page.adding.set(false));
}

fn remove(app: &App, page: Page, name: &str) {
    page.armed.set(None);
    match summonr_agent::mcp::remove(name) {
        Ok(true) => page.error.set(None),
        Ok(false) => page.error.set(Some("A plugin adds this server".to_owned())),
        Err(error) => page.error.set(Some(first_line(&error.to_string()))),
    }
    refresh(app);
}

fn refresh(app: &App) {
    drop(app.cmd.send(Cmd::Mcp));
}

fn summary(server: &Server) -> String {
    let transport = server.transport.map_or("stdio", McpTransport::label);
    match server.tools {
        0 => transport.to_owned(),
        1 => format!("{transport} · 1 tool"),
        count => format!("{transport} · {count} tools"),
    }
}

const fn state_chip(state: McpState) -> (&'static str, Tone) {
    match state {
        McpState::Live => ("Connected", Tone::Success),
        McpState::Connecting => ("Starting", Tone::Info),
        McpState::Cached => ("Ready", Tone::Neutral),
        McpState::Unlisted => ("Not started", Tone::Neutral),
        McpState::Waiting => ("Not enabled", Tone::Warning),
        McpState::Stopped => ("Stopped", Tone::Neutral),
        McpState::Failed => ("Failed", Tone::Error),
    }
}
