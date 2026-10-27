param([string]$Manifest = 'artifacts/gpu-backends.json', [string]$EmbeddedManifest = '', [switch]$CheckPublished)
$ErrorActionPreference = 'Stop'
$catalog = Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json
$assetRoot = Split-Path -Parent ([IO.Path]::GetFullPath($Manifest))
if ($catalog.packages.Count -ne 2) { throw 'Expected independent OCR and translation GPU packages' }
$ids = [Collections.Generic.HashSet[string]]::new()
$embedded = if ($EmbeddedManifest) { Get-Content -LiteralPath $EmbeddedManifest -Raw | ConvertFrom-Json } else { $null }
if ($embedded -and $embedded.packages.Count -ne $catalog.packages.Count) { throw 'Embedded GPU catalog count differs' }
foreach ($p in $catalog.packages) {
    if ($p.kind -notin @('llamacpp','translation') -or !$ids.Add($p.id) -or $p.id -ne "gameocr-$($p.kind)-cuda") { throw 'Invalid or duplicate GPU component identity' }
    $binary = if ($p.kind -eq 'translation') { 'translation-worker.exe' } else { 'ocr-worker.exe' }
    if (@($p.runtimes).Count -ne 1 -or $p.runtimes[0] -ne $p.kind -or @($p.required).Count -ne 1 -or $p.required[0] -ne $binary) { throw 'GPU runtime or required worker mismatch' }
    if ($p.sha256 -cnotmatch '^[a-f0-9]{64}$' -or $p.version -cnotmatch '^[a-f0-9]{12}$') { throw 'Invalid GPU digest/version' }
    $asset = "GameOCR-$($p.kind)-cuda-windows-x64-$($p.sha256.Substring(0,12)).zip"
    $escaped = [Regex]::Escape($asset)
    if ($p.url -cnotmatch "^https://github.com/suhli/gameocr/releases/download/[A-Za-z0-9][A-Za-z0-9._-]{0,95}/$escaped`$") { throw 'GPU URL does not match pinned asset identity' }
    $archive = Join-Path $assetRoot $asset
    if ((Get-Item -LiteralPath $archive).Length -ne $p.size -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $p.sha256) { throw 'GPU archive size/SHA-256 mismatch' }
    if ($embedded) {
        $matches = @($embedded.packages | Where-Object id -eq $p.id)
        if ($matches.Count -ne 1) { throw 'Embedded component missing/duplicated' }
        foreach ($field in @('id','kind','version','url','sha256','size','runtimes','required')) {
            if (($matches[0].$field | ConvertTo-Json -Compress) -cne ($p.$field | ConvertTo-Json -Compress)) { throw "Embedded GPU field mismatch: $field" }
        }
    }
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $workers = @($zip.Entries | Where-Object FullName -ceq $binary)
        if ($workers.Count -ne 1 -or !@($zip.Entries | Where-Object Name -match 'LICENSE|NOTICES').Count) { throw 'GPU ZIP missing unique worker or license' }
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($entry in $zip.Entries) {
            if (!$names.Add($entry.FullName) -or $entry.FullName -match '[/\\]|\.\.' -or $entry.Name -match '\.(gguf|onnx|dll)$') { throw 'Unexpected GPU ZIP path or bundled dependency' }
        }
        $stream = $workers[0].Open()
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $workerHash = [Convert]::ToHexString($sha.ComputeHash($stream)).ToLowerInvariant() }
        finally { $sha.Dispose(); $stream.Dispose() }
        if ($workerHash.Substring(0,12) -cne $p.version) { throw 'GPU worker version differs from archive contents' }
    } finally { $zip.Dispose() }
    Write-Output "PASS local GPU ZIP/catalog: $asset SHA256=$($p.sha256) size=$($p.size)"
    if ($CheckPublished) {
        # Availability alone is not a verified download or model-load acceptance.
        $response = Invoke-WebRequest -Uri $p.url -Method Head -MaximumRedirection 5 -TimeoutSec 60
        if ($response.StatusCode -ne 200) { throw "GPU Release unavailable: $($p.url)" }
        Write-Output "PASS public URL available: $($p.url); download/SHA/load still require separate acceptance."
    }
}
if (!$CheckPublished) { Write-Output 'SKIPPED public Release/download/load validation (offline verification only).' }
