#![expect(
    unsafe_code,
    reason = "Regression captures actual Windows file security descriptors"
)]

use std::path::Path;
use std::ptr::null_mut;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use tokio::time::{sleep, timeout};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;

use super::{Local, boolean, status, wide};
use crate::Result;
use crate::tools::ToolRegistry;

fn descriptor(path: &Path) -> std::io::Result<(bool, String)> {
    let path = wide(path)?;
    let mut descriptor = null_mut();
    // SAFETY: The path is terminated and descriptor receives owned storage.
    status(unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    })?;
    let descriptor = Local(descriptor);
    let mut text = null_mut();
    let mut len = 0;
    // SAFETY: The descriptor guard owns the input; text receives a LocalAlloc string.
    boolean(unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.0,
            1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            &mut len,
        )
    })?;
    let text = Local(text.cast());
    // SAFETY: The API reports the size including its terminating zero.
    let text = unsafe {
        String::from_utf16_lossy(std::slice::from_raw_parts(
            text.0.cast::<u16>(),
            len as usize - 1,
        ))
    };
    let (control, entries) = text.split_once('(').expect("fixture has access entries");
    Ok((
        control.contains('P'),
        entries.trim_end_matches('\0').to_string(),
    ))
}

async fn ready(path: &Path) -> bool {
    timeout(Duration::from_secs(10), async {
        while !path.exists() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn timeout_stops_the_command_and_restores_workspace_permissions() -> Result<()> {
    let root = tempfile::tempdir()?;
    let before = descriptor(root.path())?;
    let mut tools = ToolRegistry::new(root.path())?;
    tools.enable_sandbox(true)?;
    let output = tools.run_command(&json!({
        "command": "[IO.File]::WriteAllText('child.pid', [string]$PID); Start-Sleep -Seconds 60",
        "timeout_secs": 3
    }).to_string()).await?;
    assert!(!output.success, "{}", output.text);
    assert!(output.text.contains("timed out"), "{}", output.text);
    let pid = std::fs::read_to_string(root.path().join("child.pid"))?
        .parse()
        .expect("PID");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    assert!(
        !crate::jobs::alive_since(pid, now),
        "cancelled command is still alive"
    );
    assert_eq!(
        descriptor(root.path())?,
        before,
        "sandbox left a permission entry"
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_during_startup_restores_workspace_permissions() -> Result<()> {
    let root = tempfile::tempdir()?;
    let before = descriptor(root.path())?;
    let mut tools = ToolRegistry::new(root.path())?;
    tools.enable_sandbox(true)?;
    let output = tools
        .run_command(
            &json!({
                "command": "Start-Sleep -Seconds 60",
                "timeout_secs": 0
            })
            .to_string(),
        )
        .await?;
    assert!(!output.success, "{}", output.text);
    assert!(output.text.contains("timed out"), "{}", output.text);
    assert_eq!(
        descriptor(root.path())?,
        before,
        "startup cancellation left a permission entry"
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_commands_keep_their_permissions_until_each_finishes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let before = descriptor(root.path())?;
    let mut tools = ToolRegistry::new(root.path())?;
    tools.enable_sandbox(true)?;
    let tools = Arc::new(tools);
    let first = {
        let tools = tools.clone();
        tokio::spawn(async move {
            tools.run_command(&json!({
            "command": "[IO.File]::WriteAllText('first', 'ready'); while (![IO.File]::Exists('second')) { Start-Sleep -Milliseconds 20 }",
            "timeout_secs": 10
        }).to_string()).await
        })
    };
    assert!(
        ready(&root.path().join("first")).await,
        "first command did not start"
    );
    let second = {
        let tools = tools.clone();
        tokio::spawn(async move {
            tools.run_command(&json!({
            "command": "[IO.File]::WriteAllText('second', 'ready'); while (![IO.File]::Exists('release')) { Start-Sleep -Milliseconds 20 }; [IO.File]::WriteAllText('result', 'complete')",
            "timeout_secs": 10
        }).to_string()).await
        })
    };
    let output = first.await.expect("first task")?;
    assert!(output.success, "{}", output.text);
    std::fs::write(root.path().join("release"), "continue")?;
    let output = second.await.expect("second task")?;
    assert!(output.success, "{}", output.text);
    assert_eq!(
        std::fs::read_to_string(root.path().join("result"))?,
        "complete"
    );
    assert_eq!(
        descriptor(root.path())?,
        before,
        "concurrent commands left a permission entry"
    );
    Ok(())
}

#[tokio::test]
async fn a_later_session_stops_a_detached_job_and_restores_permissions() -> Result<()> {
    let root = tempfile::tempdir()?;
    let before = descriptor(root.path())?;
    let mut tools = ToolRegistry::new(root.path())?;
    tools.enable_sandbox(true)?;
    let output = tools
        .run_command(
            &json!({
            "command": "[IO.File]::WriteAllText('ready', 'started'); Start-Sleep -Seconds 60",
                "background": true
            })
            .to_string(),
        )
        .await?;
    assert!(output.success, "{}", output.text);
    let record = crate::jobs::since(root.path(), 0, None)
        .pop()
        .expect("background record");
    if !ready(&root.path().join("ready")).await {
        let status = crate::jobs::status(root.path(), &record.id)?;
        super::stop(record.pid)?;
        panic!("background command did not start: {}", status.text);
    }
    drop(tools);
    let tools = ToolRegistry::new(root.path())?;
    let output = tools
        .run_command(&json!({"job": record.id, "kill": true}).to_string())
        .await?;
    assert!(output.text.contains("killed"), "{}", output.text);
    assert!(
        !crate::jobs::alive_since(record.pid, record.started),
        "helper is still alive"
    );
    assert_eq!(
        descriptor(root.path())?,
        before,
        "background cancellation left a permission entry"
    );
    Ok(())
}
