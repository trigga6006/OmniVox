#!/usr/bin/env powershell

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$SrcTauri = Join-Path $RepoRoot "src-tauri"
$CargoBin = Join-Path $HOME ".cargo\bin"

if (Test-Path $CargoBin) {
    if (-not (($env:PATH -split ";") -contains $CargoBin)) {
        $env:PATH = "$CargoBin;$env:PATH"
    }
}

function Test-ViteReady {
    try {
        $response = Invoke-WebRequest -UseBasicParsing -Uri "http://localhost:1420" -TimeoutSec 2
        return $response.StatusCode -eq 200
    } catch {
        return $false
    }
}

Write-Host ""
Write-Host "OmniVox release dev launcher" -ForegroundColor Cyan
Write-Host "Repo: $RepoRoot" -ForegroundColor DarkGray

if (Test-ViteReady) {
    Write-Host "Vite is already serving on http://localhost:1420" -ForegroundColor Green
} else {
    Write-Host "Starting Vite dev server in a new PowerShell window..." -ForegroundColor Yellow
    Start-Process -FilePath "powershell.exe" -WorkingDirectory $RepoRoot -ArgumentList @(
        "-NoExit",
        "-Command",
        "Set-Location '$RepoRoot'; npm run dev"
    ) | Out-Null

    $ready = $false
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Seconds 1
        if (Test-ViteReady) {
            $ready = $true
            break
        }
    }

    if (-not $ready) {
        throw "Vite did not come up on http://localhost:1420 within 60 seconds."
    }

    Write-Host "Vite is live on http://localhost:1420" -ForegroundColor Green
}

Write-Host "Launching Tauri from release mode with Ninja + Vulkan..." -ForegroundColor Yellow
Write-Host "This runs in the current terminal so you can see native logs." -ForegroundColor DarkGray
Write-Host ""

Push-Location $SrcTauri
try {
    $env:CMAKE_GENERATOR = "Ninja"
    cargo run --release --features vulkan
} finally {
    Pop-Location
}
