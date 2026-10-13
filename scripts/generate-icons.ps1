$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$iconDir = Join-Path $PSScriptRoot '../src-tauri/icons'
New-Item -ItemType Directory -Force -Path $iconDir | Out-Null
foreach ($size in @(32, 128, 256)) {
    $image = [System.Drawing.Bitmap]::new($size, $size)
    $graphics = [System.Drawing.Graphics]::FromImage($image)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.Clear([System.Drawing.Color]::FromArgb(18,23,30))
    $pen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(106,228,197), $size/18)
    $a = $size * 0.2; $b = $size * 0.8; $length = $size * 0.16
    $graphics.DrawLines($pen, [System.Drawing.PointF[]]@([System.Drawing.PointF]::new($a,$a+$length),[System.Drawing.PointF]::new($a,$a),[System.Drawing.PointF]::new($a+$length,$a)))
    $graphics.DrawLines($pen, [System.Drawing.PointF[]]@([System.Drawing.PointF]::new($b-$length,$a),[System.Drawing.PointF]::new($b,$a),[System.Drawing.PointF]::new($b,$a+$length)))
    $graphics.DrawLines($pen, [System.Drawing.PointF[]]@([System.Drawing.PointF]::new($a,$b-$length),[System.Drawing.PointF]::new($a,$b),[System.Drawing.PointF]::new($a+$length,$b)))
    $graphics.DrawLines($pen, [System.Drawing.PointF[]]@([System.Drawing.PointF]::new($b-$length,$b),[System.Drawing.PointF]::new($b,$b),[System.Drawing.PointF]::new($b,$b-$length)))
    $graphics.DrawLine($pen, $size*.36, $size*.4, $size*.65, $size*.4)
    $graphics.DrawLine($pen, $size*.36, $size*.53, $size*.65, $size*.53)
    $graphics.DrawLine($pen, $size*.36, $size*.66, $size*.53, $size*.66)
    $image.Save((Join-Path $iconDir "${size}x${size}.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $pen.Dispose(); $image.Dispose()
}
# PNG-compressed 256 px Windows icon (ICO header + directory + PNG payload).
$png = [IO.File]::ReadAllBytes((Join-Path $iconDir '256x256.png'))
$stream = [IO.File]::Create((Join-Path $iconDir 'icon.ico'))
$writer = [IO.BinaryWriter]::new($stream)
$writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]1)
$writer.Write([byte]0); $writer.Write([byte]0); $writer.Write([byte]0); $writer.Write([byte]0)
$writer.Write([uint16]1); $writer.Write([uint16]32); $writer.Write([uint32]$png.Length); $writer.Write([uint32]22)
$writer.Write($png); $writer.Dispose()
