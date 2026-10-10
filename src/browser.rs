use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;
use url::Url;

use crate::error::{Error, Result};

mod call;
mod extras;
mod launch;
mod report;
pub(crate) mod wall;

use call::{Call, Extra};

const DEFAULT_PROFILE: &str = "default";
const BROWSER_DIR: &str = "browser";
const PROFILES_DIR: &str = "profiles";
const SHOTS_DIR: &str = "shots";
const RENDERED_NOTE: &str = "[rendered in the browser because the page needs JavaScript]";

pub(crate) struct Live {
    session: charm::Session,
    headed: bool,
}

pub(crate) struct Settings {
    name: String,
    profile: PathBuf,
    headed: bool,
    chrome: Option<PathBuf>,
    events: charm::Events,
    timing: charm::Timing,
    out_max: usize,
}

#[derive(Clone)]
pub(crate) struct Browser {
    live: Arc<Mutex<Option<Live>>>,
    settings: Arc<Settings>,
}

impl Browser {
    pub(crate) fn discover() -> Result<Self> {
        let file = crate::config::file()?;
        let raw = &file.browser;
        let name = raw.profile.as_deref().unwrap_or(DEFAULT_PROFILE).trim();
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(Error::Config(format!(
                "[browser] profile `{name}` must be a plain directory name"
            )));
        }
        let profile = crate::prompt::summonr_home()?
            .join(BROWSER_DIR)
            .join(PROFILES_DIR)
            .join(name);
        let events = raw
            .events
            .map_or_else(charm::Events::all, |events| charm::Events {
                page: events.page,
                runtime: events.runtime,
            });
        Ok(Self {
            live: Arc::new(Mutex::new(None)),
            settings: Arc::new(Settings {
                name: name.to_string(),
                profile,
                headed: raw.headed.unwrap_or(false),
                chrome: raw.chrome.clone(),
                events,
                timing: launch::timing(),
                out_max: crate::web::OUT_MAX,
            }),
        })
    }

    pub(crate) async fn call(
        &self,
        arguments: &str,
        workspace: &Path,
        session: Option<&str>,
    ) -> Result<String> {
        let call = call::parse(arguments)?;
        let mut live = self.live.lock().await;
        let out = match call {
            Call::Extra(Extra::Close) => launch::close(&mut live).await,
            Call::Extra(Extra::Login { url }) => self.login(&mut live, &url).await,
            Call::Extra(extra) => {
                let shots = shots_dir(session)?;
                let out_max = self.settings.out_max;
                let page = self.ready(&mut live).await?;
                extras::run(page, extra, &shots, out_max).await
            }
            Call::Act(action, report) => {
                let action = call::guard(action, workspace).await?;
                let out_max = self.settings.out_max;
                let page = self.ready(&mut live).await?;
                extras::act(page, action, report, out_max).await
            }
        };
        if let Some(held) = live.as_mut() {
            held.session.keep_cookies().await.map_err(fail)?;
        }
        out
    }

    pub(crate) async fn render(&self, url: &Url) -> Result<String> {
        let mut live = self.live.lock().await;
        let out_max = self.settings.out_max;
        let page = self.ready(&mut live).await?;
        page.goto(url.as_str()).await.map_err(fail)?;
        if let Some(blocked) = extras::guard(page).await? {
            return Err(Error::Tool(blocked));
        }
        let text = page
            .text(charm::TextFormat::Markdown, out_max)
            .await
            .map_err(fail)?;
        Ok(format!("{RENDERED_NOTE}\n{text}"))
    }

    async fn login(&self, live: &mut Option<Live>, raw: &str) -> Result<String> {
        let url = crate::web::allowed_url(raw).await?;
        if live.as_ref().is_some_and(|held| !held.headed) {
            launch::close(live).await?;
        }
        if live.is_none() {
            *live = Some(self.open(true).await?);
        }
        let page = self.ready(live).await?;
        page.goto(&url).await.map_err(fail)?;
        Ok(format!(
            "A Chrome window is open at {url}. Ask the user to log in there and to say when they are done, then call browser again. The login stays in browser profile `{}`.",
            self.settings.name
        ))
    }
}

fn shots_dir(session: Option<&str>) -> Result<PathBuf> {
    let home = crate::prompt::summonr_home()?;
    Ok(match session {
        Some(id) => crate::durable::log::sessions_dir(&home).join(id),
        None => home.join(BROWSER_DIR).join(SHOTS_DIR),
    })
}

pub(crate) fn fail(error: charm::Error) -> Error {
    Error::Tool(format!("browser: {error}"))
}
