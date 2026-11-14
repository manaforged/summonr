#[path = "main/cli.rs"]
mod cli;
#[path = "main/diagnostics.rs"]
mod diagnostics;
#[cfg(all(feature = "gui", target_os = "macos"))]
#[path = "main/login_env.rs"]
mod login_env;
#[path = "main/modes.rs"]
mod modes;
#[path = "main/startup.rs"]
mod startup;

/// Runs the summonr command line and exits the process with its code.
pub fn main() {
    start(false);
}

/// Runs the Summonr desktop window and exits the process with its code.
#[cfg(feature = "gui")]
pub fn desktop() {
    start(true);
}

fn start(window: bool) {
    guard_sigpipe();
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == summonr_agent::INDEX_ENTRY)
    {
        let root = std::env::args_os().nth(2).unwrap_or_default();
        std::process::exit(summonr_agent::index_files(std::path::Path::new(&root)));
    }
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "askpass")
    {
        let args: Vec<_> = std::env::args_os().skip(2).collect();
        std::process::exit(summonr_agent::askpass::client(&args));
    }
    #[cfg(all(feature = "gui", target_os = "macos"))]
    if window {
        login_env::adopt();
    }
    let profile = std::env::args().nth(1).as_deref() == Some("--profile-tui");
    let args = (!profile)
        .then(|| cli::parse(window))
        .transpose()
        .unwrap_or_else(|error| fail_parse(&error));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_keep_alive(summonr_agent::limits::limits().thread_keep_alive())
        .enable_all()
        .build()
        .unwrap_or_else(|error| fail(&error));
    drop(runtime.spawn(async {
        summonr_agent::interrupted().await;
        summonr_agent::stop_children();
    }));
    let result = runtime.block_on(run(args));
    runtime.block_on(summonr_agent::supervise::closed());
    match result {
        Ok(code) => {
            summonr_agent::stop_children();
            std::process::exit(code);
        }
        Err(error) => fail(&error),
    }
}

#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "std has no wrapper to install a SIGPIPE handler"
)]
fn guard_sigpipe() {
    // SAFETY: called first in main before any thread starts; on_sigpipe is an extern "C" handler that only calls async-signal-safe functions.
    unsafe {
        libc::signal(
            libc::SIGPIPE,
            on_sigpipe as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
    }
}

#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "a signal handler polls and re-raises through libc"
)]
extern "C" fn on_sigpipe(_: libc::c_int) {
    let mut fds = [1, 2].map(|fd| libc::pollfd {
        fd,
        events: libc::POLLOUT,
        revents: 0,
    });
    // SAFETY: poll, signal and raise are async-signal-safe; fds is a valid array of two pollfd.
    unsafe {
        let broken = libc::poll(fds.as_mut_ptr(), 2, 0) > 0
            && fds
                .iter()
                .any(|fd| fd.revents & (libc::POLLERR | libc::POLLHUP) != 0);
        if broken {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            libc::raise(libc::SIGPIPE);
        }
    }
}

#[cfg(not(unix))]
fn guard_sigpipe() {}

async fn until_signal<T>(work: impl std::future::Future<Output = T>) -> Result<T, i32> {
    use futures::FutureExt;

    let mut interrupt = std::pin::pin!(summonr_agent::interrupted());
    tokio::select! {
        biased;
        code = &mut interrupt => Err(code),
        done = work => interrupt.now_or_never().map_or(Ok(done), Err),
    }
}

async fn run(args: Option<cli::Args>) -> summonr_agent::Result<i32> {
    match args {
        Some(args) => startup::run(args).await,
        None => until_signal(async { summonr_ui::profile(std::env::args().skip(2)) })
            .await
            .map_or_else(Ok, |done| done.map(|()| 0)),
    }
}

fn fail_parse(error: &summonr_agent::Error) -> ! {
    if std::env::args().skip(1).any(|arg| arg == "--json") {
        modes::print_json(&summonr_agent::Event::Aborted {
            reason: error.to_string(),
            code: summonr_agent::AbortCode::Error,
        });
    }
    fail(error)
}

fn fail(error: &dyn std::fmt::Display) -> ! {
    use std::io::Write;
    let name = option_env!("CARGO_BIN_NAME").unwrap_or("summonr");
    drop(writeln!(std::io::stderr(), "{name}: {error}"));
    summonr_agent::stop_children();
    std::process::exit(1)
}
