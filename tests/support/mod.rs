use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

pub(crate) mod fixture;
pub(crate) mod http;
mod model;

pub(crate) use model::{Model, Step};

const WINDOWS_SLOWDOWN: u64 = 5;
const FINISH_SECS: u64 = 90;

pub(crate) fn slow(secs: u64) -> u64 {
    if cfg!(windows) {
        secs * WINDOWS_SLOWDOWN
    } else {
        secs
    }
}

const PROFILE_DIRS: &[&str] = &["AppData/Local", "AppData/Roaming"];
const TAIL_LINES: usize = 6;

#[cfg(windows)]
fn children(pid: u32) -> String {
    let script = format!(
        "function T($p, $d) {{ Get-CimInstance Win32_Process -Filter \"ParentProcessId=$p\" | ForEach-Object {{ ('  ' * $d) + \"$($_.ProcessId) $($_.Name) $($_.CommandLine)\"; T $_.ProcessId ($d + 1) }} }}; T {pid} 0"
    );
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(unix)]
fn children(pid: u32) -> String {
    Command::new("pgrep")
        .args(["-l", "-P", &pid.to_string()])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}
const TAIL_LINE_CHARS: usize = 300;

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..]
        .iter()
        .map(|line| line.chars().take(TAIL_LINE_CHARS).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn journals(home: &Path) -> String {
    std::fs::read_dir(home.join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| tail(&read(&entry.path().join("session.jsonl"))))
        .collect::<Vec<_>>()
        .join("\n--\n")
}

pub(crate) fn pause(wait: Duration) {
    let until = Instant::now() + wait;
    while let Some(left) = until
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
    {
        thread::park_timeout(left);
    }
}

pub(crate) struct Session {
    pub(crate) home: tempfile::TempDir,
    pub(crate) work: tempfile::TempDir,
    pub(crate) files: tempfile::TempDir,
}

pub(crate) struct Outcome {
    pub(crate) stdout: String,
    pub(crate) code: Option<i32>,
}

#[derive(Default)]
pub(crate) struct Options<'a> {
    pub(crate) work: Option<&'a Path>,
    pub(crate) env: &'a [(&'a str, &'a str)],
    pub(crate) images: bool,
    pub(crate) interrupt: Option<(&'a Path, &'a str)>,
    pub(crate) prompt: Option<&'a str>,
}

impl Session {
    pub(crate) fn new() -> Self {
        Self::with_files(&std::env::temp_dir())
    }

    pub(crate) fn with_files(files: &Path) -> Self {
        let home = tempfile::tempdir().unwrap();
        if cfg!(windows) {
            for dir in PROFILE_DIRS {
                std::fs::create_dir_all(home.path().join(dir)).unwrap();
            }
        }
        Self {
            home,
            work: repository(),
            files: tempfile::tempdir_in(files).unwrap(),
        }
    }

    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.files.path().join(name)
    }

    pub(crate) fn server(&self, name: &str, extra: &str) -> String {
        self.server_env(name, "", extra)
    }

    pub(crate) fn server_env(&self, name: &str, env: &str, extra: &str) -> String {
        self.launched(
            name,
            &format!("command = '{}'", fixture().display()),
            env,
            extra,
        )
    }

    pub(crate) fn launched(&self, name: &str, launch: &str, env: &str, extra: &str) -> String {
        format!(
            "[mcp.{name}]\n{launch}\nenv = {{ MCP_FIXTURE_SERVE = '1', MCP_FIXTURE_LOG = '{}', MCP_FIXTURE_STARTS = '{}'{env} }}\n{extra}\n",
            self.file(&format!("{name}.log")).display(),
            self.file(&format!("{name}.starts")).display(),
        )
    }

    pub(crate) fn run(&self, model: &Model, config: &str, flags: &[&str]) -> Outcome {
        self.run_with(model, config, flags, &Options::default())
    }

    pub(crate) fn run_with(
        &self,
        model: &Model,
        config: &str,
        flags: &[&str],
        options: &Options<'_>,
    ) -> Outcome {
        let endpoint = format!(
            "[endpoint.replay]\nbase_url = \"http://127.0.0.1:{}/v1\"\nmodel = \"replay\"\napi_key = \"replay\"\ncontext_window = 262144\n{}\n",
            model.port,
            if options.images {
                "images = true\n"
            } else {
                ""
            }
        );
        std::fs::write(
            self.home.path().join("config.toml"),
            format!("{endpoint}{config}"),
        )
        .unwrap();
        let stdout = self.file("stdout");
        let stderr = self.file("stderr");
        let mut command = Command::new(env!("CARGO_BIN_EXE_summonr"));
        command.args(["-e", "replay"]).args(flags);
        if !flags
            .iter()
            .any(|flag| matches!(*flag, "--goal" | "--steps"))
        {
            command.args(["-p", options.prompt.unwrap_or("go")]);
        }
        let mut child = command
            .current_dir(options.work.unwrap_or(self.work.path()))
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("SUMMONR_HOME", self.home.path())
            .env(
                "SUMMONR_MODELS_URL",
                format!("http://127.0.0.1:{}/registry.json", model.port),
            )
            .envs(options.env.iter().copied())
            .stdin(Stdio::null())
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let mut interrupted = false;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(slow(FINISH_SECS)) {
                drop(child.kill());
                panic!(
                    "summonr did not finish: {}\nstdout ends: {}\njournal ends: {}\nchildren: {}",
                    read(&stderr),
                    tail(&read(&stdout)),
                    journals(self.home.path()),
                    children(child.id())
                );
            }
            if !interrupted
                && options
                    .interrupt
                    .is_some_and(|(file, text)| read(file).contains(text))
            {
                interrupted = true;
                drop(
                    Command::new("kill")
                        .args(["-INT", &child.id().to_string()])
                        .status(),
                );
            }
            pause(Duration::from_millis(50));
        };
        Outcome {
            stdout: read(&stdout),
            code: status.code(),
        }
    }

    pub(crate) fn cli(&self, args: &[&str]) -> (bool, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
            .args(args)
            .current_dir(self.work.path())
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("SUMMONR_HOME", self.home.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        (output.status.success(), text)
    }

    pub(crate) fn received(&self, name: &str) -> Vec<Value> {
        read(&self.file(&format!("{name}.log")))
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    pub(crate) fn received_until(
        &self,
        name: &str,
        wait: Duration,
        ready: impl Fn(&[Value]) -> bool,
    ) -> Vec<Value> {
        let until = Instant::now() + wait;
        loop {
            let received = self.received(name);
            if ready(&received) || Instant::now() >= until {
                return received;
            }
            pause(Duration::from_millis(20));
        }
    }

    pub(crate) fn starts(&self, name: &str) -> usize {
        read(&self.file(&format!("{name}.starts"))).lines().count()
    }
}

pub(crate) fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

pub(crate) fn fixture() -> PathBuf {
    std::env::current_exe().unwrap()
}

pub(crate) fn harness(tests: &[(&'static str, fn())]) {
    let mut filters = Vec::new();
    let mut skips = Vec::new();
    let mut exact = false;
    let mut list = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--exact" => exact = true,
            "--skip" => skips.extend(args.next()),
            "--test-threads" | "--color" | "--format" | "-Z" => drop(args.next()),
            flag if flag.starts_with('-') => {}
            filter => filters.push(filter.to_string()),
        }
    }
    let matches = |name: &str, pattern: &String| {
        if exact {
            name == pattern
        } else {
            name.contains(pattern.as_str())
        }
    };
    let chosen: Vec<_> = tests
        .iter()
        .filter(|(name, _)| {
            filters.is_empty() || filters.iter().any(|filter| matches(name, filter))
        })
        .filter(|(name, _)| !skips.iter().any(|skip| matches(name, skip)))
        .collect();
    if list {
        for (name, _) in &chosen {
            println!("{name}: test");
        }
        return;
    }
    let running: Vec<_> = chosen
        .into_iter()
        .map(|(name, test)| (*name, thread::spawn(*test)))
        .collect();
    let mut failed = 0;
    let total = running.len();
    for (name, run) in running {
        let passed = run.join().is_ok();
        println!("test {name} ... {}", if passed { "ok" } else { "FAILED" });
        failed += usize::from(!passed);
    }
    println!(
        "test result: {}. {} passed; {failed} failed; 0 ignored; 0 measured; {} filtered out",
        if failed == 0 { "ok" } else { "FAILED" },
        total - failed,
        tests.len() - total
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

pub(crate) fn advertised(request: &Value) -> Vec<String> {
    request["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_string))
        .collect()
}

pub(crate) fn calls<'a>(received: &'a [Value], method: &str) -> Vec<&'a Value> {
    received
        .iter()
        .filter(|frame| frame["method"] == method)
        .collect()
}

pub(crate) fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(git.success());
    dir
}

#[cfg(unix)]
pub(crate) fn survivors(pattern: &str) -> Vec<String> {
    let mut left = Vec::new();
    for _ in 0..40 {
        let found = Command::new("pgrep")
            .args(["-f", pattern])
            .output()
            .unwrap();
        left = String::from_utf8_lossy(&found.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        if left.is_empty() {
            break;
        }
        pause(Duration::from_millis(100));
    }
    for pid in &left {
        drop(Command::new("kill").args(["-9", pid]).status());
    }
    left
}
