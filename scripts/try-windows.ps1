# Try the Windows downloads the way a person would. Run after
# scripts\package-windows.ps1, with the same argument.
#
#   scripts\try-windows.ps1 x64|arm64
#
# The zip is unpacked and its program asked its version. The installer is run
# without its windows, a Markdown file is opened the way a double click opens
# it, both copies are told of a newer release and left to update themselves,
# and ZinMD is then removed again. In the release pipeline, a picture of
# the screen with the app open is left in dist\seen\. No picture is taken of
# anyone's own screen.
param([string]$Arch = "x64")

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$inCargo = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches.Groups[1].Value
$version = if ($env:ZINMD_VERSION) { $env:ZINMD_VERSION } else { $inCargo }
$name = "ZinMD-$version-windows-$Arch"
$seen = Join-Path (Get-Location) "dist\seen"
New-Item -ItemType Directory -Force $seen | Out-Null
$work = Join-Path ([System.IO.Path]::GetTempPath()) "zinmd-try-$PID"
New-Item -ItemType Directory -Force $work | Out-Null
$env:ZINMD_DATA = Join-Path $work "data"

function Check($what, $holds) {
    if (-not $holds) { throw "FAILED: $what" }
    Write-Host "ok: $what"
}

# A program made of windows has nowhere to print unless it is given a file.
function Version($program) {
    $said = Join-Path $work "said.txt"
    Start-Process -FilePath $program -ArgumentList "--version" -RedirectStandardOutput $said -Wait -NoNewWindow
    (Get-Content $said -Raw).Trim()
}

function Default($key) {
    (Get-Item -Path $key -ErrorAction SilentlyContinue)?.GetValue("")
}

# The zip: the program on its own.
Expand-Archive "dist\$name.zip" (Join-Path $work "zip")
Check "the program from the zip says it is ZinMD $version" ((Version (Join-Path $work "zip\ZinMD\ZinMD.exe")) -eq "ZinMD $version")

# The installer, for this person alone and without its windows.
$setup = (Resolve-Path "dist\$name-setup.exe").Path
$done = Start-Process -FilePath $setup -Wait -PassThru -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CURRENTUSER", "/LOG=`"$seen\$name-install.log`""
Check "the installer finished" ($done.ExitCode -eq 0)
$installed = Join-Path $env:LOCALAPPDATA "Programs\ZinMD\ZinMD.exe"
Check "the program is where the installer put it" (Test-Path $installed)
Check "the installed program says it is ZinMD $version" ((Version $installed) -eq "ZinMD $version")
Check "a .md file is a ZinMD document" ((Default "HKCU:\Software\Classes\.md") -eq "ZinMD.Markdown")
Check "a ZinMD document opens with the installed program" ((Default "HKCU:\Software\Classes\ZinMD.Markdown\shell\open\command") -eq "`"$installed`" `"%1`"")
$listed = Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*" | Where-Object { $_.DisplayName -eq "ZinMD" }
Check "ZinMD is listed among the installed apps" ($null -ne $listed)

# Open a document the way a double click does: by asking Windows to open the
# file, not by starting ZinMD.
Start-Process (Resolve-Path "samples\roadmap.md").Path
$app = $null
foreach ($moment in 1..40) {
    Start-Sleep -Milliseconds 500
    $app = Get-Process -Name ZinMD -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($app) { break }
}
Check "opening a .md file started ZinMD" ($null -ne $app)
Start-Sleep -Seconds 10
$app.Refresh()
Check "ZinMD stayed open" (-not $app.HasExited)
Check "ZinMD has a window" ($app.MainWindowHandle -ne 0)
if ($env:CI) { try {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $picture = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
    $pen = [System.Drawing.Graphics]::FromImage($picture)
    $pen.CopyFromScreen($screen.Location, [System.Drawing.Point]::Empty, $screen.Size)
    $picture.Save((Join-Path $seen "$name-window.png"))
} catch {
    Write-Host "note: no picture of the screen could be taken: $_"
} }
Stop-Process -Id $app.Id -Force
$app.WaitForExit()

# Updates. A copy is told of a release newer than itself: these same
# downloads, under a later number. It is left to take that in with nobody
# there to click, and is expected to be replaced and to be running again.
function Update($program, $what) {
    $list = Join-Path $work "latest.json"
    python scripts\release-list.py 99.0.0 dist | Set-Content -Encoding utf8NoBOM $list
    $env:ZINMD_UPDATE_FEED = $list
    $env:ZINMD_UPDATE = "now"
    $first = Start-Process -FilePath $program -ArgumentList "`"$((Resolve-Path "samples\roadmap.md").Path)`"" -PassThru
    $env:ZINMD_UPDATE = $null
    $env:ZINMD_UPDATE_FEED = $null
    Check "$what left to make room for the new version" ($first.WaitForExit(180000))
    $second = $null
    foreach ($moment in 1..240) {
        Start-Sleep -Milliseconds 500
        $second = Get-Process -Name ZinMD -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $first.Id } | Select-Object -First 1
        if ($second) { break }
    }
    Check "$what started again by itself" ($null -ne $second)
    Start-Sleep -Seconds 5
    $second.Refresh()
    Check "$what stayed open after updating" (-not $second.HasExited)
    Check "$what still says it is ZinMD $version" ((Version $program) -eq "ZinMD $version")
    Stop-Process -Id $second.Id -Force
    $second.WaitForExit()
}
Update $installed "the installed copy"
Update (Join-Path $work "zip\ZinMD\ZinMD.exe") "the copy from the zip"

# Take it away again, and see that it is gone.
$remover = Join-Path (Split-Path $installed) "unins000.exe"
Start-Process -FilePath $remover -Wait -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"
foreach ($moment in 1..40) {
    if (-not (Test-Path $installed)) { break }
    Start-Sleep -Milliseconds 500
}
Check "removing ZinMD removes the program" (-not (Test-Path $installed))
Check "removing ZinMD gives .md files back" ((Default "HKCU:\Software\Classes\.md") -ne "ZinMD.Markdown")

Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Write-Host "the Windows downloads for $Arch work"
