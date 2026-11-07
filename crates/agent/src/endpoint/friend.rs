use super::preset::{Found, presets};

/// A coding agent Summonr runs through that agent's own program.
#[derive(Clone, Copy, Debug)]
pub struct Friend {
    /// The preset that runs turns through it.
    pub route: &'static str,
    /// The product name, such as "Claude Code".
    pub name: &'static str,
    /// The short mark its chats show.
    pub mark: &'static str,
    /// The command that signs in through the agent's own flow.
    pub login: &'static str,
    /// The vendor's install command for this platform.
    pub install: &'static str,
    found: &'static Found,
}

impl Friend {
    /// The friend behind `route`.
    #[must_use]
    pub fn of(route: &str) -> Option<Self> {
        friends().find(|friend| friend.route == route)
    }

    /// Whether its program is on this machine.
    #[must_use]
    pub fn installed(&self) -> bool {
        self.found.probe.here()
    }

    /// Whether its program reports a sign-in that this route can use.
    pub async fn signed_in(&self) -> bool {
        let Ok(endpoint) = super::resolve(Some(self.route), None, super::Requirement::Config)
        else {
            return false;
        };
        crate::provider::Client::discover(&endpoint).await.is_ok()
    }
}

/// Every coding agent Summonr can summon.
pub fn friends() -> impl Iterator<Item = Friend> {
    presets().filter_map(|row| {
        let found = row.found.as_ref()?;
        let agent = found.agent.as_ref()?;
        Some(Friend {
            route: &row.name,
            name: &found.name,
            mark: &agent.mark,
            login: &agent.login,
            install: if cfg!(windows) {
                &agent.install_windows
            } else {
                &agent.install
            },
            found,
        })
    })
}
