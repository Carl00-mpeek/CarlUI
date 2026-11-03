# CarlUI duvar kâğıdını üretir: siyah zemin, ortada "CarlUI".
# "Carl" kırık beyaz buzlu cam, "UI" One UI mavisi.
#
# Kullanım:  powershell -ExecutionPolicy Bypass -File tools\make-wallpaper.ps1
# Çıktı:     assets\wallpaper\carlui-<genişlik>x<yükseklik>.png

param([int[]]$Sizes = @(3840, 2560, 1920))

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$outDir = Join-Path $root 'assets\wallpaper'
New-Item -ItemType Directory -Force $outDir | Out-Null

$blue = [System.Drawing.Color]::FromArgb(255, 0x3E, 0x8B, 0xFF)
$families = (New-Object System.Drawing.Text.InstalledFontCollection).Families | ForEach-Object Name
$font = @('Segoe UI Variable Display Semib', 'Segoe UI Semibold', 'Segoe UI') | Where-Object { $families -contains $_ } | Select-Object -First 1

function Text-Path([string]$text, [float]$size, [float]$x, [float]$y) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $fmt = [System.Drawing.StringFormat]::GenericTypographic
    $p.AddString($text, (New-Object System.Drawing.FontFamily $font), [int][System.Drawing.FontStyle]::Regular, $size, (New-Object System.Drawing.PointF $x, $y), $fmt)
    $p
}

# Yolun çevresine yumuşak ışık: kalından inceye, çok saydam kalemlerle.
function Glow($g, $path, [System.Drawing.Color]$c, [float]$radius, [int]$alpha) {
    for ($i = 12; $i -ge 1; $i--) {
        $w = $radius * $i / 12
        $a = [int]($alpha / 12)
        $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb($a, $c)), $w
        $pen.LineJoin = 'Round'
        $g.DrawPath($pen, $path)
        $pen.Dispose()
    }
}

foreach ($W in $Sizes) {
    $H = [int]($W * 9 / 16)
    $bmp = New-Object System.Drawing.Bitmap $W, $H
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Black)

    # Yazıyı ölç ve ortala ("Carl" ile "UI" arası sıkı).
    $size = $W * 0.115
    $carl = Text-Path 'Carl' $size 0 0
    $ui = Text-Path 'UI' $size 0 0
    $bc = $carl.GetBounds(); $bu = $ui.GetBounds()
    $gap = $size * 0.11
    $total = $bc.Width + $gap + $bu.Width
    $x0 = ($W - $total) / 2 - $bc.X
    $top = [Math]::Min($bc.Y, $bu.Y); $bottom = [Math]::Max($bc.Bottom, $bu.Bottom)
    $y0 = ($H - ($bottom - $top)) / 2 - $top
    $m = New-Object System.Drawing.Drawing2D.Matrix
    $m.Translate($x0, $y0); $carl.Transform($m)
    $m = New-Object System.Drawing.Drawing2D.Matrix
    $m.Translate($x0 + $bc.Width + $gap - $bu.X + $bc.X, $y0); $ui.Transform($m)
    $bc = $carl.GetBounds(); $bu = $ui.GetBounds()

    # --- "Carl": buzlu cam ---
    Glow $g $carl ([System.Drawing.Color]::FromArgb(255, 250, 246, 238)) ($size * 0.10) 30
    $body = New-Object System.Drawing.Drawing2D.LinearGradientBrush $bc, ([System.Drawing.Color]::FromArgb(240, 248, 245, 238)), ([System.Drawing.Color]::FromArgb(222, 242, 238, 230)), 90
    $g.FillPath($body, $carl)
    # Üstteki parlak yansıma: yalnızca harflerin üst %42'sinde.
    $g.SetClip($carl)
    $spec = New-Object System.Drawing.RectangleF $bc.X, ($bc.Y - 1), $bc.Width, ($bc.Height * 0.42)
    $sb = New-Object System.Drawing.Drawing2D.LinearGradientBrush $spec, ([System.Drawing.Color]::FromArgb(150, 255, 255, 255)), ([System.Drawing.Color]::FromArgb(0, 255, 255, 255)), 90
    $g.FillRectangle($sb, $spec)
    $g.ResetClip()
    # Cam kenarı: ince yarı saydam çizgi, üst kenarda daha parlak.
    $edge = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(150, 255, 252, 246)), ($size * 0.012)
    $edge.LineJoin = 'Round'
    $g.DrawPath($edge, $carl)
    $g.SetClip((New-Object System.Drawing.RectangleF $bc.X, ($bc.Y - 4), $bc.Width, ($bc.Height * 0.45)))
    $rim = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(200, 255, 255, 255)), ($size * 0.006)
    $rim.LineJoin = 'Round'
    $g.DrawPath($rim, $carl)
    $g.ResetClip()

    # --- "UI": One UI mavisi ---
    Glow $g $ui $blue ($size * 0.16) 70
    $g.FillPath((New-Object System.Drawing.SolidBrush $blue), $ui)

    $out = Join-Path $outDir "carlui-${W}x${H}.png"
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Host "Hazır: $out ($font)"
}
