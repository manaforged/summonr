use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

#[cfg(any(unix, test))]
pub(crate) mod crontab;

#[cfg(any(target_os = "macos", test))]
const LABEL: &str = "dev.summonr.cron";
#[cfg(any(target_os = "macos", test))]
const PLIST_DIR: &str = "Library/LaunchAgents";
#[cfg(any(target_os = "macos", test))]
const PLIST_FILE: &str = "dev.summonr.cron.plist";
#[cfg(target_os = "linux")]
const SYSTEMD_DIR: &str = ".config/systemd/user";
#[cfg(target_os = "linux")]
const SERVICE_FILE: &str = "summonr-cron.service";
#[cfg(target_os = "linux")]
const TIMER_FILE: &str = "summonr-cron.timer";
#[cfg(windows)]
const TASK_NAME: &str = "summonr-cron";
#[cfg(windows)]
const TASK_PATH: &str = "schtasks:summonr-cron";
#[cfg(target_os = "macos")]
const LOG_DIR: &str = "cron";
#[cfg(target_os = "macos")]
const LOG_FILE: &str = "tick.log";
#[cfg(any(unix, test))]
const HOME_VAR: &str = "SUMMONR_HOME";
#[cfg(any(unix, test))]
const PATH_VAR: &str = "PATH";
#[cfg(any(unix, test))]
const USER_HOME_VAR: &str = "HOME";
#[cfg(any(target_os = "linux", windows, test))]
const MINUTE_SECS: u64 = 60;
#[cfg(target_os = "linux")]
const LINGER_OFF: &str = "no";
#[cfg(target_os = "linux")]
const LINGER_NOTE: &str = "jobs run only while you are logged in; `loginctl enable-linger` keeps them running after logout";
#[cfg(target_os = "macos")]
const GUI_NOTE: &str = "jobs run while you are logged in";
#[cfg(target_os = "macos")]
const FULL_DISK_NOTE: &str = "cron needs Full Disk Access (System Settings › Privacy & Security) for jobs that touch Desktop, Documents or Downloads";

pub(crate) type Installed = (PathBuf, Option<&'static str>);

#[cfg(any(unix, test))]
#[derive(Clone, Copy)]
pub(crate) struct TickEnv<'a> {
    pub path: &'a str,
    pub home: Option<&'a str>,
    pub summonr_home: Option<&'a str>,
}

#[cfg(any(unix, test))]
impl<'a> TickEnv<'a> {
    pub(crate) fn vars(self) -> Vec<(&'static str, &'a str)> {
        let mut vars = vec![(PATH_VAR, self.path)];
        vars.extend(self.home.map(|home| (USER_HOME_VAR, home)));
        vars.extend(self.summonr_home.map(|home| (HOME_VAR, home)));
        vars
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn plist(exe: &Path, every_secs: u64, log: &Path, env: TickEnv<'_>) -> String {
    let env: String = env
        .vars()
        .into_iter()
        .map(|(key, value)| {
            format!(
                "    <key>{key}</key>\n    <string>{}</string>\n",
                xml(value)
            )
        })
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{LABEL}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n    <string>cron</string>\n    <string>tick</string>\n  </array>\n  <key>StartInterval</key>\n  <integer>{every_secs}</integer>\n  <key>RunAtLoad</key>\n  <false/>\n  <key>StandardOutPath</key>\n  <string>{log}</string>\n  <key>StandardErrorPath</key>\n  <string>{log}</string>\n  <key>EnvironmentVariables</key>\n  <dict>\n{env}  </dict>\n</dict>\n</plist>\n",
        xml(&exe.display().to_string()),
        log = xml(&log.display().to_string()),
    )
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn units(exe: &Path, every_secs: u64, env: TickEnv<'_>) -> (String, String) {
    let env: String = env
        .vars()
        .into_iter()
        .map(|(key, value)| format!("Environment={}\n", systemd_quote(&format!("{key}={value}"))))
        .collect();
    let service = format!(
        "[Unit]\nDescription=summonr cron tick\n\n[Service]\nType=oneshot\nKillMode=process\nExecStart={} cron tick\n{env}",
        systemd_quote(&exe.display().to_string())
    );
    let schedule = if every_secs == MINUTE_SECS {
        "OnCalendar=*-*-* *:*:00\n".to_string()
    } else {
        format!("OnBootSec={every_secs}\nOnUnitActiveSec={every_secs}\n")
    };
    let timer = format!(
        "[Unit]\nDescription=summonr cron tick\n\n[Timer]\n{schedule}AccuracySec=1s\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
    );
    (service, timer)
}

#[cfg(any(target_os = "macos", test))]
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "linux", test))]
fn systemd_quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}

#[cfg(unix)]
struct OwnedEnv {
    path: String,
    home: Option<String>,
    summonr_home: Option<String>,
}

#[cfg(unix)]
impl OwnedEnv {
    fn current() -> Self {
        let set = |key| std::env::var(key).ok().filter(|value| !value.is_empty());
        Self {
            path: std::env::var(PATH_VAR).unwrap_or_default(),
            home: set(USER_HOME_VAR),
            summonr_home: set(HOME_VAR),
        }
    }

    fn view(&self) -> TickEnv<'_> {
        TickEnv {
            path: &self.path,
            home: self.home.as_deref(),
            summonr_home: self.summonr_home.as_deref(),
        }
    }
}

#[cfg(unix)]
fn os_home() -> Result<PathBuf> {
    crate::config::home_dir().ok_or_else(|| Error::Config("cannot find the home directory".into()))
}

fn run(program: &str, args: &[&str], stdin: Option<&str>) -> Result<String> {
    use std::io::Write;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(if stdin.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        });
    let mut child = command
        .spawn()
        .map_err(|e| Error::Config(format!("{program}: {e}")))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Config(format!(
            "{program} {}: {}",
            args.join(" "),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn domains(uid: u32) -> [String; 2] {
    [format!("gui/{uid}"), format!("user/{uid}")]
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[expect(unsafe_code, reason = "std has no user-id query")]
fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(target_os = "macos")]
fn plist_path() -> Result<PathBuf> {
    Ok(os_home()?.join(PLIST_DIR).join(PLIST_FILE))
}

#[cfg(target_os = "macos")]
fn bootout() {
    for domain in domains(uid()) {
        drop(run(
            "launchctl",
            &["bootout", &format!("{domain}/{LABEL}")],
            None,
        ));
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn install(exe: &Path, every_secs: u64) -> Result<Installed> {
    let file = plist_path()?;
    let logs = crate::prompt::summonr_home()?.join(LOG_DIR);
    let log = logs.join(LOG_FILE);
    std::fs::create_dir_all(&logs)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let env = OwnedEnv::current();
    crate::durable::store::write(&file, plist(exe, every_secs, &log, env.view()))?;
    bootout();
    let mut failures = Vec::new();
    for (index, domain) in domains(uid()).into_iter().enumerate() {
        match run(
            "launchctl",
            &["bootstrap", &domain, &file.display().to_string()],
            None,
        ) {
            Ok(_) => {
                return Ok((file, (index == 0).then_some(GUI_NOTE)));
            }
            Err(Error::Config(text)) => failures.push(text),
            Err(error) => return Err(error),
        }
    }
    std::fs::remove_file(&file)?;
    crontab::install(exe, env.view())
        .map(|path| (path, Some(FULL_DISK_NOTE)))
        .map_err(|error| match error {
            Error::Config(text) => Error::Config(format!("{}; {text}", failures.join("; "))),
            other => other,
        })
}

#[cfg(target_os = "macos")]
pub(crate) fn uninstall() -> Result<Option<PathBuf>> {
    let file = plist_path()?;
    bootout();
    let tab = crontab::uninstall()?;
    if !file.exists() {
        return Ok(tab);
    }
    std::fs::remove_file(&file)?;
    Ok(Some(file))
}

#[cfg(target_os = "macos")]
pub(crate) fn status() -> Result<Option<PathBuf>> {
    let file = plist_path()?;
    Ok(file.exists().then_some(file).or_else(crontab::status))
}

#[cfg(target_os = "linux")]
fn unit_paths() -> Result<(PathBuf, PathBuf)> {
    let dir = os_home()?.join(SYSTEMD_DIR);
    Ok((dir.join(SERVICE_FILE), dir.join(TIMER_FILE)))
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub(crate) fn install(exe: &Path, every_secs: u64) -> Result<Installed> {
    let env = OwnedEnv::current();
    if run("systemctl", &["--user", "show-environment"], None).is_ok() {
        let (service_path, timer_path) = unit_paths()?;
        if let Some(dir) = timer_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let (service, timer) = units(exe, every_secs, env.view());
        crate::durable::store::write(&service_path, service)?;
        crate::durable::store::write(&timer_path, timer)?;
        run("systemctl", &["--user", "daemon-reload"], None)?;
        run(
            "systemctl",
            &["--user", "enable", "--now", TIMER_FILE],
            None,
        )?;
        return Ok((timer_path, linger_note()));
    }
    crontab::install(exe, env.view()).map(|path| (path, None))
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub(crate) fn status() -> Result<Option<PathBuf>> {
    let (_, timer_path) = unit_paths()?;
    Ok(timer_path
        .exists()
        .then_some(timer_path)
        .or_else(crontab::status))
}

#[cfg(windows)]
pub(crate) fn install(exe: &Path, every_secs: u64) -> Result<Installed> {
    let every = (every_secs / MINUTE_SECS).max(1).to_string();
    let action = format!("\"{}\" cron tick", exe.display());
    run(
        "schtasks",
        &[
            "/Create", "/TN", TASK_NAME, "/SC", "MINUTE", "/MO", &every, "/TR", &action, "/F",
        ],
        None,
    )?;
    Ok((PathBuf::from(TASK_PATH), None))
}

#[cfg(windows)]
pub(crate) fn uninstall() -> Result<Option<PathBuf>> {
    if status()?.is_none() {
        return Ok(None);
    }
    run("schtasks", &["/Delete", "/TN", TASK_NAME, "/F"], None)?;
    Ok(Some(PathBuf::from(TASK_PATH)))
}

#[cfg(windows)]
pub(crate) fn status() -> Result<Option<PathBuf>> {
    Ok(run("schtasks", &["/Query", "/TN", TASK_NAME], None)
        .ok()
        .map(|_| PathBuf::from(TASK_PATH)))
}
