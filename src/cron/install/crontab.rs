use std::path::{Path, PathBuf};

use super::TickEnv;
use crate::error::Result;

const CRON_MARKER: &str = "# summonr-cron";
#[cfg(unix)]
const CRONTAB_PATH: &str = "crontab";

pub(crate) fn cron_line(exe: &Path, env: TickEnv<'_>) -> String {
    let vars: String = env
        .vars()
        .into_iter()
        .map(|(key, value)| format!("{key}={} ", crate::shell::quote(value)))
        .collect();
    let command = format!(
        "{vars}{} cron tick",
        crate::shell::quote(&exe.display().to_string())
    );
    format!("* * * * * {} {CRON_MARKER}", command.replace('%', "\\%"))
}

pub(crate) fn crontab_without(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_end().ends_with(CRON_MARKER))
        .map(|line| format!("{line}\n"))
        .collect()
}

#[cfg(unix)]
fn current() -> String {
    super::run("crontab", &["-l"], None).unwrap_or_default()
}

#[cfg(unix)]
fn has_line(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim_end().ends_with(CRON_MARKER))
}

#[cfg(unix)]
pub(crate) fn install(exe: &Path, env: TickEnv<'_>) -> Result<PathBuf> {
    let mut text = crontab_without(&current());
    text.push_str(&cron_line(exe, env));
    text.push('\n');
    super::run("crontab", &["-"], Some(&text))?;
    Ok(PathBuf::from(CRONTAB_PATH))
}

#[cfg(unix)]
pub(crate) fn uninstall() -> Result<Option<PathBuf>> {
    let text = current();
    if !has_line(&text) {
        return Ok(None);
    }
    super::run("crontab", &["-"], Some(&crontab_without(&text)))?;
    Ok(Some(PathBuf::from(CRONTAB_PATH)))
}

#[cfg(unix)]
pub(crate) fn status() -> Option<PathBuf> {
    has_line(&current()).then(|| PathBuf::from(CRONTAB_PATH))
}
