use std::path::{Path, PathBuf};

use summonr_core::error::{Error, Result};

#[cfg(any(unix, test))]
pub(crate) mod crontab;
#[cfg(target_os = "linux")]
mod systemd;

#[cfg(target_os = "linux")]
pub(crate) use systemd::{install, status, uninstall};

#[cfg(any(target_os = "macos", test))]
const LABEL: &str = "dev.summonr.cron";
#[cfg(target_os = "macos")]
const PLIST_DIR: &str = "Library/LaunchAgents";
#[cfg(target_os = "macos")]
const PLIST_FILE: &str = "dev.summonr.cron.plist";
#[cfg(windows)]
const TASK_NAME: &str = "summonr-cron";
#[cfg(windows)]
const TASK_PATH: &str = "schtasks:summonr-cron";
#[cfg(target_os = "macos")]
const LOG_DIR: &str = "cron";
#[cfg(target_os = "macos")]
const LOG_FILE: &str = "tick.log";
#[cfg(any(unix, test))]
#[cfg(any(unix, test))]
const USER_HOME_VAR: &str = "HOME";
#[cfg(any(target_os = "linux", windows, test))]
const MINUTE_SECS: u64 = 60;
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
        let mut vars = vec![(summonr_core::dirs::PATH_ENV, self.path)];
        vars.extend(self.home.map(|home| (USER_HOME_VAR, home)));
        vars.extend(
            self.summonr_home
                .map(|home| (summonr_core::dirs::HOME_ENV, home)),
        );
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
            path: std::env::var(summonr_core::dirs::PATH_ENV).unwrap_or_default(),
            home: set(USER_HOME_VAR),
            summonr_home: set(summonr_core::dirs::HOME_ENV),
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
    summonr_core::dirs::home_dir()
        .ok_or_else(|| Error::Config("cannot find the home directory".into()))
}

fn run(program: &str, args: &[&str], stdin: Option<&str>) -> Result<String> {
    use std::io::Write;
    let mut child = crate::process::spawn_blocking(crate::process::Spec {
        stdin: if stdin.is_some() {
            crate::process::Io::Pipe
        } else {
            crate::process::Io::Null
        },
        ..crate::process::Spec::new(program, args)
    })?;
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct Saved {
    path: PathBuf,
    before: Option<Vec<u8>>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Saved {
    fn take(path: &Path) -> Result<Self> {
        let before = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path: path.to_path_buf(),
            before,
        })
    }

    fn restore(&self) -> Option<String> {
        let restored = match &self.before {
            Some(bytes) => crate::durable::store::write(
                &self.path,
                bytes,
                crate::durable::store::Durability::Synced,
            ),
            None => match std::fs::remove_file(&self.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        restored
            .err()
            .map(|error| format!("cannot restore {}: {error}", self.path.display()))
    }
}

#[cfg(target_os = "linux")]
fn rolled_back(error: Error, saved: &[Saved], mut notes: Vec<String>) -> Error {
    notes.extend(saved.iter().filter_map(Saved::restore));
    if notes.is_empty() {
        return error;
    }
    Error::Config(format!("{error}; {}", notes.join("; ")))
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
    let logs = summonr_core::dirs::summonr_home()?.join(LOG_DIR);
    let log = logs.join(LOG_FILE);
    std::fs::create_dir_all(&logs)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let env = OwnedEnv::current();
    let saved = Saved::take(&file)?;
    crate::durable::store::write(
        &file,
        plist(exe, every_secs, &log, env.view()),
        crate::durable::store::Durability::Synced,
    )?;
    bootout();
    let mut notes = match bootstrap(&file) {
        Ok(gui) => return Ok((file, gui.then_some(GUI_NOTE))),
        Err(failures) => failures,
    };
    let restored = saved.restore();
    let reload = saved.before.is_some() && restored.is_none();
    notes.extend(restored);
    if reload {
        match bootstrap(&file) {
            Ok(_) => {
                return Err(Error::Config(format!(
                    "cannot load the new launch agent ({}); restored and reloaded the previous {}",
                    notes.join("; "),
                    file.display()
                )));
            }
            Err(again) => notes.push(format!(
                "cannot reload the previous launch agent {}: {}",
                file.display(),
                again.join("; ")
            )),
        }
    }
    crontab::install(exe, env.view())
        .map(|path| (path, Some(FULL_DISK_NOTE)))
        .map_err(|error| Error::Config(format!("{}; {error}", notes.join("; "))))
}

#[cfg(target_os = "macos")]
fn bootstrap(file: &Path) -> std::result::Result<bool, Vec<String>> {
    let target = file.display().to_string();
    let mut failures = Vec::new();
    for (index, domain) in domains(uid()).into_iter().enumerate() {
        match run("launchctl", &["bootstrap", &domain, &target], None) {
            Ok(_) => return Ok(index == 0),
            Err(error) => failures.push(error.to_string()),
        }
    }
    Err(failures)
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
