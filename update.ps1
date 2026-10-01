# Updates an installed s1grep on Windows.
#
#   irm https://raw.githubusercontent.com/apiservicesac/s1grep/main/update.ps1 | iex
#
# Pin a version with $env:S1GREP_VERSION = "0.3.0". It downloads, checks every file against SHA256SUMS, proves the new
# s1grep.exe runs, and only then swaps it in. Models, indexes and settings are kept.

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Repository = "apiservicesac/s1grep"
$Version = if ($env:S1GREP_VERSION) { $env:S1GREP_VERSION } else { "latest" }

function Write-Info($Text)    { Write-Host "→ $Text" -ForegroundColor Cyan }
function Write-Success($Text) { Write-Host "✓ $Text" -ForegroundColor Green }
function Write-Warn($Text)    { Write-Host "! $Text" -ForegroundColor Yellow }
# Errors throw instead of `exit`: piped into iex, `exit` would close the user's PowerShell window.
function Stop-WithError($Text) { throw "✗ $Text" }

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

function Invoke-Updater {
    $command = Get-Command s1grep -ErrorAction SilentlyContinue
    if (-not $command) { Stop-WithError "s1grep is not installed (or not in PATH). Use install.ps1 instead." }
    $target = $command.Source
    $installDir = Split-Path $target
    $current = & $target --version 2>$null
    Write-Info "Installed: $current ($target)"

    if ($script:Version -eq "latest") {
        try {
            $release = Invoke-RestMethod "https://api.github.com/repos/$Repository/releases/latest" -Headers @{ "User-Agent" = "s1grep-updater" }
        } catch {
            Stop-WithError "Could not reach GitHub to find the latest release."
        }
        $script:Version = $release.tag_name
    }
    $script:Version = $script:Version.TrimStart("v")
    if ($current -eq "s1grep $Version") {
        Write-Success "Already on $current."
        return
    }

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

        # Prove it runs before it replaces anything that works.
        $fetched = & (Join-Path $work "s1grep.exe") --version 2>$null
        if ($LASTEXITCODE -ne 0) { Stop-WithError "That download is not a working s1grep. Try again." }

        # The background process still runs the old version and holds the files; stop it first.
        & $target stop *> $null
        Copy-Item -Force (Join-Path $work "onnxruntime.dll") $installDir
        Copy-Item -Force (Join-Path $work "s1grep.exe") $installDir
        Write-Success "Updated $current → $fetched"
    } finally {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }

    # Fetch any model a new version needs; files already present are kept.
    & $target setup *> $null
    if ($LASTEXITCODE -ne 0) { Write-Warn "Could not check the models. Run: s1grep setup" }
}

try {
    Invoke-Updater
} catch {
    Write-Host "`n$($_.Exception.Message)`n" -ForegroundColor Red
}
