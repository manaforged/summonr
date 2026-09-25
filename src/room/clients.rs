pub(super) struct Client {
    pub(super) pattern: &'static str,
    pub(super) kind: &'static str,
    pub(super) push: bool,
}

const CLIENTS: &[Client] = &[
    Client {
        pattern: "claude",
        kind: "claude",
        push: true,
    },
    Client {
        pattern: "codex",
        kind: "codex",
        push: false,
    },
    Client {
        pattern: "summoner",
        kind: "summoner",
        push: false,
    },
    Client {
        pattern: "summonr",
        kind: "summonr",
        push: false,
    },
];

pub(super) fn client(name: &str) -> Option<&'static Client> {
    CLIENTS.iter().find(|client| name.contains(client.pattern))
}
