# Build ZinMD for Windows and wrap it the two ways it is given out: an
# installer, and a zip of the program on its own that runs from anywhere.
#
#   scripts\package-windows.ps1          for Intel and AMD processors
#   scripts\package-windows.ps1 arm64    for ARM processors
#
# Needs Inno Setup for the installer. The files land in dist\.
param([string]$Arch = "x64")

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

switch ($Arch) {
    "x64" { $target = "x86_64-pc-windows-msvc"; $allowed = "x64compatible" }
    "arm64" { $target = "aarch64-pc-windows-msvc"; $allowed = "arm64" }
    default { Write-Error "usage: scripts\package-windows.ps1 x64|arm64" }
}

# The version being released when a release is being made, otherwise the one
# in Cargo.toml. Windows wants plain numbers in places: 0.1.0, not 0.1.0-rc.1.
$inCargo = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches.Groups[1].Value
$version = if ($env:ZINMD_VERSION) { $env:ZINMD_VERSION } else { $inCargo }
$numeric = $version.Split("-")[0]
$name = "ZinMD-$version-windows-$Arch"

rustup target add $target
cargo build --locked --profile dist --target $target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$program = (Resolve-Path "target\$target\dist\zinmd.exe").Path

New-Item -ItemType Directory -Force dist | Out-Null
Remove-Item -Force -ErrorAction SilentlyContinue "dist\$name.zip", "dist\$name-setup.exe"

# The program alone, for those who would rather not install: unzip and run.
$stage = Join-Path ([System.IO.Path]::GetTempPath()) "zinmd-$PID"
New-Item -ItemType Directory -Force "$stage\ZinMD" | Out-Null
Copy-Item $program "$stage\ZinMD\ZinMD.exe"
Copy-Item LICENSE "$stage\ZinMD\LICENSE.txt"
Compress-Archive -Path "$stage\ZinMD" -DestinationPath "dist\$name.zip"
Remove-Item -Recurse -Force $stage

# The installer.
$compiler = Get-Command iscc -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $compiler) {
    $compiler = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
}
if (-not (Test-Path $compiler)) {
    Write-Error "Inno Setup was not found. Install it from https://jrsoftware.org/isdl.php"
}
& $compiler /Qp "/DAppVersion=$version" "/DNumericVersion=$numeric" "/DOutputName=$name-setup" "/DBinary=$program" "/DArch=$allowed" packaging\windows\zinmd.iss
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "packaged dist\$name-setup.exe and dist\$name.zip"
