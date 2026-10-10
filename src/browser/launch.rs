use std::path::Path;

use super::{Browser, Live, fail};
use crate::error::{Error, Result};

const ACTIVE_PORT: &str = "DevToolsActivePort";
#[cfg(unix)]
const PROFILE_MODE: u32 = 0o700;

pub(super) fn timing() -> charm::Timing {
    let limits = crate::limits::limits();
    charm::Timing {
        connect: limits.connect(),
        command: limits.web_fetch(),
        navigate: limits.web_fetch(),
        wait: limits.web_search(),
        ..charm::Timing::default()
    }
}

impl Browser {
    pub(super) async fn ready<'a>(
        &self,
        live: &'a mut Option<Live>,
    ) -> Result<&'a mut charm::Session> {
        if live.is_none() {
            *live = Some(self.open(self.settings.headed).await?);
        }
        live.as_mut()
            .map(|held| &mut held.session)
            .ok_or_else(|| Error::Tool("browser: Chrome is not open".to_string()))
    }

    pub(super) async fn open(&self, headed: bool) -> Result<Live> {
        let settings = &self.settings;
        profile_dir(&settings.profile)?;
        let base = match &settings.chrome {
            Some(path) => charm::Launch::new(path),
            None => charm::Launch::installed().map_err(fail)?,
        };
        let mut launch = base
            .profile(&settings.profile)
            .timing(settings.timing)
            .events(settings.events)
            .dialogs(charm::DialogPolicy::default());
        if headed {
            launch = launch.headed();
        }
        match charm::Session::launch(launch).await {
            Ok(session) => Ok(Live { session, headed }),
            Err(charm::Error::ProfileInUse { .. }) => self.attach().await,
            Err(error) => Err(fail(error)),
        }
    }

    async fn attach(&self) -> Result<Live> {
        let settings = &self.settings;
        let file = settings.profile.join(ACTIVE_PORT);
        let port = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| text.lines().next().and_then(|line| line.trim().parse().ok()))
            .ok_or_else(|| {
                Error::Tool(format!(
                    "browser: profile {} is open in another Chrome without remote debugging; close that Chrome and call again",
                    settings.profile.display()
                ))
            })?;
        let mut session = charm::Session::connect(charm::Endpoint::Port(port), settings.timing)
            .await
            .map_err(fail)?;
        session
            .set_events(settings.events, charm::DialogPolicy::default())
            .await
            .map_err(fail)?;
        Ok(Live {
            session,
            headed: true,
        })
    }
}

pub(super) async fn close(live: &mut Option<Live>) -> Result<String> {
    match live.take() {
        Some(held) => {
            held.session.close().await.map_err(fail)?;
            Ok("browser closed".to_string())
        }
        None => Ok("browser was not open".to_string()),
    }
}

fn profile_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(PROFILE_MODE))?;
    }
    Ok(())
}
