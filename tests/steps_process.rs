use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn steps_resume_after_process_kill_at_first_unfinished_step()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let server = FakeServer::spawn(&fixture)?;
    let args = fixture.command_args();

    let mut first = RunningChild::spawn(&fixture, &args, "first")?;
    wait_for_file(&fixture.step_two_first, WAIT)?;
    let before_kill = wait_for_state(&fixture.workspace, |state| {
        state.get("next_step").and_then(Value::as_u64) == Some(2)
            && state.get("last_completed_step").and_then(Value::as_u64) == Some(1)
            && state.get("spent_tokens").and_then(Value::as_u64) == Some(110)
    })?;
    let session = before_kill
        .get("session")
        .and_then(Value::as_str)
        .ok_or("step state did not bind a session")?
        .to_string();

    let killed = first.kill()?;
    assert!(
        !killed.success(),
        "the first Summoner process exited normally before the kill"
    );
    fs::write(&fixture.release_first_step_two, b"release\n")?;

    let mut second = RunningChild::spawn(&fixture, &args, "second")?;
    wait_for_file(&fixture.resumed_step_two, WAIT)?;
    let status = second.wait(WAIT)?;
    assert!(
        status.success(),
        "resumed Summoner exited with {status}; stderr: {}",
        fs::read_to_string(fixture.stderr("second")).unwrap_or_default()
    );

    let after_restart = wait_for_state(&fixture.workspace, |state| {
        state
            .get("terminal")
            .and_then(|terminal| terminal.get("state"))
            == Some(&Value::String("completed".to_string()))
            && state.get("spent_tokens").and_then(Value::as_u64) == Some(180)
            && state.get("session").and_then(Value::as_str) == Some(session.as_str())
    })?;
    assert_eq!(
        after_restart.get("next_step").and_then(Value::as_u64),
        Some(3),
        "the resumed job did not advance past step 2"
    );
    assert_eq!(
        after_restart.get("max_tokens").and_then(Value::as_u64),
        Some(1_000),
        "the resumed job changed its cumulative budget ceiling"
    );

    let requests = server.join()?;
    assert_eq!(
        requests.len(),
        3,
        "unexpected provider request count: {requests:?}"
    );
    assert!(requests[0].starts_with("Step 1 of 2 from PLAN.md."));
    assert!(requests[1].starts_with("Step 2 of 2 from PLAN.md."));
    assert!(requests[2].starts_with("Step 2 of 2 from PLAN.md."));
    assert_eq!(
        requests
            .iter()
            .filter(|prompt| prompt.starts_with("Step 1 of 2 from PLAN.md."))
            .count(),
        1,
        "completed step 1 was submitted again"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|prompt| prompt.starts_with("Step 2 of 2 from PLAN.md."))
            .count(),
        2,
        "the in-flight step was not retried at least once after the kill"
    );

    let sessions = fs::read_dir(fixture.home.join("sessions"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("jsonl"))
        .count();
    assert_eq!(sessions, 1, "restart created a new unbound session");
    Ok(())
}

struct Fixture {
    root: PathBuf,
    workspace: PathBuf,
    home: PathBuf,
    step_two_first: PathBuf,
    release_first_step_two: PathBuf,
    resumed_step_two: PathBuf,
}

impl Fixture {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mage-steps-process-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let workspace = root.join("workspace");
        let home = root.join("home");
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(&home)?;
        let plan = workspace.join("PLAN.md");
        fs::write(
            &plan,
            "## Steps\n\n1. Complete the first step.\n2. Complete the second step.\n",
        )?;
        Ok(Self {
            step_two_first: root.join("step-two-first-requested"),
            release_first_step_two: root.join("release-first-step-two"),
            resumed_step_two: root.join("step-two-resumed"),
            root,
            workspace,
            home,
        })
    }

    fn command_args(&self) -> Vec<String> {
        vec![
            "--steps".to_string(),
            "PLAN.md".to_string(),
            "--json".to_string(),
            "--no-tui".to_string(),
            "--no-splash".to_string(),
            "--max-tokens".to_string(),
            "1000".to_string(),
            "--max-secs".to_string(),
            "600".to_string(),
        ]
    }

    fn stdout(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.stdout"))
    }

    fn stderr(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.stderr"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.root));
    }
}

struct RunningChild {
    child: Option<Child>,
}

impl RunningChild {
    fn spawn(fixture: &Fixture, args: &[String], name: &str) -> io::Result<Self> {
        let stdout = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(fixture.stdout(name))?;
        let stderr = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(fixture.stderr(name))?;
        let child = Command::new(env!("CARGO_BIN_EXE_summon"))
            .args(args)
            .current_dir(&fixture.workspace)
            .env("MAGE_HOME", &fixture.home)
            .env("MAGE_BASE_URL", fixture.server_url()?)
            .env("MAGE_API_KEY", "integration-test")
            .env("MAGE_MODEL", "integration-test-model")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    fn kill(&mut self) -> io::Result<ExitStatus> {
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| io::Error::other("child was already reaped"))?;
        if child.try_wait()?.is_none() {
            child.kill()?;
        }
        self.wait(WAIT)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "bounded synchronous integration-test polling"
    )]
    fn wait(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| io::Error::other("child was already reaped"))?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "child did not exit before the deadline",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if child.try_wait().ok().flatten().is_none() {
            drop(child.kill());
            drop(child.wait());
        }
    }
}

struct FakeServer {
    join: Option<JoinHandle<Result<Vec<String>, String>>>,
}

impl FakeServer {
    fn spawn(fixture: &Fixture) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server_url = format!("http://{address}");
        fs::write(fixture.root.join("server-url"), &server_url)?;
        let first_request = fixture.step_two_first.clone();
        let release = fixture.release_first_step_two.clone();
        let resumed = fixture.resumed_step_two.clone();
        let join = thread::spawn(move || serve(listener, first_request, release, resumed));
        Ok(Self { join: Some(join) })
    }

    fn join(mut self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let join = self.join.take().ok_or("fake server was already joined")?;
        let result = join
            .join()
            .map_err(|_panic| io::Error::other("fake server thread panicked"))?;
        result.map_err(io::Error::other).map_err(Into::into)
    }
}

impl Fixture {
    fn server_url(&self) -> io::Result<String> {
        fs::read_to_string(self.root.join("server-url"))
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "bounded synchronous integration-test polling"
)]
fn serve(
    listener: TcpListener,
    first_request: PathBuf,
    release: PathBuf,
    resumed: PathBuf,
) -> Result<Vec<String>, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("set listener nonblocking: {error}"))?;
    let deadline = Instant::now() + WAIT;
    let mut requests = Vec::new();
    while requests.len() < 3 {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("fake server timed out waiting for a provider request".to_string());
                }
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(error) => return Err(format!("accept provider connection: {error}")),
        };
        stream
            .set_read_timeout(Some(WAIT))
            .map_err(|error| format!("set stream timeout: {error}"))?;
        let body = read_http_body(&mut stream).map_err(|error| format!("read request: {error}"))?;
        let prompt = last_user_prompt(&body).map_err(|error| format!("decode request: {error}"))?;
        requests.push(prompt.clone());
        match requests.len() {
            1 => write_response(&mut stream, 100, 10)
                .map_err(|error| format!("first response: {error}"))?,
            2 => {
                if !prompt.starts_with("Step 2 of 2 from PLAN.md.") {
                    return Err(format!("second request was not step 2: {prompt}"));
                }
                fs::write(&first_request, b"step 2 request reached provider\n")
                    .map_err(|error| format!("write first step-2 marker: {error}"))?;
                wait_for_file(&release, WAIT)
                    .map_err(|error| format!("wait for first step-2 release: {error}"))?;
                drop(write_response(&mut stream, 63, 7));
            }
            3 => {
                if !prompt.starts_with("Step 2 of 2 from PLAN.md.") {
                    return Err(format!("resumed request was not step 2: {prompt}"));
                }
                write_response(&mut stream, 63, 7)
                    .map_err(|error| format!("resumed response: {error}"))?;
                fs::write(&resumed, b"step 2 resumed\n")
                    .map_err(|error| format!("write resumed marker: {error}"))?;
            }
            _ => unreachable!(),
        }
    }
    Ok(requests)
}

fn read_http_body(stream: &mut TcpStream) -> io::Result<String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Ok(String::from_utf8_lossy(&request).into_owned());
        }
        request.extend_from_slice(&buffer[..count]);
        if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "request has no content length")
        })?;
    while request.len() < header_end + length {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
    }
    if request.len() < header_end + length {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "request body ended before content length",
        ));
    }
    Ok(String::from_utf8_lossy(&request[header_end..header_end + length]).into_owned())
}

fn last_user_prompt(body: &str) -> Result<String, serde_json::Error> {
    let value: Value = serde_json::from_str(body)?;
    Ok(value
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages.iter().rev().find_map(|message| {
                (message.get("role").and_then(Value::as_str) == Some("user")).then(|| {
                    message
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                })
            })
        })
        .unwrap_or_default()
        .to_string())
}

fn write_response(
    stream: &mut TcpStream,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> io::Result<()> {
    let body = format!(
        "{{\"choices\":[{{\"message\":{{\"content\":\"done\"}}}}],\"usage\":{{\"prompt_tokens\":{prompt_tokens},\"completion_tokens\":{completion_tokens}}}}}"
    );
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

#[expect(
    clippy::disallowed_methods,
    reason = "bounded synchronous integration-test polling"
)]
fn wait_for_file(path: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    while !path.is_file() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{} did not appear before the deadline", path.display()),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[expect(
    clippy::disallowed_methods,
    reason = "bounded synchronous integration-test polling"
)]
fn wait_for_state(
    workspace: &Path,
    predicate: impl Fn(&Value) -> bool,
) -> Result<Value, Box<dyn std::error::Error>> {
    let jobs = workspace.join(".mage/jobs");
    let deadline = Instant::now() + WAIT;
    loop {
        let mut seen = Vec::new();
        if let Ok(entries) = fs::read_dir(&jobs) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let text = fs::read_to_string(entry.path())?;
                let state: Value = serde_json::from_str(&text)?;
                seen.push(state.clone());
                if predicate(&state) {
                    return Ok(state);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "step state did not reach the expected value in {}; seen: {seen:?}",
                jobs.display()
            )
            .into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}
