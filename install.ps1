#!/usr/bin/env pwsh
#
# OmniVox Installer - downloads, verifies, and runs the latest signed release.
#
# Usage:
#   irm https://raw.githubusercontent.com/trigga6006/OmniVox/main/install.ps1 | iex

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Repo = "trigga6006/OmniVox"
$AppName = "OmniVox"
$UserAgent = "OmniVox-Installer"

Write-Host ""
Write-Host "  OmniVox Installer" -ForegroundColor DarkYellow
Write-Host "  Local AI Dictation for Windows" -ForegroundColor DarkYellow
Write-Host ""

Write-Host "  Fetching latest release..." -ForegroundColor Cyan
try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = $UserAgent }
} catch {
    Write-Host "  ERROR: Could not reach GitHub. Check your internet connection." -ForegroundColor Red
    Write-Host "  Details: $_" -ForegroundColor DarkGray
    exit 1
}

$Version = [string]$release.tag_name
if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$') {
    Write-Host "  ERROR: Latest release has an invalid version tag." -ForegroundColor Red
    exit 1
}
Write-Host "  Latest version: $Version" -ForegroundColor Green

$ExpectedInstallerName = "OmniVox-$Version-x86_64-windows-setup.exe"
$ExpectedChecksumName = "OmniVox-$Version-SHA256SUMS.txt"
$installerAsset = @($release.assets | Where-Object { $_.name -ceq $ExpectedInstallerName })
$checksumAsset = @($release.assets | Where-Object { $_.name -ceq $ExpectedChecksumName })
if ($installerAsset.Count -ne 1 -or $checksumAsset.Count -ne 1) {
    Write-Host "  ERROR: Release $Version does not contain one exact installer and checksum manifest." -ForegroundColor Red
    exit 1
}
$installerAsset = $installerAsset[0]
$checksumAsset = $checksumAsset[0]

foreach ($asset in @($installerAsset, $checksumAsset)) {
    $uri = [Uri]$asset.browser_download_url
    $expectedPrefix = "/$Repo/releases/download/"
    if ($uri.Scheme -ne "https" -or $uri.Host -ne "github.com" -or -not $uri.AbsolutePath.StartsWith($expectedPrefix, [StringComparison]::Ordinal)) {
        Write-Host "  ERROR: GitHub returned an unexpected release asset URL." -ForegroundColor Red
        exit 1
    }
}

$FileSize = [math]::Round([int64]$installerAsset.size / 1MB, 1)
Write-Host "  Downloading $ExpectedInstallerName ($FileSize MB)..." -ForegroundColor Cyan

$TempDir = Join-Path ([IO.Path]::GetTempPath()) "omnivox-install-$([Guid]::NewGuid().ToString('N'))"
$InstallerPath = Join-Path $TempDir $ExpectedInstallerName
$ChecksumPath = Join-Path $TempDir $ExpectedChecksumName
New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest -Uri $installerAsset.browser_download_url -OutFile $InstallerPath -Headers @{ "User-Agent" = $UserAgent }
    Invoke-WebRequest -Uri $checksumAsset.browser_download_url -OutFile $ChecksumPath -Headers @{ "User-Agent" = $UserAgent }

    if ((Get-Item -LiteralPath $InstallerPath).Length -ne [int64]$installerAsset.size) {
        throw "Downloaded installer size does not match the release metadata."
    }
    if ((Get-Item -LiteralPath $ChecksumPath).Length -ne [int64]$checksumAsset.size) {
        throw "Downloaded checksum manifest size does not match the release metadata."
    }

    $escapedName = [Regex]::Escape($ExpectedInstallerName)
    $checksumLine = Get-Content -LiteralPath $ChecksumPath | Where-Object { $_ -match "^([0-9a-fA-F]{64}) [ *]$escapedName$" }
    if (@($checksumLine).Count -ne 1) {
        throw "Checksum manifest does not contain exactly one entry for the installer."
    }
    $expectedHash = [Regex]::Match([string]$checksumLine, '^([0-9a-fA-F]{64})').Groups[1].Value.ToLowerInvariant()
    $actualHash = (Get-FileHash -LiteralPath $InstallerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -cne $expectedHash) {
        throw "Installer SHA-256 verification failed."
    }

    # SHA-256 against the published manifest (above) is the mandatory
    # integrity gate. Authenticode is layered on top: a PRESENT-but-invalid
    # signature means tampering and is refused, while an unsigned installer is
    # accepted with a warning — OmniVox releases are not code-signed today,
    # and Windows SmartScreen will show its own unsigned-publisher prompt.
    # If releases gain a certificate later, this verification tightens
    # automatically because the signature will then be present and must be
    # valid.
    $signature = Get-AuthenticodeSignature -LiteralPath $InstallerPath
    if ($signature.Status -eq [System.Management.Automation.SignatureStatus]::Valid) {
        Write-Host "  Verified SHA-256 and Authenticode signature." -ForegroundColor Green
        Write-Host "  Publisher: $($signature.SignerCertificate.Subject)" -ForegroundColor DarkGray
    }
    elseif ($signature.Status -eq [System.Management.Automation.SignatureStatus]::NotSigned) {
        Write-Host "  Verified SHA-256 (installer is not code-signed; expect a SmartScreen prompt)." -ForegroundColor Yellow
    }
    else {
        throw "Installer carries an invalid Authenticode signature ($($signature.Status)) - refusing to run it."
    }
    Write-Host ""
    Write-Host "  Launching installer..." -ForegroundColor Cyan
    Write-Host "  (Follow the installer prompts to complete setup)" -ForegroundColor DarkGray
    Write-Host ""
    Start-Process -FilePath $InstallerPath -Wait
} catch {
    Write-Host "  ERROR: Installation was stopped before executing an unverified artifact." -ForegroundColor Red
    Write-Host "  Details: $_" -ForegroundColor DarkGray
    exit 1
} finally {
    if (Test-Path -LiteralPath $TempDir) {
        Remove-Item -LiteralPath $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "  $AppName $Version installed!" -ForegroundColor Green
Write-Host "  Launch OmniVox from your Start Menu or Desktop." -ForegroundColor Cyan
Write-Host ""
