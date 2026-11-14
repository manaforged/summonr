use super::preset::{preset, presets};

/// A coding agent Summonr runs through that agent's own program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Friend {
    /// The preset that runs turns through it.
    pub route: &'static str,
    /// The product name, such as "Claude Code".
    pub name: &'static str,
    /// The model the route starts on.
    pub model: &'static str,
    /// Who makes its models, such as "anthropic".
    pub provider: &'static str,
    /// The icon its chats show, as rows of block characters.
    pub mark: &'static [String],
    /// The icon's color as red, green and blue.
    pub color: [u8; 3],
    /// The command that signs in through the agent's own flow.
    pub login: &'static str,
    /// The vendor's install command for this platform.
    pub install: &'static str,
}

impl Friend {
    /// The friend behind `route`.
    #[must_use]
    pub fn of(route: &str) -> Option<Self> {
        friends().find(|friend| friend.route == route)
    }

    /// The models it can run: those it announced, else the provider's current ones.
    #[must_use]
    pub fn models(&self) -> Vec<String> {
        if let Some(listing) = crate::catalog::listing(self.route) {
            return listing.models.into_iter().map(|model| model.id).collect();
        }
        let offered = crate::catalog::offered(self.route, crate::catalog::Cost::Plan);
        let rows: Vec<_> = offered.iter().map(|model| (self.route, model)).collect();
        crate::catalog::current(&rows)
            .into_iter()
            .zip(&offered)
            .filter(|(current, _)| *current)
            .map(|(_, model)| model.id.clone())
            .collect()
    }

    /// Whether its program is on this machine.
    #[must_use]
    pub fn installed(&self) -> bool {
        preset(self.route)
            .and_then(|row| row.found.as_ref())
            .is_some_and(|found| found.probe.here())
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
            model: &row.model,
            provider: &row.provider,
            mark: &agent.mark,
            color: agent.color,
            login: &agent.login,
            install: if cfg!(windows) {
                &agent.install_windows
            } else {
                &agent.install
            },
        })
    })
}
