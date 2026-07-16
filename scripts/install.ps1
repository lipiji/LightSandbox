# LightSandbox Windows install script
# Usage: iwr https://raw.githubusercontent.com/lipiji/LightSandbox/master/scripts/install.ps1 | iex
#
# Or with a custom install directory:
#   $env:LIGHTSANDBOX_INSTALL_DIR = "C:\Tools"; iwr ... | iex

param(
    [string]$InstallDir = $env:LIGHTSANDBOX_INSTALL_DIR
)

$ErrorActionPreference = "Stop"

$Repo     = "lipiji/LightSandbox"
$BinName  = "lightsandbox-server.exe"
$Artifact = "lightsandbox-server-windows-x86_64.exe"

# ── resolve install directory ────────────────────────────────────────────────

if (-not $InstallDir) {
    $InstallDir = Join-Path $env:LOCALAPPDATA "LightSandbox\bin"
}

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# ── fetch latest version ─────────────────────────────────────────────────────

Write-Host "fetching latest release..."
$Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
$Version = $Release.tag_name

if (-not $Version) {
    Write-Error "could not determine latest release"
    exit 1
}

Write-Host "installing lightsandbox-server $Version..."

$Url  = "https://github.com/$Repo/releases/download/$Version/$Artifact"
$Dest = Join-Path $InstallDir $BinName

Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing

# ── add to user PATH if not already present ──────────────────────────────────

$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable(
        "PATH",
        "$InstallDir;$UserPath",
        "User"
    )
    Write-Host "added $InstallDir to your user PATH"
    Write-Host "(restart your terminal for PATH to take effect)"
}

# ── done ─────────────────────────────────────────────────────────────────────

Write-Host ""
Write-Host "installed lightsandbox-server $Version -> $Dest"
Write-Host ""
Write-Host "get started:"
Write-Host "  lightsandbox-server              # start with built-in defaults"
Write-Host "  lightsandbox-server --help       # show options"
