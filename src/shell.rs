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

#[cfg(unix)]
pub(crate) fn program() -> Result<String> {
    Ok("bash".to_string())
}

#[cfg(windows)]
pub(crate) fn program() -> Result<String> {
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

#[cfg(unix)]
pub(crate) fn arguments(script: &str, _: &Path) -> Vec<String> {
    vec!["-c".to_string(), script.to_string()]
}

#[cfg(windows)]
pub(crate) fn arguments(script: &str, cwd: &Path) -> Vec<String> {
    vec![
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-OutputFormat".to_string(),
        "Text".to_string(),
        "-EncodedCommand".to_string(),
        encode(script, cwd),
    ]
}

#[cfg(any(unix, test))]
pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(unix)]
pub(crate) fn wrap(command: &str, exit: &Path, _: &Path) -> String {
    let exit = quote(&exit.to_string_lossy());
    format!(
        "trap '' HUP\n(\n{command}\nsummonr_status=$?\nwait\nexit \"$summonr_status\"\n)\ncode=$?\nprintf '%s\\n' \"$code\" > {exit}\nexit \"$code\""
    )
}

#[cfg(windows)]
pub(crate) fn wrap(command: &str, exit: &Path, cwd: &Path) -> String {
    let exit = simplified(exit).to_string_lossy().replace('\'', "''");
    let command = encode(command, cwd);
    format!(
        "& (Join-Path $PSHOME 'powershell.exe') -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand '{command}'\n$code = $LASTEXITCODE\n[System.IO.File]::WriteAllText('{exit}', \"$code`n\")\nexit $code"
    )
}

#[cfg(windows)]
fn encode(script: &str, cwd: &Path) -> String {
    let cwd = simplified(cwd).to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ProgressPreference = 'SilentlyContinue'\n[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()\ntry {{\nSet-Location -LiteralPath '{cwd}' -ErrorAction Stop\n[Environment]::CurrentDirectory = '{cwd}'\n}} catch {{\n[Console]::Error.WriteLine($_.Exception.Message)\nexit 1\n}}\n$global:LASTEXITCODE = $null\n$global:SummonrOther = $false\ntry {{\n& {{\n{script}\n$global:SummonrStatus = $?\n$global:SummonrCode = $global:LASTEXITCODE\n}} 2>&1 | ForEach-Object {{ if ($_ -is [System.Management.Automation.ErrorRecord]) {{ if ($_.FullyQualifiedErrorId -like 'NativeCommandError*') {{ [Console]::Error.WriteLine($_.Exception.Message) }} else {{ $global:SummonrOther = $true; [Console]::Error.WriteLine(($_ | Out-String).TrimEnd()) }} }} else {{ $_ }} }}\n}} catch {{\n[Console]::Error.WriteLine(($_ | Out-String).TrimEnd())\nexit 1\n}}\nif ($global:SummonrStatus -eq $false) {{ if ($null -ne $global:SummonrCode -and -not $global:SummonrOther) {{ exit $global:SummonrCode }}; exit 1 }}"
    );
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    STANDARD.encode(bytes)
}
