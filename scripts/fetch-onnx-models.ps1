param([string]$Directory='.cache/test-models', [ValidateSet('https://huggingface.co','https://hf-mirror.com')][string]$Mirror='https://huggingface.co')
$ErrorActionPreference='Stop'
$models=Get-Content -LiteralPath (Join-Path $PSScriptRoot '../src-tauri/catalog/ppocrv6.json') -Raw | ConvertFrom-Json
New-Item -ItemType Directory -Force -Path $Directory | Out-Null
foreach($model in $models){
    foreach($file in $model.files){
        $path=Join-Path $Directory $file.path
        if((Test-Path -LiteralPath $path) -and (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() -eq $file.sha256){continue}
        $part="$path.part"
        & curl.exe -fL --retry 3 -C - ($file.url.Replace('https://huggingface.co',$Mirror)) -o $part
        if($LASTEXITCODE){throw 'Model download interrupted; rerun to resume'}
        if((Get-Item -LiteralPath $part).Length -ne $file.size -or (Get-FileHash -LiteralPath $part).Hash.ToLowerInvariant() -ne $file.sha256){throw 'Model checksum mismatch'}
        Move-Item -LiteralPath $part -Destination $path -Force
    }
    $model.installed=$true
    $model | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath (Join-Path $Directory "$($model.id).json") -Encoding utf8NoBOM
}
