$ErrorActionPreference = 'Stop'

$Repo = 'manaforged/summonr'
$Releases = "https://github.com/$Repo/releases"
$Installer = 'https://summonr.dev/install.ps1'
$Target = 'x86_64-pc-windows-msvc'
$Dir = if ($env:SUMMONR_INSTALL_DIR) { $env:SUMMONR_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\summonr\bin' }
$Version = if ($env:SUMMONR_VERSION) { $env:SUMMONR_VERSION } else { 'latest' }

function Fail($Message) {
    throw "summonr install: $Message"
}

$Arch = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment').PROCESSOR_ARCHITECTURE
if ($Arch -notin @('AMD64', 'ARM64')) {
    Fail "unsupported CPU $Arch. summonr ships an x64 build for Windows, which also runs on ARM64 Windows 11. See $Releases for every build."
}

$Name = "summonr-$Target"
$Asset = "$Name.zip"
if ($Version -eq 'latest') {
    $Base = "$Releases/latest/download"
} else {
    $Version = 'v' + $Version.TrimStart('v')
    $Base = "$Releases/download/$Version"
}

[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'

$Work = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $Work | Out-Null
try {
    Write-Host "Downloading $Asset ($Version)"
    $Archive = Join-Path $Work $Asset
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "$Base/$Asset" -OutFile $Archive
        Invoke-WebRequest -UseBasicParsing -Uri "$Base/$Asset.sha256" -OutFile "$Archive.sha256"
    } catch {
        Fail "cannot download $Base/$Asset. See $Releases for available builds, or build from source: cargo install --git https://github.com/$Repo --locked (Rust 1.98, Git, CMake, and a C/C++ compiler required)."
    }

    $Want = ((Get-Content "$Archive.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
    $Got = (Get-FileHash -Algorithm SHA256 -Path $Archive).Hash.ToLower()
    if ($Want -ne $Got) {
        Fail "checksum mismatch for ${Asset}: expected $Want, got $Got. Nothing was installed."
    }

    Expand-Archive -Path $Archive -DestinationPath $Work -Force
    New-Item -ItemType Directory -Path $Dir -Force | Out-Null
    $Exe = Join-Path $Dir 'summonr.exe'
    Copy-Item -Path (Join-Path $Work "$Name\summonr.exe") -Destination $Exe -Force
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}

$Installed = try { (& $Exe --version) -join ' ' } catch { "summonr $Version" }
Write-Host "Installed $Installed to $Exe"

$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$Entries = if ($UserPath) { $UserPath -split ';' | Where-Object { $_ } } else { @() }
if ($Entries -notcontains $Dir) {
    [Environment]::SetEnvironmentVariable('Path', (@($Entries) + $Dir) -join ';', 'User')
    Write-Host "Added $Dir to your user Path. Open a new terminal to use summonr."
}
Write-Host "Update: irm $Installer | iex"
Write-Host "Uninstall: Remove-Item `"$Exe`""
Write-Host 'Next: run summonr, then /login to pick a provider and add a key or sign in to a plan.'
