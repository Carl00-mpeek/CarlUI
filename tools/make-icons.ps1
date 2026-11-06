# CarlUI exe simgelerini üretir: assets\icons\<ad>.ico
# Hepsi aynı aileden: vurgu degradeli (mavi -> mor) yuvarlak kare ve ortada
# beyaz, sade bir işaret. Her .ico 16-256 px arası PNG katmanları içerir.
# Kullanım: powershell -ExecutionPolicy Bypass -File tools\make-icons.ps1

Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $root 'assets\icons'
New-Item -ItemType Directory -Force $out | Out-Null

$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$c0 = [System.Drawing.Color]::FromArgb(255, 0x4F, 0x8B, 0xFF)
$c1 = [System.Drawing.Color]::FromArgb(255, 0x8B, 0x5C, 0xF6)
$white = [System.Drawing.Color]::FromArgb(255, 255, 255, 255)

function RoundRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = [Math]::Min($r * 2, [Math]::Min($w, $h))
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    $p
}

# Simge içeriği: 0..1 birim karede çizen betik blokları.
$marks = @{
    # Ana kabuk: üstte ada, altta dock.
    'carlui'   = { param($g, $s, $b) $g.FillPath($b, (RoundRect (0.32*$s) (0.22*$s) (0.36*$s) (0.12*$s) (0.06*$s))); $g.FillPath($b, (RoundRect (0.2*$s) (0.62*$s) (0.6*$s) (0.16*$s) (0.08*$s))) }
    # Dock: içinde üç simge yuvası olan çubuk.
    'dock'     = { param($g, $s, $b) $g.FillPath($b, (RoundRect (0.16*$s) (0.56*$s) (0.68*$s) (0.22*$s) (0.09*$s))); $h = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 0x6A, 0x70, 0xF9)); foreach ($i in 0..2) { $g.FillPath($h, (RoundRect ((0.24 + $i * 0.18)*$s) (0.61*$s) (0.14*$s) (0.12*$s) (0.04*$s))) } }
    # Ada: üstte yüzen hap.
    'island'   = { param($g, $s, $b) $g.FillPath($b, (RoundRect (0.2*$s) (0.36*$s) (0.6*$s) (0.24*$s) (0.12*$s))); $h = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 0x6A, 0x70, 0xF9)); $g.FillEllipse($h, (0.28*$s), (0.42*$s), (0.12*$s), (0.12*$s)) }
    'tray'     = 'E70E'
    'taskmgr'  = 'E9D9'
    'settings' = 'E713'
    'convert'  = 'E8AB'
    # Vault: kilit.
    'vault'    = 'E72E'
    'uninstall' = 'E74D'
    # Office: köşesi kıvrık belge, içinde satırlar.
    'office'   = { param($g, $s, $b) $g.FillPath($b, (RoundRect (0.27*$s) (0.17*$s) (0.46*$s) (0.66*$s) (0.06*$s))); $h = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 0x6A, 0x70, 0xF9)); foreach ($i in 0..3) { $w = if ($i -eq 3) { 0.18 } else { 0.30 }; $g.FillRectangle($h, (0.35*$s), ((0.33 + $i * 0.11)*$s), ($w*$s), [Math]::Max(1, 0.05*$s)) } }
    # Sensörler: termometre.
    'sensors'  = { param($g, $s, $b) $g.FillPath($b, (RoundRect (0.43*$s) (0.18*$s) (0.14*$s) (0.46*$s) (0.07*$s))); $g.FillEllipse($b, (0.36*$s), (0.56*$s), (0.28*$s), (0.28*$s)) }
}

function Render([string]$name, [int]$s) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.TextRenderingHint = 'AntiAliasGridFit'
    $g.PixelOffsetMode = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Transparent)
    $pad = if ($s -le 20) { 0 } else { [Math]::Round($s * 0.04) }
    $rect = New-Object System.Drawing.RectangleF $pad, $pad, ($s - 2 * $pad), ($s - 2 * $pad)
    $grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, $c0, $c1, 45.0
    $g.FillPath($grad, (RoundRect $pad $pad ($s - 2 * $pad) ($s - 2 * $pad) (($s - 2 * $pad) * 0.24)))
    $wb = New-Object System.Drawing.SolidBrush $white
    $m = $marks[$name]
    if ($m -is [string]) {
        $font = New-Object System.Drawing.Font 'Segoe Fluent Icons', ([float]($s * 0.5)), ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
        $sf = New-Object System.Drawing.StringFormat
        $sf.Alignment = 'Center'; $sf.LineAlignment = 'Center'
        $glyph = [string][char][Convert]::ToInt32($m, 16)
        $g.DrawString($glyph, $font, $wb, (New-Object System.Drawing.RectangleF 0, ($s * 0.02), $s, $s), $sf)
    } else {
        & $m $g $s $wb
    }
    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    if ($s -ge 256) {
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        # Küçük boyutlar klasik 32 bit DIB: her Windows API'si okuyabilsin.
        $w = New-Object System.IO.BinaryWriter $ms
        $w.Write([UInt32]40); $w.Write([Int32]$s); $w.Write([Int32]($s * 2)); $w.Write([UInt16]1); $w.Write([UInt16]32)
        $w.Write([UInt32]0); $w.Write([UInt32]0); $w.Write([Int32]0); $w.Write([Int32]0); $w.Write([UInt32]0); $w.Write([UInt32]0)
        for ($y = $s - 1; $y -ge 0; $y--) {
            for ($x = 0; $x -lt $s; $x++) {
                $c = $bmp.GetPixel($x, $y)
                $w.Write([byte]$c.B); $w.Write([byte]$c.G); $w.Write([byte]$c.R); $w.Write([byte]$c.A)
            }
        }
        $maskRow = [Math]::Ceiling($s / 32) * 4
        $w.Write((New-Object byte[] ($maskRow * $s)))
        $w.Flush()
    }
    $bmp.Dispose()
    , $ms.ToArray()
}

foreach ($name in $marks.Keys) {
    $pngs = @($sizes | ForEach-Object { , (Render $name $_) })
    $fs = New-Object System.IO.MemoryStream
    $w = New-Object System.IO.BinaryWriter $fs
    $w.Write([UInt16]0); $w.Write([UInt16]1); $w.Write([UInt16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $s = $sizes[$i]; $len = $pngs[$i].Length
        $w.Write([byte]($s % 256)); $w.Write([byte]($s % 256)); $w.Write([byte]0); $w.Write([byte]0)
        $w.Write([UInt16]1); $w.Write([UInt16]32); $w.Write([UInt32]$len); $w.Write([UInt32]$offset)
        $offset += $len
    }
    foreach ($p in $pngs) { $w.Write($p) }
    [IO.File]::WriteAllBytes((Join-Path $out "$name.ico"), $fs.ToArray())
    # Önizleme için 256 px PNG.
    [IO.File]::WriteAllBytes((Join-Path $out "$name.png"), $pngs[-1])
    Write-Output "$name.ico"
}
