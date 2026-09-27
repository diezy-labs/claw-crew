# Script to generate Protobuf code for Go and Rust
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "==> Generating Protobuf Code..." -ForegroundColor Cyan

# 1. Locate protoc compiler
$protocCmd = Get-Command "protoc" -ErrorAction SilentlyContinue
if (-not $protocCmd) {
    # Check WinGet package directory
    $wingetProtoc = Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter "protoc.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($wingetProtoc) {
        $protocDir = Split-Path -Parent $wingetProtoc.FullName
        $env:PATH = "$protocDir;$env:PATH"
    } else {
        # Refresh PATH from system registry
        $machinePath = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
        $userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
        $env:PATH = "$userPath;$machinePath;$env:PATH"
    }
}

# 2. Ensure go/bin is in PATH
$goBin = Join-Path $env:USERPROFILE "go\bin"
if ($env:PATH -notlike "*$goBin*") {
    $env:PATH = "$goBin;$env:PATH"
}

# 3. Generate Go protobuf
$goOut = Join-Path $root "engine\pkg\pb"
if (-not (Test-Path $goOut)) {
    New-Item -ItemType Directory -Path $goOut -Force | Out-Null
}

Write-Host "--> Compiling proto for Go (into engine/pkg/pb)..."
protoc --proto_path=proto `
       --go_out=engine/pkg/pb --go_opt=paths=source_relative `
       --go-grpc_out=engine/pkg/pb --go-grpc_opt=paths=source_relative `
       proto/agent_service.proto

if ($LASTEXITCODE -eq 0) {
    Write-Host "[OK] Go Protobuf generated successfully." -ForegroundColor Green
} else {
    Write-Error "[FAIL] Failed to generate Go Protobuf"
}
