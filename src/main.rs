#[path = "main/cli.rs"]
mod cli;
#[path = "main/diagnostics.rs"]
mod diagnostics;
#[path = "main/modes.rs"]
mod modes;
#[path = "main/startup.rs"]
mod startup;

fn main() {
    guard_sigpipe();
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == summonr::INDEX_ENTRY)
    {
        let root = std::env::args_os().nth(2).unwrap_or_default();
        std::process::exit(summonr::index_files(std::path::Path::new(&root)));
    }
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "askpass")
    {
        let args: Vec<_> = std::env::args_os().skip(2).collect();
        std::process::exit(summonr::askpass::client(&args));
    }
    let profile = std::env::args().nth(1).as_deref() == Some("--profile-tui");
    let args = (!profile)
        .then(cli::parse)
        .transpose()
        .unwrap_or_else(|error| fail_parse(&error));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap_or_else(|error| fail(&error));
    match runtime.block_on(run(args)) {
        Ok(code) => {
            summonr::stop_children();
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

    let mut interrupt = std::pin::pin!(summonr::interrupted());
    tokio::select! {
        biased;
        code = &mut interrupt => Err(code),
        done = work => interrupt.now_or_never().map_or(Ok(done), Err),
    }
}

async fn run(args: Option<cli::Args>) -> summonr::Result<i32> {
    match args {
        Some(args) => {
            summonr::check_config()?;
            startup::run(args).await
        }
        None => until_signal(async { summonr::tui::profile(std::env::args().skip(2)) })
            .await
            .map_or_else(Ok, |done| done.map(|()| 0)),
    }
}

fn fail_parse(error: &summonr::Error) -> ! {
    if std::env::args().skip(1).any(|arg| arg == "--json") {
        modes::print_json(&summonr::Event::Aborted {
            reason: error.to_string(),
            code: summonr::AbortCode::Error,
        });
    }
    fail(error)
}

fn fail(error: &dyn std::fmt::Display) -> ! {
    eprintln!(
        "{}: {error}",
        option_env!("CARGO_BIN_NAME").unwrap_or("summonr")
    );
    summonr::stop_children();
    std::process::exit(1)
}
