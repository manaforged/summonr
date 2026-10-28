use tokio::io::AsyncReadExt;

use crate::process::{Env, Io, Spec, start};

async fn seen(base: Env) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut process = start(Spec {
        cwd: Some(dir.path().to_path_buf()),
        base,
        stderr: Io::Null,
        ..Spec::new("sh", ["-c", "printf %s \"${CARGO_MANIFEST_DIR:-unset}\""])
    })
    .unwrap();
    let mut out = String::new();
    process.stdout.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn a_clean_server_does_not_see_the_parent_environment() {
    assert_eq!(seen(Env::Clean).await, "unset");
    assert_ne!(seen(Env::Inherit).await, "unset");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_client_returns_before_the_close_grace() {
    let dir = tempfile::tempdir().unwrap();
    let client = crate::rpc::client::Client::spawn(&crate::rpc::client::Spawn {
        program: "sh",
        args: &["-c", "while :; do sleep 1; done"],
        env: &[],
        cwd: dir.path(),
        wait: std::time::Duration::from_secs(5),
        base: Env::Clean,
        framing: crate::rpc::Framing::Line,
        max: crate::limits::limits().mcp_frame_max_bytes,
        peer: crate::rpc::Peer::Mcp,
    })
    .unwrap();
    let started = std::time::Instant::now();
    drop(client);
    let grace = crate::limits::limits().mcp_close_grace();
    assert!(started.elapsed() < grace / 2, "{:?}", started.elapsed());
}
