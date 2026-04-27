#Requires -RunAsAdministrator
<#
.SYNOPSIS
    MCProxy Server — Windows one-click installer
    Detects deps, builds, registers Windows service via NSSM or Task Scheduler, starts
#>

$ErrorActionPreference = "Stop"

$ServiceName   = "MCProxyServer"
$InstallDir    = "$env:ProgramData\MCProxy"
$BinName       = "mcproxy-server.exe"
$BindAddr      = if ($env:BIND_ADDR) { $env:BIND_ADDR } else { "0.0.0.0:9800" }
$Port          = ($BindAddr -split ":")[1]
$LogFile       = "$InstallDir\mcproxy.log"

function Write-Info  { Write-Host "[INFO] $args" -ForegroundColor Cyan }
function Write-Ok    { Write-Host "[  OK] $args" -ForegroundColor Green }
function Write-Warn  { Write-Host "[WARN] $args" -ForegroundColor Yellow }
function Write-Fail  { Write-Host "[FAIL] $args" -ForegroundColor Red; exit 1 }

Write-Host ""
Write-Host "  ╔══════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "  ║  MCProxy Server Installer (Windows)  ║" -ForegroundColor Cyan
Write-Host "  ╚══════════════════════════════════════╝" -ForegroundColor Cyan
Write-Host ""

# ── 1. Check & install Rust ─────────────────
Write-Info "Checking Rust toolchain..."
$cargoPath = (Get-Command cargo -ErrorAction SilentlyContinue).Source

if ($cargoPath) {
    $ver = & cargo --version
    Write-Ok "Rust found: $ver"
} else {
    Write-Info "Rust not found. Installing via rustup..."
    $rustupUrl = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"
    $rustupExe = "$env:TEMP\rustup-init.exe"
    Invoke-WebRequest -Uri $rustupUrl -OutFile $rustupExe -UseBasicParsing
    & $rustupExe -y
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    Write-Ok "Rust installed"
}

# ── 2. Check Visual Studio Build Tools ──────
Write-Info "Checking C++ build tools..."
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasBuildTools = $false

if (Test-Path $vsWhere) {
    $result = & $vsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($result) { $hasBuildTools = $true }
}

if ($hasBuildTools) {
    Write-Ok "Visual Studio Build Tools found"
} else {
    Write-Info "Installing Visual Studio Build Tools..."
    $vsUrl = "https://aka.ms/vs/17/release/vs_buildtools.exe"
    $vsExe = "$env:TEMP\vs_buildtools.exe"
    Invoke-WebRequest -Uri $vsUrl -OutFile $vsExe -UseBasicParsing
    Write-Info "Launching installer (select C++ build tools)..."
    Start-Process -FilePath $vsExe -ArgumentList "--add", "Microsoft.VisualStudio.Workload.VCTools", "--quiet", "--wait" -Wait
    Write-Ok "Build Tools installed"
}

# ── 3. Build ────────────────────────────────
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Write-Info "Building release binary..."
Push-Location $ScriptDir
& cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Fail "Build failed" }
Pop-Location
Write-Ok "Build complete"

$BuiltBin = Join-Path $ScriptDir "target\release\$BinName"

# ── 4. Install binary ──────────────────────
Write-Info "Installing to $InstallDir ..."
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
Copy-Item $BuiltBin "$InstallDir\$BinName" -Force
Write-Ok "Binary installed: $InstallDir\$BinName"

# ── 5. Register as scheduled task (runs at logon, restarts on failure) ──
Write-Info "Checking service registration..."
$existingTask = Get-ScheduledTask -TaskName $ServiceName -ErrorAction SilentlyContinue

if ($existingTask) {
    Write-Warn "Task '$ServiceName' already exists"
    Write-Info "Stopping and updating..."
    Stop-ScheduledTask -TaskName $ServiceName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $ServiceName -Confirm:$false
}

Write-Info "Registering scheduled task..."

$action = New-ScheduledTaskAction `
    -Execute "$InstallDir\$BinName" `
    -WorkingDirectory $InstallDir

$trigger = New-ScheduledTaskTrigger -AtStartup

$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit (New-TimeSpan -Days 9999) `
    -RestartCount 999 `
    -RestartInterval (New-TimeSpan -Seconds 10)

$principal = New-ScheduledTaskPrincipal `
    -UserId "SYSTEM" `
    -LogonType ServiceAccount `
    -RunLevel Highest

Register-ScheduledTask `
    -TaskName $ServiceName `
    -Action $action `
    -Trigger $trigger `
    -Settings $settings `
    -Principal $principal `
    -Description "MCProxy Relay Server" | Out-Null

Write-Ok "Task registered: $ServiceName"

# ── 6. Set environment variable for the task ──
$taskXml = Export-ScheduledTask -TaskName $ServiceName
$doc = [xml]$taskXml

$ns = New-Object System.Xml.XmlNamespaceManager($doc.NameTable)
$ns.AddNamespace("t", "http://schemas.microsoft.com/windows/2004/02/mit/task")

$execNode = $doc.SelectSingleNode("//t:Exec", $ns)
$argsNode = $execNode.SelectSingleNode("t:Arguments", $ns)
if (-not $argsNode) {
    $argsNode = $doc.CreateElement("Arguments", "http://schemas.microsoft.com/windows/2004/02/mit/task")
    $execNode.AppendChild($argsNode) | Out-Null
}

# Store bind addr in a wrapper script
$wrapperScript = "$InstallDir\start.cmd"
@"
@echo off
set BIND_ADDR=$BindAddr
set RUST_LOG=info
"$InstallDir\$BinName" >> "$LogFile" 2>&1
"@ | Out-File -FilePath $wrapperScript -Encoding ASCII

$action2 = New-ScheduledTaskAction -Execute $wrapperScript -WorkingDirectory $InstallDir
Set-ScheduledTask -TaskName $ServiceName -Action $action2 | Out-Null

# ── 7. Start ────────────────────────────────
Write-Info "Starting service..."
Start-ScheduledTask -TaskName $ServiceName
Start-Sleep -Seconds 2

$taskInfo = Get-ScheduledTask -TaskName $ServiceName
if ($taskInfo.State -eq "Running") {
    Write-Ok "Service running"
} else {
    Write-Warn "Task state: $($taskInfo.State) — check log: $LogFile"
}

# ── 8. Firewall ─────────────────────────────
Write-Info "Checking firewall..."
$fwRule = Get-NetFirewallRule -DisplayName "MCProxy Server" -ErrorAction SilentlyContinue
if ($fwRule) {
    Write-Ok "Firewall rule already exists"
} else {
    New-NetFirewallRule `
        -DisplayName "MCProxy Server" `
        -Direction Inbound `
        -Protocol TCP `
        -LocalPort $Port `
        -Action Allow | Out-Null
    Write-Ok "Firewall rule added: TCP $Port inbound"
}

# ── 9. Get local IP ─────────────────────────
$localIP = (Get-NetIPAddress -AddressFamily IPv4 |
    Where-Object { $_.InterfaceAlias -notmatch "Loopback" -and $_.PrefixOrigin -eq "Dhcp" } |
    Select-Object -First 1).IPAddress

if (-not $localIP) { $localIP = "YOUR_IP" }

Write-Host ""
Write-Host "  ════════════════════════════════════════" -ForegroundColor Green
Write-Host "    MCProxy server is running!" -ForegroundColor Green
Write-Host "    Listening on: $BindAddr" -ForegroundColor Green
Write-Host "  ════════════════════════════════════════" -ForegroundColor Green
Write-Host ""
Write-Host "  Commands (PowerShell as Admin):"
Write-Host "    Get-ScheduledTask -TaskName $ServiceName    # status"
Write-Host "    Stop-ScheduledTask -TaskName $ServiceName   # stop"
Write-Host "    Start-ScheduledTask -TaskName $ServiceName  # start"
Write-Host "    Get-Content '$LogFile' -Tail 20             # logs"
Write-Host ""
Write-Host "  Clients connect to: ws://${localIP}:${Port}"
Write-Host ""
Write-Host "  Uninstall:"
Write-Host "    Stop-ScheduledTask -TaskName $ServiceName"
Write-Host "    Unregister-ScheduledTask -TaskName $ServiceName -Confirm:`$false"
Write-Host "    Remove-NetFirewallRule -DisplayName 'MCProxy Server'"
Write-Host "    Remove-Item -Recurse '$InstallDir'"
Write-Host ""
