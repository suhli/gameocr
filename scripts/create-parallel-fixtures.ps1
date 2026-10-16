# Reproducible synthetic layouts; these are NOT screenshots of real games.
param([string]$Directory='tests/fixtures/parallel', [switch]$TexturedOnly)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force -Path $Directory | Out-Null
$scenarios=[Collections.Generic.List[object]]::new()
function New-Scene([string]$Name,[int]$Width,[int]$Height,[object[]]$Lines,[bool]$Textured=$false) {
    $bitmap=[Drawing.Bitmap]::new($Width,$Height)
    $graphics=[Drawing.Graphics]::FromImage($bitmap)
    $labels=[Collections.Generic.List[object]]::new()
    try {
        $graphics.Clear([Drawing.Color]::FromArgb(25,32,42))
        if ($Textured) {
            # Low-contrast, high-entropy pixels isolate PNG compression cost.
            # This is a transport stress case, not a realistic game background.
            Add-Type -TypeDefinition 'public static class GameOcrTexture { public static void Fill(byte[] b) { var random = new System.Random(1650); for(int i=0;i<b.Length;i+=4) { b[i]=(byte)random.Next(24,48); b[i+1]=(byte)random.Next(24,48); b[i+2]=(byte)random.Next(24,48); b[i+3]=255; } } }'
            $locked=$bitmap.LockBits([Drawing.Rectangle]::new(0,0,$Width,$Height),[Drawing.Imaging.ImageLockMode]::WriteOnly,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
            try {
                $bytes=[byte[]]::new($locked.Stride*$Height)
                [GameOcrTexture]::Fill($bytes)
                [Runtime.InteropServices.Marshal]::Copy($bytes,0,$locked.Scan0,$bytes.Length)
            } finally { $bitmap.UnlockBits($locked) }
        }
        $graphics.TextRenderingHint=[Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        foreach($line in $Lines) {
            $font=[Drawing.Font]::new('Microsoft YaHei',[single]$line.size,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
            try {
                $measure=$graphics.MeasureString($line.text,$font)
                $graphics.TranslateTransform([single]$line.x,[single]$line.y)
                $angle=if($line.angle){[single]$line.angle}else{0}
                $graphics.RotateTransform($angle)
                $graphics.DrawString($line.text,$font,[Drawing.Brushes]::White,0,0)
                $points=[Drawing.PointF[]]@([Drawing.PointF]::new(0,0),[Drawing.PointF]::new($measure.Width,0),[Drawing.PointF]::new($measure.Width,$measure.Height),[Drawing.PointF]::new(0,$measure.Height))
                $graphics.Transform.TransformPoints($points)
                $xs=$points | ForEach-Object X; $ys=$points | ForEach-Object Y
                $labels.Add(@{text=$line.text;bbox=@(($xs|Measure-Object -Minimum).Minimum,($ys|Measure-Object -Minimum).Minimum,($xs|Measure-Object -Maximum).Maximum,($ys|Measure-Object -Maximum).Maximum)})
                $graphics.ResetTransform()
            } finally {$font.Dispose()}
        }
        $bitmap.Save((Join-Path $Directory "$Name.png"),[Drawing.Imaging.ImageFormat]::Png)
        $scenarios.Add(@{name=$Name;image="$Name.png";width=$Width;height=$Height;groundTruth=$labels.ToArray()})
    } finally {$graphics.Dispose();$bitmap.Dispose()}
}
if ($TexturedOnly) {
    New-Scene 'textured-hud' 1920 1080 @(
        @{text='Health: 100 / 100';x=70;y=70;size=38},@{text='Quest complete!';x=1250;y=80;size=38},
        @{text='Inventory 12 / 40';x=80;y=840;size=38},@{text='Press ENTER to continue';x=1180;y=870;size=38}) $true
    @{source='synthetic seeded low-contrast noise transport stress case, not a game screenshot';coordinateGroundTruth='Font layout rectangles, not pixel-tight glyph segmentation';scenarios=$scenarios.ToArray()} | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $Directory 'corpus.json') -Encoding utf8
    return
}
New-Scene 'hud-1080' 1920 1080 @(
    @{text='Health: 100 / 100';x=70;y=70;size=38},@{text='Quest complete!';x=1250;y=80;size=38},
    @{text='Inventory 12 / 40';x=80;y=840;size=38},@{text='Press ENTER to continue';x=1180;y=870;size=38})
New-Scene 'hud-1440' 2560 1440 @(
    @{text='Health: 100 / 100';x=90;y=90;size=48},@{text='Quest complete!';x=1700;y=90;size=48},
    @{text='Inventory 12 / 40';x=90;y=1170;size=48},@{text='Press ENTER to continue';x=1650;y=1200;size=48})
New-Scene 'dialogue' 1920 1080 @(@{text='We finally reached the ancient city.';x=280;y=840;size=42},@{text='Press ENTER to continue';x=280;y=920;size=36})
$menu=@();foreach($x in @(80,1100)){foreach($row in 0..9){$menu+=@{text="Menu option $($row+1) - Settings";x=$x;y=50+95*$row;size=34}}}
New-Scene 'dense-menu' 1920 1080 $menu
New-Scene 'mixed-cjk' 1920 1080 @(
    @{text='游戏任务完成 GAME OCR 100';x=70;y=110;size=38},@{text='繁體中文 測試成功';x=1130;y=110;size=38},
    @{text='日本語 こんにちは 世界';x=70;y=810;size=38},@{text='Health: 100 / 100';x=1130;y=810;size=38})
New-Scene 'rotated' 1920 1080 @(@{text='ROTATED TEXT 123';x=100;y=100;size=42;angle=12},@{text='VERTICAL TEXT';x=1600;y=600;size=42;angle=90},@{text='Quest complete!';x=80;y=850;size=38})
New-Scene 'blank' 1920 1080 @()
New-Scene 'cross-boundary' 1920 1080 @(
    @{text='CROSS BOUNDARY: the complete sentence must survive';x=350;y=495;size=38},
    @{text='Top left marker';x=80;y=100;size=38},@{text='Bottom right marker';x=1250;y=850;size=38})
$manifest=@{source='synthetic System.Drawing layouts, not real game screenshots';coordinateGroundTruth='Font layout rectangles, not pixel-tight glyph segmentation';scenarios=$scenarios.ToArray()}
$manifest | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $Directory 'corpus.json') -Encoding utf8
Write-Output "Generated $($scenarios.Count) labelled synthetic layouts."
