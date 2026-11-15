# Starts the gallery with scenarios to run, takes a picture of its window each time the gallery
# says it is ready (a file named <id>.ready in the output folder), answers with <id>.taken, and
# reads the pictures of the scenarios that mark their parts with colours.
#
#   powershell -File run-gallery.ps1 -Exe <Navree.Gallery.exe> -OutDir <folder> [-Run table,stick,anchor]
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Run = 'table,stick,anchor',
    [int]$Seconds = 420
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$log = Join-Path $OutDir 'script.txt'
function Say([string]$line) {
    Write-Host $line
    Add-Content -Path $log -Value $line
}

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

public static class Probe
{
    [StructLayout(LayoutKind.Sequential)]
    private struct Rect { public int Left; public int Top; public int Right; public int Bottom; }

    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] private static extern bool PrintWindow(IntPtr window, IntPtr context, uint flags);

    public static string PrintWindowTo(IntPtr window, string path)
    {
        try
        {
            Rect rect;
            if (!GetWindowRect(window, out rect)) { return "GetWindowRect failed"; }
            int width = rect.Right - rect.Left;
            int height = rect.Bottom - rect.Top;
            if (width <= 0 || height <= 0) { return "the window is " + width + "x" + height; }
            using (Bitmap bitmap = new Bitmap(width, height))
            {
                using (Graphics graphics = Graphics.FromImage(bitmap))
                {
                    IntPtr context = graphics.GetHdc();
                    bool drawn = PrintWindow(window, context, 2);
                    graphics.ReleaseHdc(context);
                    if (!drawn) { return "PrintWindow answered false"; }
                }
                bitmap.Save(path, ImageFormat.Png);
                return "ok";
            }
        }
        catch (Exception problem) { return problem.Message; }
    }
}
'@

Say "== the gallery, started from $Exe with --run $Run"
$folder = Split-Path -Parent $Exe
$app = Start-Process -FilePath $Exe -ArgumentList @('--run', $Run, '--out', $OutDir) -WorkingDirectory $folder -PassThru
Say ("process " + $app.Id)
$winapp = Get-Command winapp -ErrorAction SilentlyContinue
if (-not $winapp) { Say "winapp is not on this machine: the window is pictured with PrintWindow" }

$started = Get-Date
$taken = 0
while (-not $app.HasExited -and ((Get-Date) - $started).TotalSeconds -lt $Seconds) {
    foreach ($ready in (Get-ChildItem $OutDir -Filter '*.ready' -ErrorAction SilentlyContinue)) {
        $id = [IO.Path]::GetFileNameWithoutExtension($ready.Name)
        $done = Join-Path $OutDir ($id + '.taken')
        if (Test-Path $done) { continue }
        $png = Join-Path $OutDir ($id + '.png')
        $how = ''
        if ($winapp) {
            $out = & winapp ui screenshot -a $app.Id -o $png 2>&1
            $how = 'winapp exit ' + $LASTEXITCODE
        }
        if (-not (Test-Path $png)) {
            $app.Refresh()
            $how += ' printwindow: ' + [Probe]::PrintWindowTo($app.MainWindowHandle, $png)
        }
        if ($winapp -and $id -like 'uia-*') {
            $tree = Join-Path $OutDir ($id + '-inspect.txt')
            & winapp ui inspect -a $app.Id 2>&1 | Out-File -FilePath $tree -Encoding utf8
            $how += ' inspect exit ' + $LASTEXITCODE
        }
        Say ("picture " + $id + ": " + $how + "; " + (Get-Content $ready.FullName -Raw).Trim())
        New-Item -ItemType File -Path $done -Force | Out-Null
        $taken++
    }
    Start-Sleep -Milliseconds 80
    $app.Refresh()
}
if (-not $app.HasExited) {
    Say "the gallery was still running after $Seconds seconds and was stopped by its process id"
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
} else {
    Say ("the gallery ended, exit code " + $app.ExitCode + "; " + $taken + " pictures")
}
$crash = Join-Path $folder 'navree-gallery.log'
if (Test-Path $crash) { Copy-Item $crash (Join-Path $OutDir 'navree-gallery.log'); Say "the gallery wrote navree-gallery.log" }

# The scenarios that mark their parts have their pictures read: where the frozen parts stood, whether
# the header and the rows agreed, and what the scroll view's own numbers were.
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'read-gallery-pictures.ps1') -OutDir $OutDir
exit 0
