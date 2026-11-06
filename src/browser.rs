use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;
use url::Url;

use summonr_core::error::{Error, Result};

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
    profile: Option<PathBuf>,
    headed: bool,
    chrome: Option<PathBuf>,
    events: charm::Events,
    timing: charm::Timing,
    out_max: usize,
}

#[derive(Clone, Copy)]
enum Profile {
    Named,
    Ephemeral,
}

#[derive(Clone)]
pub(crate) struct Browser {
    live: Arc<Mutex<Option<Live>>>,
    settings: Arc<Settings>,
}

impl Browser {
    pub(crate) fn discover() -> Result<Self> {
        Self::open_with(Profile::Named)
    }

    pub(crate) fn ephemeral() -> Result<Self> {
        Self::open_with(Profile::Ephemeral)
    }

    fn open_with(profile: Profile) -> Result<Self> {
        let raw = &crate::config::file()?.browser;
        let (name, profile, headed) = match profile {
            Profile::Ephemeral => (String::new(), None, false),
            Profile::Named => {
                let name = raw.profile.as_deref().unwrap_or(DEFAULT_PROFILE).trim();
                if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
                    return Err(Error::Config(format!(
                        "[browser] profile `{name}` must be a plain directory name"
                    )));
                }
                let path = summonr_core::dirs::summonr_home()?
                    .join(BROWSER_DIR)
                    .join(PROFILES_DIR)
                    .join(name);
                (name.to_string(), Some(path), raw.headed.unwrap_or(false))
            }
        };
        let events = raw
            .events
            .map_or_else(charm::Events::all, |events| charm::Events {
                page: events.page,
                runtime: events.runtime,
            });
        Ok(Self {
            live: Arc::new(Mutex::new(None)),
            settings: Arc::new(Settings {
                name,
                profile,
                headed,
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
        resolve: &(dyn Fn(&str) -> Result<PathBuf> + Send + Sync),
        session: Option<&str>,
    ) -> Result<String> {
        let call = match call::parse(arguments)? {
            Call::Act(action, report) => Call::Act(call::guard(action, resolve).await?, report),
            call => call,
        };
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
                let out_max = self.settings.out_max;
                let page = self.ready(&mut live).await?;
                extras::act(page, action, report, out_max).await
            }
        };
        let kept = match live.as_mut() {
            Some(held) => held.session.keep_cookies().await.map_err(fail),
            None => Ok(()),
        };
        let out = out.and_then(|text| kept.map(|()| text));
        if out.is_err() {
            launch::recover(&mut live).await;
        }
        out
    }

    pub(crate) async fn render(&self, url: &Url, format: crate::web::Format) -> Result<String> {
        let mut live = self.live.lock().await;
        let out = self.rendered(&mut live, url, format).await;
        if out.is_err() {
            launch::recover(&mut live).await;
        }
        out
    }

    async fn rendered(
        &self,
        live: &mut Option<Live>,
        url: &Url,
        format: crate::web::Format,
    ) -> Result<String> {
        let out_max = self.settings.out_max;
        let page = self.ready(live).await?;
        page.goto(url.as_str()).await.map_err(fail)?;
        if let Some(blocked) = extras::guard(page).await? {
            return Err(Error::Tool(blocked));
        }
        let text = match format {
            crate::web::Format::Markdown => page.text(charm::TextFormat::Markdown, out_max).await,
            crate::web::Format::Text => page.text(charm::TextFormat::Text, out_max).await,
            crate::web::Format::Html => page.content().await,
        }
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
    let home = summonr_core::dirs::summonr_home()?;
    Ok(match session {
        Some(id) => crate::durable::log::sessions_dir(&home).join(id),
        None => home.join(BROWSER_DIR).join(SHOTS_DIR),
    })
}

pub(crate) fn fail(error: charm::Error) -> Error {
    Error::Tool(format!("browser: {error}"))
}

/// The Chrome the browser tools launch: the configured one, else the first install found.
pub(crate) fn chrome() -> Option<PathBuf> {
    crate::config::file()
        .ok()
        .and_then(|file| file.browser.chrome.clone())
        .or_else(charm::resolve)
}

/// The program inside a picked macOS app bundle, or the picked file itself.
pub(crate) fn executable(picked: &Path) -> PathBuf {
    if picked.extension().is_some_and(|ext| ext == "app") {
        let programs = picked.join("Contents/MacOS");
        let named = picked.file_stem().map(|stem| programs.join(stem));
        if let Some(named) = named.filter(|path| path.is_file()) {
            return named;
        }
        if let Some(first) = std::fs::read_dir(&programs)
            .ok()
            .and_then(|entries| entries.flatten().map(|entry| entry.path()).find(|path| path.is_file()))
        {
            return first;
        }
    }
    picked.to_path_buf()
}

/// A short name for a browser program, such as `Google Chrome`.
pub(crate) fn display_name(program: &Path) -> String {
    program
        .ancestors()
        .find(|part| part.extension().is_some_and(|ext| ext == "app"))
        .unwrap_or(program)
        .file_stem()
        .map_or_else(|| program.display().to_string(), |stem| stem.to_string_lossy().into_owned())
}
