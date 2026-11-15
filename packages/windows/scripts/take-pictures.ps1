# Starts the demo from its build folder, and takes a picture of its window every
# way there is, writing what each way did to report.txt. A way of taking a picture
# that fails is a result, not an error: the run goes on and the report says which
# ways worked. What is an error, and ends the script with exit code 1 after the
# rest has run, is the demo not starting, showing no window, or a walk of its
# screens that does not end or makes no picture of the app's own content.
#
#   powershell -File take-pictures.ps1 -Exe <Navree.Demo.exe> -OutDir <folder>
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$OutDir
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$report = Join-Path $OutDir 'report.txt'
$failures = New-Object System.Collections.Generic.List[string]
function Say([string]$line) {
    Write-Host $line
    Add-Content -Path $report -Value $line
}

Add-Type -ReferencedAssemblies System.Core, System.Drawing, System.Windows.Forms -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public static class Pictures
{
    [StructLayout(LayoutKind.Sequential)]
    private struct Rect { public int Left; public int Top; public int Right; public int Bottom; }

    [DllImport("user32.dll")] private static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] private static extern bool PrintWindow(IntPtr window, IntPtr context, uint flags);

    // How many different colours a spread of points of the picture holds: a picture
    // of one colour is a picture of nothing.
    private static int Colours(Bitmap bitmap)
    {
        HashSet<int> seen = new HashSet<int>();
        for (int y = 0; y < 60; y++)
        {
            for (int x = 0; x < 60; x++)
            {
                seen.Add(bitmap.GetPixel(x * (bitmap.Width - 1) / 59, y * (bitmap.Height - 1) / 59).ToArgb());
            }
        }
        return seen.Count;
    }

    public static string WholeScreen(string path)
    {
        try
        {
            SetProcessDPIAware();
            Rectangle area = SystemInformation.VirtualScreen;
            using (Bitmap bitmap = new Bitmap(area.Width, area.Height))
            {
                using (Graphics graphics = Graphics.FromImage(bitmap))
                {
                    graphics.CopyFromScreen(area.Left, area.Top, 0, 0, area.Size);
                }
                bitmap.Save(path, ImageFormat.Png);
                return "wrote " + area.Width + "x" + area.Height + ", " + Colours(bitmap) + " colours sampled";
            }
        }
        catch (Exception problem)
        {
            return "failed: " + problem.Message;
        }
    }

    public static string OneWindow(IntPtr window, string path)
    {
        try
        {
            SetProcessDPIAware();
            Rect rect;
            if (!GetWindowRect(window, out rect)) { return "failed: GetWindowRect"; }
            int width = rect.Right - rect.Left;
            int height = rect.Bottom - rect.Top;
            if (width <= 0 || height <= 0) { return "failed: the window is " + width + "x" + height; }
            using (Bitmap bitmap = new Bitmap(width, height))
            {
                using (Graphics graphics = Graphics.FromImage(bitmap))
                {
                    IntPtr context = graphics.GetHdc();
                    bool drawn = PrintWindow(window, context, 2);
                    graphics.ReleaseHdc(context);
                    if (!drawn) { return "failed: PrintWindow answered false"; }
                }
                bitmap.Save(path, ImageFormat.Png);
                return "wrote " + width + "x" + height + ", " + Colours(bitmap) + " colours sampled";
            }
        }
        catch (Exception problem)
        {
            return "failed: " + problem.Message;
        }
    }
}
'@

Say "== the session"
Say ("user interactive: " + [Environment]::UserInteractive)
Say ("this process: session " + (Get-Process -Id $PID).SessionId + ", user " + [Environment]::UserName)
try {
    Add-Type -AssemblyName System.Windows.Forms
    Say ("primary screen: " + [System.Windows.Forms.SystemInformation]::PrimaryMonitorSize + ", virtual screen: " + [System.Windows.Forms.SystemInformation]::VirtualScreen)
} catch { Say "screen size: failed: $($_.Exception.Message)" }
try {
    $metrics = Get-ItemProperty 'HKCU:\Control Panel\Desktop\WindowMetrics' -ErrorAction Stop
    Say ("applied DPI: " + $metrics.AppliedDPI)
    $personalize = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' -ErrorAction Stop
    Say ("apps use light theme: " + $personalize.AppsUseLightTheme + ", transparency: " + $personalize.EnableTransparency)
} catch { Say "dpi and theme: failed: $($_.Exception.Message)" }
Say ("Set-DisplayResolution available: " + [bool](Get-Command Set-DisplayResolution -ErrorAction SilentlyContinue))
try {
    Get-CimInstance Win32_VideoController | ForEach-Object { Say ("video: " + $_.Name + ", " + $_.CurrentHorizontalResolution + "x" + $_.CurrentVerticalResolution) }
} catch { Say "video: failed: $($_.Exception.Message)" }

Say ""
Say "== the display, made larger"
foreach ($size in @(@(1920, 1080), @(1680, 1050), @(1600, 900), @(1366, 768), @(1280, 720))) {
    try {
        Set-DisplayResolution -Width $size[0] -Height $size[1] -Force -ErrorAction Stop | Out-Null
        Start-Sleep -Seconds 2
        $now = [System.Windows.Forms.SystemInformation]::PrimaryMonitorSize
        Say ("Set-DisplayResolution " + $size[0] + "x" + $size[1] + ": the screen is now " + $now)
        if ($now.Width -eq $size[0]) { break }
    } catch {
        Say ("Set-DisplayResolution " + $size[0] + "x" + $size[1] + ": failed: " + $_.Exception.Message)
    }
}

Say ""
Say "== the demo, started from $Exe"
$folder = Split-Path -Parent $Exe
$started = Get-Date
$app = Start-Process -FilePath $Exe -WorkingDirectory $folder -PassThru
Say ("process " + $app.Id + ", session " + $app.SessionId)

$window = [IntPtr]::Zero
for ($i = 0; $i -lt 90; $i++) {
    Start-Sleep -Seconds 1
    $app.Refresh()
    if ($app.HasExited) { break }
    if ($app.MainWindowHandle -ne [IntPtr]::Zero) { $window = $app.MainWindowHandle; break }
}
if ($app.HasExited) {
    $failures.Add("the demo ended by itself, exit code " + $app.ExitCode)
    Say ("the demo ended by itself, exit code " + $app.ExitCode)
    Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = $started } -MaxEvents 8 -ErrorAction SilentlyContinue |
        ForEach-Object { Say ("event " + $_.ProviderName + ": " + (($_.Message -split "`n")[0..6] -join ' | ')) }
} elseif ($window -eq [IntPtr]::Zero) {
    $failures.Add("the demo is running but no window came in 90 seconds")
    Say "the demo is running but no window came in 90 seconds"
} else {
    Say ("a window came after about " + $i + " seconds: handle " + $window + ", title '" + $app.MainWindowTitle + "', responding " + $app.Responding)
    # Time to draw the first frame, and for the backdrop to arrive.
    Start-Sleep -Seconds 4
    $app.Refresh()
    Say ("after four more seconds: still running " + (-not $app.HasExited) + ", responding " + $app.Responding)

    Say ""
    Say "== a picture, each way"
    Say ("whole screen (CopyFromScreen): " + [Pictures]::WholeScreen((Join-Path $OutDir 'screen.png')))
    Say ("the window (PrintWindow): " + [Pictures]::OneWindow($window, (Join-Path $OutDir 'window-printwindow.png')))

    $winapp = Get-Command winapp -ErrorAction SilentlyContinue
    if ($winapp) {
        $out = & winapp ui screenshot -a $app.Id -o (Join-Path $OutDir 'window-winapp.png') 2>&1
        Say ("winapp ui screenshot (the window): exit " + $LASTEXITCODE + ": " + (($out | Out-String).Trim() -replace "`r?`n", ' | '))
        $out = & winapp ui screenshot -w ([int64]$window) --capture-screen -o (Join-Path $OutDir 'window-winapp-screen.png') 2>&1
        Say ("winapp ui screenshot (--capture-screen): exit " + $LASTEXITCODE + ": " + (($out | Out-String).Trim() -replace "`r?`n", ' | '))
        $out = & winapp ui inspect -a $app.Id 2>&1
        Set-Content -Path (Join-Path $OutDir 'winapp-inspect.txt') -Value ($out | Out-String)
        Say ("winapp ui inspect: exit " + $LASTEXITCODE)
    } else {
        Say "winapp is not on this machine"
    }
}

if (-not $app.HasExited) {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
    Say "the demo was stopped by its process id"
}
$log = Join-Path $folder 'navree-demo.log'
if (Test-Path $log) { Copy-Item $log (Join-Path $OutDir 'navree-demo.log'); Say "the demo wrote navree-demo.log" }

Say ""
Say "== the app's own pictures of its content, walked through its screens at three widths"
$walk = Join-Path $OutDir 'walk'
New-Item -ItemType Directory -Force -Path $walk | Out-Null
foreach ($look in @(@('--light'), @('--dark'), @('--light', '--rtl'))) {
    $arguments = @('--walk', $walk) + $look
    $walker = Start-Process -FilePath $Exe -ArgumentList $arguments -WorkingDirectory $folder -PassThru
    if ($walker.WaitForExit(240000)) {
        Say ("the walk " + ($look -join ' ') + " ended by itself, exit code " + $walker.ExitCode)
    } else {
        $failures.Add("the walk " + ($look -join ' ') + " did not end in 240 seconds")
        Say ("the walk " + ($look -join ' ') + " did not end in 240 seconds")
        Stop-Process -Id $walker.Id -Force -ErrorAction SilentlyContinue
    }
}
Get-ChildItem $walk -Filter '*.txt' -ErrorAction SilentlyContinue | ForEach-Object { Say ("--- " + $_.Name); Get-Content $_.FullName | ForEach-Object { Say ("    " + $_) } }
$log = Join-Path $folder 'navree-demo.log'
if (Test-Path $log) { Copy-Item $log (Join-Path $OutDir 'navree-demo-after-walks.log'); Say "the demo wrote navree-demo.log" }

Say ""
Say "== what was written"
Get-ChildItem $OutDir -Recurse -Filter '*.png' | ForEach-Object { Say ($_.Name + " " + $_.Length) }
if (-not (Get-ChildItem $walk -Filter '*.png' -ErrorAction SilentlyContinue)) { $failures.Add("the walks made no picture of the app's own content") }
if ($failures.Count -gt 0) {
    Say ""
    Say "== what failed"
    $failures | ForEach-Object { Say ("FAILED: " + $_) }
    exit 1
}
exit 0
