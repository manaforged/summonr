use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(unix)]
#[tokio::test]
async fn a_monitor_timeout_stops_descendant_commands() -> crate::Result<()> {
    let workspace = tempfile::tempdir()?;
    let tools = ToolRegistry::with_policy_and_search(
        workspace.path(),
        crate::policy::Policy::builtin(),
        None,
    )?;
    tools.set_session("monitor-timeout-test");
    let output = tools
        .monitor(&serde_json::json!({
            "description": "deadline regression",
            "command": "printf started > started.txt; sleep 2; printf escaped > escaped.txt",
            "timeout_ms": 1000,
        }))
        .await?;
    assert!(output.success, "{}", output.text);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        workspace.path().join("started.txt").exists(),
        "the monitor command never started"
    );
    assert!(
        !workspace.path().join("escaped.txt").exists(),
        "the descendant command survived its monitor deadline"
    );
    Ok(())
}

#[tokio::test]
async fn a_finished_monitor_cancels_its_pending_timer() -> crate::Result<()> {
    let workspace = tempfile::tempdir()?;
    let spec = Spec::new(
        summonr_core::shell::program()?,
        summonr_core::shell::arguments("exit 0", workspace.path()),
    );
    let mut child = spawn_blocking(Spec {
        cwd: Some(workspace.path().to_path_buf()),
        stdin: Io::Null,
        stderr: Io::Null,
        ..spec
    })?;
    let stdout = child.stdout.take().expect("the monitor has stdout");
    let group = super::super::proc::ProcessGroup::new(child.id());
    let fired = Arc::new(AtomicBool::new(false));
    let timer_fired = Arc::clone(&fired);
    let timer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        timer_fired.store(true, Ordering::SeqCst);
    });
    tokio::task::spawn_blocking(move || {
        watch(
            child,
            stdout,
            group,
            "monitor-timer-test".to_string(),
            "timer regression".to_string(),
            Some(timer),
        );
    })
    .await
    .expect("the monitor watcher finished");
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !fired.load(Ordering::SeqCst),
        "the retired monitor timer fired"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn a_monitor_command_sees_only_the_clean_environment() -> crate::Result<()> {
    let workspace = tempfile::tempdir()?;
    let tools = ToolRegistry::with_policy_and_search(
        workspace.path(),
        crate::policy::Policy::builtin(),
        None,
    )?;
    tools.set_session("monitor-env-test");
    tools
        .monitor(&serde_json::json!({
            "description": "environment",
            "command": "printf '%s' \"${CARGO_MANIFEST_DIR:-unset}\" > seen.tmp && mv seen.tmp seen.txt",
            "timeout_ms": 5000,
        }))
        .await?;
    let seen = workspace.path().join("seen.txt");
    for _ in 0..250 {
        if seen.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(std::fs::read_to_string(seen)?, "unset");
    Ok(())
}
