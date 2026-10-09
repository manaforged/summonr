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
