# Deterministic text-only OCR test inputs. This script is for development, not
# an application dependency. Expected text is never passed to inference.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$fixtureDir = Join-Path $PSScriptRoot '../tests/fixtures'
$bitmap = [Drawing.Bitmap]::new(1000,420)
$graphics = [Drawing.Graphics]::FromImage($bitmap)
$font = [Drawing.Font]::new('Microsoft YaHei',30,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
try {
    $graphics.Clear([Drawing.Color]::White)
    $graphics.TextRenderingHint = [Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    $lines = @('游戏任务完成 GAME OCR 100','繁體中文 測試成功','日本語 こんにちは 世界','Health: 100 / 100')
    for ($i=0; $i -lt $lines.Count; $i++) { $graphics.DrawString($lines[$i],$font,[Drawing.Brushes]::Black,40,25+75*$i) }
    $bitmap.Save((Join-Path $fixtureDir 'mixed-cjk.png'),[Drawing.Imaging.ImageFormat]::Png)
    $graphics.Clear([Drawing.Color]::White)
    $bitmap.Save((Join-Path $fixtureDir 'blank.png'),[Drawing.Imaging.ImageFormat]::Png)
    $graphics.TranslateTransform(180,80)
    $graphics.RotateTransform(12)
    $graphics.DrawString('ROTATED TEXT 123',$font,[Drawing.Brushes]::Black,0,0)
    $graphics.ResetTransform()
    $smallFont = [Drawing.Font]::new('Microsoft YaHei',16,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
    try { $graphics.DrawString('Small game text 456',$smallFont,[Drawing.Brushes]::Black,40,280) } finally { $smallFont.Dispose() }
    $bitmap.Save((Join-Path $fixtureDir 'rotated-small.png'),[Drawing.Imaging.ImageFormat]::Png)
} finally { $font.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
