#!/usr/bin/env pwsh
#
# OmniVox Installer — downloads and installs the latest release.
#
# Usage:
#   irm https://raw.githubusercontent.com/trigga6006/OmniVox/main/install.ps1 | iex
#

$ErrorActionPreference = "Stop"

$Repo = "trigga6006/OmniVox"
$AppName = "OmniVox"

Write-Host ""
Write-Host "  ╔══════════════════════════════════════╗" -ForegroundColor DarkYellow
Write-Host "  ║         OmniVox Installer            ║" -ForegroundColor DarkYellow
Write-Host "  ║    Local AI Dictation for Windows    ║" -ForegroundColor DarkYellow
Write-Host "  ╚══════════════════════════════════════╝" -ForegroundColor DarkYellow
Write-Host ""

# 1. Fetch latest release info
Write-Host "  Fetching latest release..." -ForegroundColor Cyan
try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "OmniVox-Installer" }
} catch {
    Write-Host "  ERROR: Could not reach GitHub. Check your internet connection." -ForegroundColor Red
    Write-Host "  Details: $_" -ForegroundColor DarkGray
    exit 1
}

$Version = $release.tag_name
Write-Host "  Latest version: $Version" -ForegroundColor Green

# 2. Find the Windows installer asset
$asset = $release.assets | Where-Object { $_.name -like "*windows-setup.exe" } | Select-Object -First 1

if (-not $asset) {
    # Fallback: look for any .exe asset
    $asset = $release.assets | Where-Object { $_.name -like "*.exe" } | Select-Object -First 1
}

if (-not $asset) {
    Write-Host "  ERROR: No Windows installer found in release $Version" -ForegroundColor Red
    Write-Host "  Available assets:" -ForegroundColor DarkGray
    $release.assets | ForEach-Object { Write-Host "    - $($_.name)" -ForegroundColor DarkGray }
    exit 1
}

$DownloadUrl = $asset.browser_download_url
$FileName = $asset.name
$FileSize = [math]::Round($asset.size / 1MB, 1)

Write-Host "  Downloading $FileName ($FileSize MB)..." -ForegroundColor Cyan

# 3. Download to temp
$TempDir = Join-Path $env:TEMP "omnivox-install"
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
$InstallerPath = Join-Path $TempDir $FileName

try {
    $ProgressPreference = 'SilentlyContinue'  # speeds up Invoke-WebRequest
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $InstallerPath -Headers @{ "User-Agent" = "OmniVox-Installer" }
} catch {
    Write-Host "  ERROR: Download failed." -ForegroundColor Red
    Write-Host "  Details: $_" -ForegroundColor DarkGray
    exit 1
}

Write-Host "  Download complete." -ForegroundColor Green

# 4. Run the installer
Write-Host ""
Write-Host "  Launching installer..." -ForegroundColor Cyan
Write-Host "  (Follow the installer prompts to complete setup)" -ForegroundColor DarkGray
Write-Host ""

try {
    Start-Process -FilePath $InstallerPath -Wait
} catch {
    Write-Host "  ERROR: Failed to launch installer." -ForegroundColor Red
    Write-Host "  Details: $_" -ForegroundColor DarkGray
    Write-Host "  You can manually run: $InstallerPath" -ForegroundColor DarkGray
    exit 1
}

# 5. Cleanup
Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "  ╔══════════════════════════════════════╗" -ForegroundColor Green
Write-Host "  ║   OmniVox $Version installed!       ║" -ForegroundColor Green
Write-Host "  ╚══════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""
Write-Host "  Launch OmniVox from your Start Menu or Desktop." -ForegroundColor Cyan
Write-Host ""
