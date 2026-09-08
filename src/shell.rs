use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
#[cfg(windows)]
use dunce::simplified;

#[cfg(windows)]
use crate::error::Error;
use crate::error::Result;

pub(crate) fn program() -> Result<String> {
    #[cfg(unix)]
    {
        Ok("bash".to_string())
    }
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot")
            .ok_or_else(|| Error::Config("SystemRoot is missing".to_string()))?;
        let path = PathBuf::from(root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if !path.is_file() {
            return Err(Error::Config(
                "Windows PowerShell is unavailable".to_string(),
            ));
        }
        path.into_os_string().into_string().map_err(|path| {
            Error::Config(format!(
                "PowerShell path is not valid Unicode: {}",
                path.to_string_lossy()
            ))
        })
    }
}

pub(crate) fn arguments(script: &str, _cwd: &Path) -> Vec<String> {
    #[cfg(unix)]
    {
        vec!["-c".to_string(), script.to_string()]
    }
    #[cfg(windows)]
    {
        vec![
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-OutputFormat".to_string(),
            "Text".to_string(),
            "-EncodedCommand".to_string(),
            encode(script, _cwd),
        ]
    }
}

pub(crate) fn wrap(command: &str, exit: &Path, _cwd: &Path) -> String {
    #[cfg(unix)]
    {
        let exit = exit.to_string_lossy().replace('\'', "'\\''");
        format!(
            "trap '' HUP\n(\n{command}\n)\ncode=$?\nprintf '%s\\n' \"$code\" > '{exit}'\nexit \"$code\""
        )
    }
    #[cfg(windows)]
    {
        let exit = simplified(exit).to_string_lossy().replace('\'', "''");
        let command = encode(command, _cwd);
        format!(
            "& (Join-Path $PSHOME 'powershell.exe') -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand '{command}'\n$code = $LASTEXITCODE\n[System.IO.File]::WriteAllText('{exit}', \"$code`n\")\nexit $code"
        )
    }
}

#[cfg(windows)]
fn encode(script: &str, cwd: &Path) -> String {
    let cwd = simplified(cwd).to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ProgressPreference = 'SilentlyContinue'\n[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()\ntry {{\nif ($env:SUMMONER_SANDBOX_ROOT) {{\nNew-PSDrive -Name Workspace -PSProvider FileSystem -Root $env:SUMMONER_SANDBOX_ROOT -ErrorAction Stop | Out-Null\nSet-Location -LiteralPath ('Workspace:\\' + $env:SUMMONER_SANDBOX_DIRECTORY) -ErrorAction Stop\n}} else {{\nSet-Location -LiteralPath '{cwd}' -ErrorAction Stop\n}}\n[Environment]::CurrentDirectory = '{cwd}'\n}} catch {{\n[Console]::Error.WriteLine($_.Exception.Message)\nexit 1\n}}\n{script}"
    );
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    STANDARD.encode(bytes)
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
