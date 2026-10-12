use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthInfo {
    pub name: String,
    pub flow: LoginFlow,
    pub login: LoginFlow,
    pub key: bool,
    pub plan: String,
    pub configured: bool,
    pub source: Option<Source>,
    pub setup: String,
    pub env: Option<&'static str>,
    pub model: String,
    pub base_url: Option<String>,
    pub own: bool,
    pub(super) env_hint: Option<&'static str>,
    pub(super) group: LoginGroup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum LoginGroup {
    Key,
    Plan,
    Decide,
    Local,
}

impl LoginGroup {
    const fn title(self) -> &'static str {
        match self {
            Self::Key => "API keys",
            Self::Plan => "Coding plans",
            Self::Decide => "Decisions  (summonr classify / Jev)",
            Self::Local => "Local  (no key)",
        }
    }
}

impl AuthInfo {
    #[must_use]
    pub fn dual(&self) -> bool {
        self.key && self.login.signs_in()
    }

    #[must_use]
    pub fn status(&self) -> String {
        format!("{}: {}; {}", self.name, self.state(), self.setup)
    }

    #[must_use]
    pub fn display_name(&self) -> String {
        match self.plan.as_str() {
            "" => self.name.clone(),
            plan => format!("{} · {plan}", self.name),
        }
    }

    fn listed(&self) -> LoginGroup {
        match self.group {
            LoginGroup::Key if !self.plan.is_empty() => LoginGroup::Plan,
            group => group,
        }
    }

    #[must_use]
    pub fn plan(&self) -> bool {
        self.group == LoginGroup::Plan
    }

    #[must_use]
    pub fn keyless(&self) -> bool {
        self.group == LoginGroup::Local
    }

    #[must_use]
    pub fn catalog_rank(&self) -> u8 {
        self.listed() as u8
    }

    #[must_use]
    pub fn catalog_title(&self) -> &'static str {
        self.listed().title()
    }

    #[must_use]
    pub fn catalog_line(&self, name: usize, state: usize) -> String {
        let url = self.base_url.as_deref().unwrap_or_default();
        let about = label(&self.name).map_or_else(
            || match self.model.as_str() {
                "" => url.to_string(),
                model => format!("{model} at {url}"),
            },
            str::to_string,
        );
        let hint = match (self.unmodeled(), self.configured, self.env_hint) {
            _ if self.dual() => format!(" · sign in with {} or paste a key", self.plan),
            (true, ..) => format!(" · summonr {} --model NAME", self.name),
            (false, false, Some(env)) if !self.flow.signs_in() => format!(" · or set {env}"),
            _ => String::new(),
        };
        format!(
            "{:<name$}  {:<state$}  {about}{hint}",
            self.name,
            self.state()
        )
    }

    #[must_use]
    pub fn unmodeled(&self) -> bool {
        self.keyless() && self.model.is_empty()
    }

    #[must_use]
    pub fn state(&self) -> String {
        if self.unmodeled() {
            return State::NeedsModel.to_string();
        }
        match (self.flow.signs_in(), self.configured, self.source) {
            (true, true, _) | (false, true, None) => State::Ready.to_string(),
            (true, false, _) => State::SignIn.to_string(),
            (false, true, Some(Source::Builtin)) => State::NoKey.to_string(),
            (false, true, Some(source)) => format!("{} · {}", State::Ready, source.label()),
            (false, false, _) if self.keyless() => State::NoKey.to_string(),
            (false, false, _) => State::NeedsKey.to_string(),
        }
    }

    #[must_use]
    pub fn stored(&self) -> bool {
        self.configured
            && (self.flow.signs_in()
                || matches!(
                    self.source,
                    Some(Source::Keychain | Source::File | Source::Config)
                ))
    }
}
