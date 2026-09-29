<#
.SYNOPSIS
  Sets up the vendored Rust toolchain (see .gitignore / docs/OVERVIEW.md)
  and runs either the frontend app or the CLI engine, in release mode.

.PARAMETER Target
  "app" (default) - the Tauri desktop app (the drive-picker window).
  "cli"            - the scanner-cli engine.

.PARAMETER Elevate
  Relaunch this script in an elevated (Administrator) PowerShell first, so
  the MFT engine can actually open a raw volume handle. The walk engine
  works fine without this; skip -Elevate if that's all you need.

.PARAMETER CliArgs
  Passed straight through to scanner-cli when -Target cli is used, e.g.
  "D:\" "--children". Not used for -Target app.

.EXAMPLE
  .\start.ps1
.EXAMPLE
  .\start.ps1 -Target app -Elevate
.EXAMPLE
  .\start.ps1 -Target cli D:\ --children
.EXAMPLE
  .\start.ps1 -Target cli D:\ --children -Elevate
#>
param(
    [ValidateSet("app", "cli")]
    [string]$Target = "app",

    [switch]$Elevate,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CliArgs
)

$root = $PSScriptRoot

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltinRole]::Administrator)

if ($Elevate -and -not $isAdmin) {
    $scriptPath = $MyInvocation.MyCommand.Path
    $argString = "-NoExit -File `"$scriptPath`" -Target $Target"
    if ($CliArgs) { $argString += " " + ($CliArgs -join " ") }
    # Note: this doesn't re-quote individual CliArgs, so a path containing
    # spaces won't survive the relaunch. Fine for drive-root paths like
    # "D:\"; pass -Target cli from an already-elevated terminal instead if
    # you need a spaced path.
    Start-Process powershell -Verb RunAs -ArgumentList $argString
    exit
}

$env:CARGO_HOME = Join-Path $root ".toolchain\cargo"
$env:RUSTUP_HOME = Join-Path $root ".toolchain\rustup"

# Only strictly required to build (not just run) the frontend, whose
# tauri-build step shells out to windres for the Windows icon resource.
# Edit this if MinGW/UCRT64 lives somewhere else on your machine.
$mingwBin = "D:\C\ucrt64\bin"
if (Test-Path $mingwBin) {
    $env:PATH = "$mingwBin;$env:CARGO_HOME\bin;$env:PATH"
} else {
    Write-Warning "MinGW not found at $mingwBin - building frontend/ may fail at the windres step. Edit `$mingwBin in this script if it moved."
    $env:PATH = "$env:CARGO_HOME\bin;$env:PATH"
}

Write-Host "Administrator: $isAdmin $(if (-not $isAdmin) { '(MFT engine will fail with Access is denied - rerun with -Elevate for that)' })"

if ($Target -eq "cli") {
    Set-Location (Join-Path $root "engine")
    cargo run --release -p scanner-cli -- @CliArgs
} else {
    Set-Location (Join-Path $root "frontend\src-tauri")
    cargo run --release
}
