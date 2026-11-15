# Starts the gallery with scenarios to run, takes a picture of its window each time the gallery
# says it is ready (a file named <id>.ready in the output folder), answers with <id>.taken, and
# reads the pictures of the scenarios that mark their parts with colours.
#
#   powershell -File run-gallery.ps1 -Exe <Navree.Gallery.exe> -OutDir <folder> [-Run stick,anchor]
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Run = 'stick,anchor',
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

    private static byte[] Pixels(string path, out int width, out int height)
    {
        using (Bitmap bitmap = new Bitmap(path))
        {
            width = bitmap.Width;
            height = bitmap.Height;
            Rectangle area = new Rectangle(0, 0, width, height);
            BitmapData data = bitmap.LockBits(area, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            byte[] bytes = new byte[data.Stride * height];
            Marshal.Copy(data.Scan0, bytes, 0, bytes.Length);
            bitmap.UnlockBits(data);
            return bytes;
        }
    }

    // The count and the bounds of the pixels of one colour in a region: count, minX, maxX, minY, maxY.
    public static int[] Bounds(string path, int r, int g, int b, int x0, int x1, int y0, int y1)
    {
        int width, height;
        byte[] bytes = Pixels(path, out width, out height);
        int stride = bytes.Length / height;
        int count = 0, minX = int.MaxValue, maxX = -1, minY = int.MaxValue, maxY = -1;
        for (int y = Math.Max(0, y0); y < Math.Min(height, y1); y++)
        {
            for (int x = Math.Max(0, x0); x < Math.Min(width, x1); x++)
            {
                int at = (y * stride) + (x * 4);
                if (bytes[at + 2] == r && bytes[at + 1] == g && bytes[at] == b)
                {
                    count++;
                    if (x < minX) { minX = x; }
                    if (x > maxX) { maxX = x; }
                    if (y < minY) { minY = y; }
                    if (y > maxY) { maxY = y; }
                }
            }
        }
        return new int[] { count, minX, maxX, minY, maxY };
    }

    // How many pixels in a region are exactly one of the colours given as r,g,b triples.
    public static int CountAny(string path, int[] colours, int x0, int x1, int y0, int y1)
    {
        int width, height;
        byte[] bytes = Pixels(path, out width, out height);
        int stride = bytes.Length / height;
        int count = 0;
        for (int y = Math.Max(0, y0); y < Math.Min(height, y1); y++)
        {
            for (int x = Math.Max(0, x0); x < Math.Min(width, x1); x++)
            {
                int at = (y * stride) + (x * 4);
                for (int c = 0; c + 2 < colours.Length; c += 3)
                {
                    if (bytes[at + 2] == colours[c] && bytes[at + 1] == colours[c + 1] && bytes[at] == colours[c + 2]) { count++; break; }
                }
            }
        }
        return count;
    }

    public static int[] Size(string path)
    {
        using (Bitmap bitmap = new Bitmap(path)) { return new int[] { bitmap.Width, bitmap.Height }; }
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

# The scenarios that mark their parts read their own pictures: where the frozen markers stand,
# whether the scrolling cells show through them, and whether the header and the rows line up.
$analysis = Join-Path $OutDir 'analysis.txt'
function Note([string]$line) { Write-Host $line; Add-Content -Path $analysis -Value $line }
$palette = @(220,50,50, 50,170,70, 50,90,220, 240,150,30, 30,170,170, 150,60,190)
$rows = @{}
foreach ($pic in (Get-ChildItem $OutDir -Filter 'stick-*.png' | Sort-Object Name)) {
    $size = [Probe]::Size($pic.FullName)
    $rtl = $pic.Name -like '*-rtl-*'
    $magenta = [Probe]::Bounds($pic.FullName, 255, 0, 255, 0, $size[0], 0, $size[1])
    if ($magenta[0] -eq 0) { Note ($pic.Name + ": no magenta marker found (picture " + $size[0] + "x" + $size[1] + ")"); continue }
    $top = $magenta[3]
    $cyan = [Probe]::Bounds($pic.FullName, 0, 255, 255, 0, $size[0], 0, $size[1])
    $headerYellow = [Probe]::Bounds($pic.FullName, 255, 255, 0, 0, $size[0], $top, $top + 32)
    $bodyYellow = [Probe]::Bounds($pic.FullName, 255, 255, 0, 0, $size[0], $top + 40, $size[1])
    $headerMagenta = [Probe]::Bounds($pic.FullName, 255, 0, 255, 0, $size[0], $top, $top + 32)
    $bodyMagenta = [Probe]::Bounds($pic.FullName, 255, 0, 255, 0, $size[0], $top + 40, $size[1])
    if ($rtl) { $leak = [Probe]::CountAny($pic.FullName, $palette, $magenta[2] - 190, $magenta[2] - 8, $top + 40, $size[1]) }
    else { $leak = [Probe]::CountAny($pic.FullName, $palette, $magenta[1] + 8, $magenta[1] + 190, $top + 40, $size[1]) }
    Note ("{0}: magenta (frozen) x {1} to {2} (header {3}, rows {4}); cyan (end) x {5} to {6}; yellow in the header from x {7}, in the rows from x {8}; scrolling colours seen in the frozen area: {9}" -f $pic.Name, $magenta[1], $magenta[2], $headerMagenta[1], $bodyMagenta[1], $cyan[1], $cyan[2], $headerYellow[1], $bodyYellow[1], $leak)
    $key = ($pic.Name -replace '-\d+\.png$', '' -replace '-moving\.png$', '')
    if (-not $rows.ContainsKey($key)) { $rows[$key] = @() }
    $rows[$key] += ,@($pic.Name, $(if ($rtl) { $magenta[2] } else { $magenta[1] }))
}
foreach ($key in $rows.Keys) {
    $edges = $rows[$key] | ForEach-Object { $_[1] }
    $spread = ($edges | Measure-Object -Maximum -Minimum)
    Note ("{0}: the frozen marker stood at {1} to {2} across {3} pictures (spread {4} pixels)" -f $key, $spread.Minimum, $spread.Maximum, $edges.Count, ($spread.Maximum - $spread.Minimum))
}
exit 0
