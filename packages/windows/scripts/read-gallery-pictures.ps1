# Reads the pictures the gallery's marking scenarios leave in a folder and writes what they show to
# analysis.txt there: where the frozen parts stand, whether the header and the rows agree, and what
# the scroll view's own numbers were.
#
#   powershell -File read-gallery-pictures.ps1 -OutDir <folder>
param(
    [Parameter(Mandatory = $true)][string]$OutDir
)

$ErrorActionPreference = 'Continue'

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

public static class PictureReader
{
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

$analysis = Join-Path $OutDir 'analysis.txt'
if (Test-Path $analysis) { Remove-Item $analysis }
function Note([string]$line) { Write-Host $line; Add-Content -Path $analysis -Value $line }

# "from 12 to 40" for a bounds array, or "none".
function Span($bounds) {
    if ($bounds[0] -eq 0) { return 'none' }
    return ('{0} to {1}' -f $bounds[1], $bounds[2])
}

# The largest difference between two bounds arrays' edges, or the word missing when one has no pixels.
function Differ($a, $b) {
    if ($a[0] -eq 0 -and $b[0] -eq 0) { return 0 }
    if ($a[0] -eq 0 -or $b[0] -eq 0) { return 'missing' }
    return [Math]::Max([Math]::Abs($a[1] - $b[1]), [Math]::Abs($a[2] - $b[2]))
}

# The pictures of the page that keeps parts in place: a magenta bar at the start edge of the frozen
# cell, a cyan bar at the end edge of the end cell, yellow bars among the scrolling cells. The header
# is the first 32 rows of pixels below the window's title; the rows are what is below it.
$palette = @(220,50,50, 50,170,70, 50,90,220, 240,150,30, 30,170,170, 150,60,190)
$groups = @{}
foreach ($pic in (Get-ChildItem $OutDir -Filter 'stick-*.png' | Sort-Object Name)) {
    $size = [PictureReader]::Size($pic.FullName)
    $rtl = $pic.Name -like '*-rtl-*'
    $magenta = [PictureReader]::Bounds($pic.FullName, 255, 0, 255, 0, $size[0], 0, $size[1])
    if ($magenta[0] -eq 0) { Note ($pic.Name + ": no magenta marker found (picture " + $size[0] + "x" + $size[1] + ")"); continue }
    $top = $magenta[3]
    $w = $size[0]; $h = $size[1]
    $hm = [PictureReader]::Bounds($pic.FullName, 255, 0, 255, 0, $w, $top, $top + 32)
    $bm = [PictureReader]::Bounds($pic.FullName, 255, 0, 255, 0, $w, $top + 40, $h)
    $hc = [PictureReader]::Bounds($pic.FullName, 0, 255, 255, 0, $w, $top, $top + 32)
    $bc = [PictureReader]::Bounds($pic.FullName, 0, 255, 255, 0, $w, $top + 40, $h)
    $hy = [PictureReader]::Bounds($pic.FullName, 255, 255, 0, 0, $w, $top, $top + 32)
    $by = [PictureReader]::Bounds($pic.FullName, 255, 255, 0, 0, $w, $top + 40, $h)
    $edge = if ($rtl) { $magenta[2] } else { $magenta[1] }
    if ($rtl) { $leak = [PictureReader]::CountAny($pic.FullName, $palette, $edge - 190, $edge - 8, $top + 40, $h) }
    else { $leak = [PictureReader]::CountAny($pic.FullName, $palette, $edge + 8, $edge + 190, $top + 40, $h) }
    $dm = Differ $hm $bm
    $dc = Differ $hc $bc
    $dy = Differ $hy $by
    Note ("{0}: frozen (magenta) header {1}, rows {2}; end (cyan) header {3}, rows {4}; scrolling (yellow) header {5}, rows {6}; header and rows differ by {7} (frozen), {8} (end), {9} (scrolling); scrolling colours in the frozen area: {10}" -f $pic.Name, (Span $hm), (Span $bm), (Span $hc), (Span $bc), (Span $hy), (Span $by), $dm, $dc, $dy, $leak)
    $key = ($pic.Name -replace '-(\d+|moving-\d+)\.png$', '')
    if (-not $groups.ContainsKey($key)) { $groups[$key] = @() }
    $groups[$key] += ,@($pic.Name, $edge, $dm, $dc, $dy, $leak)
}
foreach ($key in ($groups.Keys | Sort-Object)) {
    $at = $groups[$key] | Where-Object { $_[0] -notlike '*moving*' }
    $moving = $groups[$key] | Where-Object { $_[0] -like '*moving*' }
    if ($at) {
        $edges = $at | ForEach-Object { $_[1] }
        $spread = ($edges | Measure-Object -Maximum -Minimum)
        Note ("{0}: at rest the frozen marker stood at {1} to {2} across {3} pictures (spread {4} pixels)" -f $key, $spread.Minimum, $spread.Maximum, $edges.Count, ($spread.Maximum - $spread.Minimum))
    }
    if ($moving) {
        $worst = 0; $missing = 0; $leaks = 0
        foreach ($m in $moving) {
            foreach ($d in $m[2], $m[3], $m[4]) { if ($d -eq 'missing') { $missing++ } elseif ($d -gt $worst) { $worst = $d } }
            if ($m[5] -gt 0) { $leaks++ }
        }
        Note ("{0}: in {1} pictures taken while moving, the header and the rows differed by {2} pixels at most; {3} parts missing from one of them; {4} pictures with scrolling colours in the frozen area" -f $key, $moving.Count, $worst, $missing, $leaks)
    }
}

# The pictures of the scroll view's own numbers: a bar for each, moved from x = 500 by the number
# times its scale, so the bar's place says the number. 1 is what the window's left edge adds.
$markers = @(
    @('Position', 255, 0, 0, 0.2),
    @('MinPosition', 0, 200, 0, 0.2),
    @('MaxPosition', 0, 0, 255, 0.2),
    @('Offset', 255, 128, 0, 0.2),
    @('Extent', 0, 255, 255, 0.1),
    @('Viewport', 255, 0, 255, 0.1)
)
foreach ($pic in (Get-ChildItem $OutDir -Filter 'values-*.png' | Sort-Object Name)) {
    $size = [PictureReader]::Size($pic.FullName)
    $parts = @()
    foreach ($m in $markers) {
        $b = [PictureReader]::Bounds($pic.FullName, $m[1], $m[2], $m[3], 0, $size[0], 0, $size[1])
        if ($b[0] -eq 0) { $parts += ($m[0] + ' not seen'); continue }
        $value = ($b[1] - 1 - 500) / $m[4]
        $parts += ('{0} {1:0} (bar at {2})' -f $m[0], $value, $b[1])
    }
    Note ($pic.Name + ': ' + ($parts -join '; '))
}

# The pictures of bars moved by a written number, an animation, or both: each bar starts at x = 100,
# so where it is says how far it was moved.
$bars = @(
    @('A written 50', 255, 0, 0),
    @('B written 50 then animated to 80', 0, 200, 0),
    @('C animated to 80 then written 50', 0, 0, 255),
    @('D animated to 80, written 50, animation stopped', 255, 128, 0),
    @('E written 50, animated to 80, animation stopped', 0, 255, 255),
    @('F animated to 80', 255, 0, 255)
)
foreach ($pic in (Get-ChildItem $OutDir -Filter 'trans-*.png' | Sort-Object Name)) {
    $size = [PictureReader]::Size($pic.FullName)
    $parts = @()
    foreach ($m in $bars) {
        $b = [PictureReader]::Bounds($pic.FullName, $m[1], $m[2], $m[3], 0, $size[0], 0, $size[1])
        if ($b[0] -eq 0) { $parts += ($m[0] + ': not seen'); continue }
        $parts += ('{0}: moved {1}' -f $m[0], ($b[1] - 1 - 100))
    }
    Note ($pic.Name + ': ' + ($parts -join '; '))
}
