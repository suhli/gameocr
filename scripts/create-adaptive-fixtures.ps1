# Held-out synthetic layouts: never used to seed the adaptive training archive.
param([string]$Directory='tests/fixtures/adaptive')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force -Path $Directory | Out-Null
$scenes=[Collections.Generic.List[object]]::new()
function Scene($Name,$Group,$Lines,[bool]$Light=$false) {
    $bitmap=[Drawing.Bitmap]::new(1920,1080);$g=[Drawing.Graphics]::FromImage($bitmap)
    $labels=[Collections.Generic.List[object]]::new()
    try {
        $g.Clear($(if($Light){[Drawing.Color]::FromArgb(218,226,211)}else{[Drawing.Color]::FromArgb(20,28,44)}))
        $ink=if($Light){[Drawing.Brushes]::Black}else{[Drawing.Brushes]::White}
        $g.TextRenderingHint=[Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        foreach($line in $Lines){
            $font=[Drawing.Font]::new('Microsoft YaHei',[single]$line.size,[Drawing.FontStyle]::Regular,[Drawing.GraphicsUnit]::Pixel)
            try{$size=$g.MeasureString($line.text,$font);$g.DrawString($line.text,$font,$ink,[single]$line.x,[single]$line.y)
                $labels.Add(@{text=$line.text;bbox=@($line.x,$line.y,($line.x+$size.Width),($line.y+$size.Height))})
            }finally{$font.Dispose()}
        }
        $bitmap.Save((Join-Path $Directory "$Name.png"),[Drawing.Imaging.ImageFormat]::Png)
        $scenes.Add(@{name=$Name;group=$Group;image="$Name.png";width=1920;height=1080;groundTruth=$labels.ToArray()})
    }finally{$g.Dispose();$bitmap.Dispose()}
}
Scene 'heldout-hud' 'known-layout-new-text' @(@{text='Health: 88 / 100';x=74;y=74;size=38},@{text='Quest updated!';x=1254;y=84;size=38},@{text='Inventory 18 / 40';x=84;y=844;size=38},@{text='Press SPACE to continue';x=1184;y=874;size=38})
Scene 'heldout-dialogue' 'known-layout-new-text' @(@{text='The journey begins in a distant land.';x=284;y=844;size=42},@{text='Press SPACE to continue';x=284;y=924;size=36})
$menu=@();foreach($x in @(84,1104)){foreach($row in 0..9){$menu+=@{text="Menu option $($row+1) - Controls";x=$x;y=54+95*$row;size=34}}}
Scene 'heldout-menu' 'known-layout-new-text' $menu
Scene 'style-transfer' 'different-style-similar-workload' @(@{text='游戏任务开始 GAME OCR 200';x=70;y=110;size=38},@{text='繁體中文 遊戲世界';x=1130;y=110;size=38},@{text='日本語 こんにちは 世界';x=70;y=810;size=38},@{text='Health: 80 / 100';x=1130;y=810;size=38}) $true
Scene 'novel-center' 'unseen-layout' @(@{text='NEW ADVENTURE';x=730;y=210;size=44},@{text='Choose your destination';x=650;y=360;size=36},@{text='Mountain village';x=710;y=490;size=36},@{text='Return to the forest';x=690;y=620;size=36},@{text='Continue';x=840;y=780;size=36})
Scene 'heldout-boundary' 'known-layout-new-text' @(@{text='CROSS BOUNDARY: every character should remain complete';x=300;y=495;size=38},@{text='Top left checkpoint';x=80;y=100;size=38},@{text='Bottom right checkpoint';x=1250;y=850;size=38})
Scene 'heldout-blank' 'unseen-background' @() $true
@{source='Held-out synthetic System.Drawing layouts with new text, positions and polarity; not real games';coordinateGroundTruth='Font layout rectangles; approximate, not glyph-tight';learningCorpus='../parallel/corpus.json';scenarios=$scenes.ToArray()} | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $Directory 'corpus.json') -Encoding utf8
Write-Output "Generated $($scenes.Count) held-out synthetic layouts."
