param([string]$Directory='tests/fixtures/small-text')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
$source=Join-Path $Directory 'dense-3840.png'
foreach($variant in @('changed','removed','scrolled')) {
    $original=[Drawing.Bitmap]::new((Resolve-Path -LiteralPath $source).Path)
    $bitmap=[Drawing.Bitmap]::new($original.Width,$original.Height)
    $g=[Drawing.Graphics]::FromImage($bitmap)
    $background=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(24,32,48))
    try {
        $g.Clear($background.Color)
        if($variant -eq 'scrolled') {
            $g.DrawImageUnscaled($original,0,27)
        } else {
            $g.DrawImageUnscaled($original,0,0)
            $g.FillRectangle($background,45,65,300,35)
            if($variant -eq 'changed') {
                $g.TextRenderingHint=[Drawing.Text.TextRenderingHint]::AntiAliasGridFit
                $font=[Drawing.Font]::new('Microsoft YaHei',16,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
                try{$g.DrawString('Item 0-0 Health 79',$font,[Drawing.Brushes]::White,50,70)}finally{$font.Dispose()}
            }
        }
        $bitmap.Save((Join-Path $Directory "dense-3840-$variant.png"),[Drawing.Imaging.ImageFormat]::Png)
    }finally{$g.Dispose();$bitmap.Dispose();$original.Dispose();$background.Dispose()}
}
