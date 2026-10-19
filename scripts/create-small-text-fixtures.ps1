param([string]$Directory='tests/fixtures/small-text')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force -Path $Directory | Out-Null
$scenes=[Collections.Generic.List[object]]::new()
foreach($dimensions in @(@(2560,1440),@(3840,2160))) {
    $w=$dimensions[0];$h=$dimensions[1]
    $bitmap=[Drawing.Bitmap]::new($w,$h);$g=[Drawing.Graphics]::FromImage($bitmap)
    $labels=[Collections.Generic.List[object]]::new()
    try {
        $g.Clear([Drawing.Color]::FromArgb(24,32,48))
        $g.TextRenderingHint=[Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $lines=@()
        foreach($col in 0..3){foreach($row in 0..11){
            $lines+=@{text="Item $col-$row Health 80";x=50+$col*[int]($w/4);y=70+$row*36;size=16}
        }}
        $lines+=@{text='CROSS TILE BOUNDARY every character stays in this long sentence';x=350;y=650;size=22}
        foreach($row in 0..3){$lines+=@{text="Quest $row Find the hidden key";x=[int]($w/2)+40;y=[int]($h*0.75)+$row*34;size=16}}
        foreach($line in $lines){
            $font=[Drawing.Font]::new('Microsoft YaHei',[single]$line.size,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
            try{
                $size=$g.MeasureString($line.text,$font)
                $g.DrawString($line.text,$font,[Drawing.Brushes]::White,[single]$line.x,[single]$line.y)
                $labels.Add(@{text=$line.text;bbox=@($line.x,$line.y,($line.x+$size.Width),($line.y+$size.Height))})
            }finally{$font.Dispose()}
        }
        $name="dense-$w"
        $bitmap.Save((Join-Path $Directory "$name.png"),[Drawing.Imaging.ImageFormat]::Png)
        $scenes.Add(@{name=$name;image="$name.png";width=$w;height=$h;groundTruth=$labels.ToArray()})
    }finally{$g.Dispose();$bitmap.Dispose()}
}
@{source='Synthetic high-resolution UI; no real-game accuracy claim';coordinateGroundTruth='Approximate font layout rectangles';scenarios=$scenes.ToArray()} | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $Directory 'corpus.json') -Encoding utf8
