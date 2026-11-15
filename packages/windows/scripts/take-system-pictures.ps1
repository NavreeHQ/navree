# Walks the demo's place in Windows and writes what each part answered, with a picture of the screen after each:
# the count on the taskbar, the icon in the notification area and the small window by it, settings, opening at sign-in,
# a notification, a window of its own, the full screen, the offer to start again, the title bar's search, the first
# load failing. It then tries what comes from outside on the running app: a second start, and an address on its scheme.
#
# Without -Packaged the demo is started from its build folder (no package identity). With -Packaged the demo's package
# project is registered with the winapp command, which gives the app package identity, and started the same way.
#
#   powershell -File take-system-pictures.ps1 -Exe <Navree.Demo.exe> -OutDir <folder>
#   powershell -File take-system-pictures.ps1 -Project <Navree.Demo.Packaged.csproj> -Packaged -OutDir <folder>
param(
    [string]$Exe = '',
    [string]$Project = '',
    [switch]$Packaged,
    [Parameter(Mandatory = $true)][string]$OutDir
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$report = Join-Path $OutDir 'report.txt'
function Say([string]$line) {
    Write-Host $line
    Add-Content -Path $report -Value $line
}

Add-Type -AssemblyName System.Windows.Forms
Say ("== the session: user " + [Environment]::UserName + ", session " + (Get-Process -Id $PID).SessionId + ", " + $(if ($Packaged) { 'with package identity' } else { 'with no package' }))
foreach ($size in @(@(1920, 1080), @(1600, 900), @(1366, 768))) {
    try {
        Set-DisplayResolution -Width $size[0] -Height $size[1] -Force -ErrorAction Stop | Out-Null
        Start-Sleep -Seconds 2
        $now = [System.Windows.Forms.SystemInformation]::PrimaryMonitorSize
        Say ("the screen is " + $now)
        if ($now.Width -eq $size[0]) { break }
    } catch { Say ("Set-DisplayResolution " + $size[0] + ": failed: " + $_.Exception.Message) }
}

# Starts the demo with arguments; returns the process (for a package, the process the winapp command started).
function Start-Demo([string[]]$Arguments, [bool]$Build) {
    if (-not $Packaged) {
        $folder = Split-Path -Parent $Exe
        return Start-Process -FilePath $Exe -ArgumentList $Arguments -WorkingDirectory $folder -PassThru
    }
    $call = @('run', $Project, '-c', 'Release', '--arch', 'x64', '--detach', '--json')
    if (-not $Build) { $call += '--no-build' }
    $call += '--'
    $call += $Arguments
    $out = (& winapp @call 2>&1 | Out-String)
    Say ("winapp run: " + (($out -replace "`r?`n", ' | ').Substring(0, [Math]::Min(1500, $out.Length))))
    if ($out -match '"(?:processId|pid|ProcessId|Pid)"\s*:\s*(\d+)') {
        return Get-Process -Id ([int]$Matches[1]) -ErrorAction SilentlyContinue
    }
    return $null
}

function Wait-Demo($Process, [int]$Seconds) {
    if ($null -eq $Process) { return 'no process' }
    if ($Process.WaitForExit($Seconds * 1000)) { return ("ended by itself, exit code " + $Process.ExitCode) }
    Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
    return "did not end in $Seconds seconds"
}

$first = $true
$walks = @(
    @{ Name = 'light'; Arguments = @('--light') },
    @{ Name = 'dark'; Arguments = @('--dark') },
    @{ Name = 'light-rtl'; Arguments = @('--light', '--rtl') },
    @{ Name = 'search'; Arguments = @('--light', '--search') },
    @{ Name = 'offline'; Arguments = @('--light', '--offline') }
)
foreach ($walk in $walks) {
    Say ""
    Say ("== the walk " + $walk.Name)
    $arguments = @('--system-walk', $OutDir) + $walk.Arguments
    $process = Start-Demo $arguments $first
    $first = $false
    Say ("the walk " + (Wait-Demo $process 240))
}

# What comes from outside, tried on the app while it runs as it would for a person.
Say ""
Say "== a second start, and an address on the app's scheme, tried on the running app"
$probeFile = Join-Path $OutDir 'probe.txt'
$running = Start-Demo @('--probe', $probeFile, '--light') $false
if ($null -ne $running) {
    $ready = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Seconds 1
        $running.Refresh()
        if ($running.HasExited) { break }
        if ($running.MainWindowHandle -ne [IntPtr]::Zero) { $ready = $true; break }
    }
    Say ("the first copy: window " + $ready + ", still running " + (-not $running.HasExited))
    Start-Sleep -Seconds 6
    if ($Packaged) {
        # A package is started again through its identity: from the Start menu's own entry, and by an address on its scheme.
        $family = (Get-AppxPackage -Name 'Navree.Demo' -ErrorAction SilentlyContinue | Select-Object -First 1).PackageFamilyName
        Say ("the package family: " + $family)
        if ($family) {
            Start-Process "shell:AppsFolder\$family!App"
            Say "the app was started again from its entry in the Start menu"
        }
        Start-Sleep -Seconds 6
        try {
            Start-Process 'navree-demo://signed-in?code=abc&state=xyz' -ErrorAction Stop
            Say "an address on the scheme was opened"
        } catch { Say ("the address on the scheme did not open: " + $_.Exception.Message) }
        Start-Sleep -Seconds 8
    } else {
        $second = Start-Demo @('--open=%2Forders') $false
        Say ("a second start with a path: " + (Wait-Demo $second 40))
        try {
            Start-Process 'navree-demo://signed-in?code=abc&state=xyz' -ErrorAction Stop
            Say "an address on the scheme was opened"
        } catch { Say ("the address on the scheme did not open: " + $_.Exception.Message) }
        Start-Sleep -Seconds 8
        $third = Start-Demo @('--new-window') $false
        Say ("a second start asking for a window: " + (Wait-Demo $third 40))
    }
    Say ("the first copy then: " + (Wait-Demo $running 60))
} else {
    Say "the first copy did not start"
}

Say ""
Say "== what the walks and the probe wrote"
Get-ChildItem $OutDir -Filter '*.txt' | ForEach-Object {
    if ($_.Name -ne 'report.txt') {
        Say ("--- " + $_.Name)
        Get-Content $_.FullName | ForEach-Object { Say ("    " + $_) }
    }
}
Get-ChildItem $OutDir -Filter '*.png' | ForEach-Object { Say ($_.Name + " " + $_.Length) }
exit 0
