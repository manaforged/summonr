use tokio::io::AsyncReadExt;

use super::*;

async fn seen(base: Env) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut process = start(&Launch {
        program: "sh",
        args: &["-c", "printf %s \"${CARGO_MANIFEST_DIR:-unset}\""],
        env: &[],
        cwd: dir.path(),
        sandbox: None,
        base,
        stderr: Stderr::Discard,
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
#[tokio::test]
async fn a_read_only_server_cannot_write_its_workspace() {
    use crate::tools::sandbox::{Network, Sandbox};
    let dir = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let Ok(sandbox) = Sandbox::discover(dir.path(), Network::Allowed, &[]) else {
        return;
    };
    for (access, name) in [(Access::Read, "read"), (Access::Write, "write")] {
        let script = format!("echo x > {name}; true");
        let mut process = start(&Launch {
            program: "sh",
            args: &["-c", &script],
            env: &[],
            cwd: dir.path(),
            sandbox: Some((&sandbox, access)),
            base: Env::Clean,
            stderr: Stderr::Discard,
        })
        .unwrap();
        process.child.wait().await.unwrap();
    }
    assert!(
        !dir.path().join("read").exists(),
        "a read-only server wrote"
    );
    assert!(
        dir.path().join("write").exists(),
        "the writable sandbox refused"
    );
}
