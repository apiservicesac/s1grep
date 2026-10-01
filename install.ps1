# s1grep installer for Windows.
#
#   irm https://raw.githubusercontent.com/apiservicesac/s1grep/main/install.ps1 | iex
#
# Installs the latest release for the current user, without administrator rights: s1grep.exe and the onnxruntime.dll
# it needs go to %LOCALAPPDATA%\Programs\s1grep, which is added to the user's PATH, and the models are downloaded
# (about 2.9 GB, once). Every download is checked against the release's SHA256SUMS and the new s1grep.exe is proven to
# run before anything is replaced. Running it again updates s1grep in place.
#
# Options, through environment variables when piped into iex:
#   $env:S1GREP_VERSION = "0.2.0"      install a specific release instead of the latest
#   $env:S1GREP_NO_MODELS = "1"        do not download the models now (run `s1grep setup` later)
#   $env:S1GREP_UNINSTALL = "1"        remove s1grep; with $env:S1GREP_PURGE = "1" also its models, indexes and settings

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# ══════════════════════════════════════════════════════════════════════════════
# Settings
# ══════════════════════════════════════════════════════════════════════════════
$Repository = "apiservicesac/s1grep"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\s1grep"
$Target = Join-Path $InstallDir "s1grep.exe"
$Version = if ($env:S1GREP_VERSION) { $env:S1GREP_VERSION } else { "latest" }
$DataDirs = @((Join-Path $env:LOCALAPPDATA "s1grep"), (Join-Path $env:APPDATA "s1grep"))

# ══════════════════════════════════════════════════════════════════════════════
# Output
# ══════════════════════════════════════════════════════════════════════════════
function Write-Info($Text)    { Write-Host "→ $Text" -ForegroundColor Cyan }
function Write-Success($Text) { Write-Host "✓ $Text" -ForegroundColor Green }
function Write-Warn($Text)    { Write-Host "! $Text" -ForegroundColor Yellow }
function Write-Step($Text)    { Write-Host "`n── $Text ──" }
# Errors throw instead of `exit`: piped into iex, `exit` would close the user's PowerShell window.
function Stop-WithError($Text) { throw "✗ $Text" }

function Stop-Background {
    if (Test-Path $Target) { & $Target stop *> $null }
}

# ══════════════════════════════════════════════════════════════════════════════
# Uninstall
# ══════════════════════════════════════════════════════════════════════════════
function Uninstall-S1grep {
    Write-Step "Removing s1grep"
    Stop-Background
    Remove-Item -Recurse -Force $InstallDir -ErrorAction SilentlyContinue
    $path = [Environment]::GetEnvironmentVariable("Path", "User")
    $kept = ($path -split ";" | Where-Object { $_ -and $_ -ne $InstallDir }) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $kept, "User")
    Write-Success "Removed $InstallDir"
    if ($env:S1GREP_PURGE -eq "1") {
        $DataDirs | ForEach-Object { Remove-Item -Recurse -Force $_ -ErrorAction SilentlyContinue }
        Write-Success "Removed the models, indexes and settings"
    } else {
        Write-Info "Models and indexes stay in $($DataDirs[0]); set S1GREP_PURGE=1 to delete them too."
    }
}

# ══════════════════════════════════════════════════════════════════════════════
# Download, check and install
# ══════════════════════════════════════════════════════════════════════════════
function Resolve-Version {
    if ($script:Version -eq "latest") {
        try {
            $release = Invoke-RestMethod "https://api.github.com/repos/$Repository/releases/latest" -Headers @{ "User-Agent" = "s1grep-installer" }
        } catch {
            Stop-WithError "Could not reach GitHub to find the latest release."
        }
        $script:Version = $release.tag_name
    }
    $script:Version = $script:Version.TrimStart("v")
}

function Get-Checked($Base, $Name, $Destination, $Sums) {
    Write-Info "Downloading $Name..."
    try {
        Invoke-WebRequest "$Base/$Name" -OutFile $Destination -UseBasicParsing
    } catch {
        Stop-WithError "Could not download $Base/$Name"
    }
    $expected = ($Sums | Where-Object { $_ -match "\s\*?$([regex]::Escape($Name))$" } | Select-Object -First 1) -split "\s+" | Select-Object -First 1
    $actual = (Get-FileHash -Algorithm SHA256 $Destination).Hash.ToLower()
    if (-not $expected -or $actual -ne $expected.ToLower()) {
        Stop-WithError "$Name does not match SHA256SUMS: it arrived incomplete or changed. Try again."
    }
}

function Install-Binary {
    Write-Step "Installing s1grep $Version"
    $base = "https://github.com/$Repository/releases/download/$Version"
    $work = Join-Path ([IO.Path]::GetTempPath()) ("s1grep-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Force $work | Out-Null
    try {
        try {
            $sums = (Invoke-WebRequest "$base/SHA256SUMS" -UseBasicParsing).Content -split "`n"
        } catch {
            Stop-WithError "Release $Version not found (https://github.com/$Repository/releases)."
        }
        Get-Checked $base "s1grep-$Version-x86_64-windows.exe" (Join-Path $work "s1grep.exe") $sums
        Get-Checked $base "onnxruntime.dll" (Join-Path $work "onnxruntime.dll") $sums

        # Prove it runs before it replaces anything.
        $installed = & (Join-Path $work "s1grep.exe") --version 2>$null
        if ($LASTEXITCODE -ne 0) { Stop-WithError "The downloaded s1grep.exe does not run on this machine." }

        New-Item -ItemType Directory -Force $InstallDir | Out-Null
        Stop-Background
        Copy-Item -Force (Join-Path $work "onnxruntime.dll") $InstallDir
        Copy-Item -Force (Join-Path $work "s1grep.exe") $InstallDir
        Write-Success "Installed $installed in $InstallDir"
    } finally {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }
}

function Add-ToPath {
    $path = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($path -split ";") -contains $InstallDir) { return }
    [Environment]::SetEnvironmentVariable("Path", (($path, $InstallDir) | Where-Object { $_ }) -join ";", "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Success "Added $InstallDir to your PATH (new terminals pick it up)"
}

function Install-Models {
    Write-Step "Models"
    if ($env:S1GREP_NO_MODELS -eq "1") {
        Write-Info "Skipped. Run ``s1grep setup`` before the first search."
        return
    }
    & $Target setup
}

# ══════════════════════════════════════════════════════════════════════════════
# Main
# ══════════════════════════════════════════════════════════════════════════════
function Invoke-Installer {
    Write-Host "`n  s1grep · find code by asking what it does`n"
    if ($env:S1GREP_UNINSTALL -eq "1") { Uninstall-S1grep; return }
    if (-not [Environment]::Is64BitOperatingSystem) { Stop-WithError "s1grep needs 64-bit Windows." }
    Resolve-Version
    Install-Binary
    Add-ToPath
    Install-Models

    Write-Step "Ready"
    Write-Host '  s1grep "where do we retry a failed payment" C:\path\to\repo'
    Write-Host "  s1grep status            what is loaded and indexed"
    Write-Host "  s1grep skill --install   teach Claude Code to use it`n"
}

try {
    Invoke-Installer
} catch {
    Write-Host "`n$($_.Exception.Message)`n" -ForegroundColor Red
}
