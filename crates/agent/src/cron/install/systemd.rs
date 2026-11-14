use std::path::{Path, PathBuf};

use summonr_core::error::Result;

use super::{Installed, OwnedEnv, Saved, crontab, os_home, rolled_back, run, uid, units};

const SYSTEMD_DIR: &str = ".config/systemd/user";
const SERVICE_FILE: &str = "summonr-cron.service";
const TIMER_FILE: &str = "summonr-cron.timer";
const LINGER_OFF: &str = "no";
const LINGER_NOTE: &str = "jobs run only while you are logged in; `loginctl enable-linger` keeps them running after logout";

fn unit_paths() -> Result<(PathBuf, PathBuf)> {
    let dir = os_home()?.join(SYSTEMD_DIR);
    Ok((dir.join(SERVICE_FILE), dir.join(TIMER_FILE)))
}

fn linger_note() -> Option<&'static str> {
    let user = uid().to_string();
    let linger = run(
        "loginctl",
        &["show-user", &user, "--property=Linger", "--value"],
        None,
    )
    .ok()?;
    (linger.trim() == LINGER_OFF).then_some(LINGER_NOTE)
}

pub(crate) fn install(exe: &Path, every_secs: u64) -> Result<Installed> {
    let env = OwnedEnv::current();
    if run("systemctl", &["--user", "show-environment"], None).is_ok() {
        let (service_path, timer_path) = unit_paths()?;
        if let Some(dir) = timer_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let saved = [Saved::take(&service_path)?, Saved::take(&timer_path)?];
        let (service, timer) = units(exe, every_secs, env.view());
        if let Err(error) = load_units(&service_path, &service, &timer_path, &timer) {
            let mut notes = Vec::new();
            if saved[1].before.is_none()
                && let Err(disable) = run(
                    "systemctl",
                    &["--user", "disable", "--now", TIMER_FILE],
                    None,
                )
            {
                notes.push(format!("cannot disable {TIMER_FILE}: {disable}"));
            }
            let error = rolled_back(error, &saved, notes);
            drop(run("systemctl", &["--user", "daemon-reload"], None));
            return Err(error);
        }
        return Ok((timer_path, linger_note()));
    }
    crontab::install(exe, env.view()).map(|path| (path, None))
}

fn load_units(service_path: &Path, service: &str, timer_path: &Path, timer: &str) -> Result<()> {
    crate::durable::store::write(
        service_path,
        service,
        crate::durable::store::Durability::Synced,
    )?;
    crate::durable::store::write(timer_path, timer, crate::durable::store::Durability::Synced)?;
    run("systemctl", &["--user", "daemon-reload"], None)?;
    run(
        "systemctl",
        &["--user", "enable", "--now", TIMER_FILE],
        None,
    )?;
    Ok(())
}

pub(crate) fn uninstall() -> Result<Option<PathBuf>> {
    let (service_path, timer_path) = unit_paths()?;
    if timer_path.exists() {
        drop(run(
            "systemctl",
            &["--user", "disable", "--now", TIMER_FILE],
            None,
        ));
        std::fs::remove_file(&timer_path)?;
        if service_path.exists() {
            std::fs::remove_file(&service_path)?;
        }
        drop(run("systemctl", &["--user", "daemon-reload"], None));
        return Ok(Some(timer_path));
    }
    crontab::uninstall()
}

pub(crate) fn status() -> Result<Option<PathBuf>> {
    let (_, timer_path) = unit_paths()?;
    Ok(timer_path
        .exists()
        .then_some(timer_path)
        .or_else(crontab::status))
}
