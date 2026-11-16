# Draws the demo package's pictures (its logos), so no picture has to be kept in the repository:
# a dark rounded square with a green N, at the sizes the package's manifest names.
#
#   powershell -File make-assets.ps1 -Out <folder>
param([Parameter(Mandatory = $true)][string]$Out)

Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force -Path $Out | Out-Null

function New-Logo([int]$Width, [int]$Height, [string]$Name) {
    $bitmap = New-Object System.Drawing.Bitmap $Width, $Height, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $side = [Math]::Min($Width, $Height)
    $radius = [int]($side * 0.22)
    $left = [int](($Width - $side) / 2)
    $top = [int](($Height - $side) / 2)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc($left, $top, $radius * 2, $radius * 2, 180, 90)
    $path.AddArc($left + $side - $radius * 2 - 1, $top, $radius * 2, $radius * 2, 270, 90)
    $path.AddArc($left + $side - $radius * 2 - 1, $top + $side - $radius * 2 - 1, $radius * 2, $radius * 2, 0, 90)
    $path.AddArc($left, $top + $side - $radius * 2 - 1, $radius * 2, $radius * 2, 90, 90)
    $path.CloseFigure()
    $graphics.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 18, 20, 18))), $path)
    $font = New-Object System.Drawing.Font 'Segoe UI', ([single]($side * 0.5)), ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
    $format = New-Object System.Drawing.StringFormat
    $format.Alignment = [System.Drawing.StringAlignment]::Center
    $format.LineAlignment = [System.Drawing.StringAlignment]::Center
    $rect = New-Object System.Drawing.RectangleF ([single]$left), ([single]$top), ([single]$side), ([single]$side)
    $graphics.DrawString('N', $font, (New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 57, 230, 122))), $rect, $format)
    $graphics.Dispose()
    $bitmap.Save((Join-Path $Out $Name), [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
    Write-Host "wrote $Name ${Width}x${Height}"
}

New-Logo 44 44 'Square44x44Logo.png'
New-Logo 150 150 'Square150x150Logo.png'
New-Logo 50 50 'StoreLogo.png'
