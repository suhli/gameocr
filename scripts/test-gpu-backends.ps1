param([string]$Manifest = 'artifacts/gpu-backends.json')
$ErrorActionPreference = 'Stop'
$verify = Join-Path $PSScriptRoot 'verify-gpu-backends.ps1'
& $verify -Manifest $Manifest
$root = Split-Path -Parent ([IO.Path]::GetFullPath($Manifest))
$test = Join-Path $root ('.invalid-gpu-' + [Guid]::NewGuid() + '.json')
try {
    foreach ($field in @('size','sha256','url','kind','runtimes','required','version')) {
        $catalog = Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json
        $p = $catalog.packages[0]
        switch ($field) {
            'size' { $p.size++ }
            'sha256' { $p.sha256 = '0' * 64 }
            'url' { $p.url = $p.url -replace '/download/', '/unpublished/' }
            'kind' { $p.kind = 'translation' }
            'runtimes' { $p.runtimes = @('translation') }
            'required' { $p.required = @('missing.exe') }
            'version' { $p.version = '0' * 12 }
        }
        $catalog | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $test -Encoding utf8NoBOM
        $rejected = $false
        try { & $verify -Manifest $test | Out-Null } catch { $rejected = $true }
        if (!$rejected) { throw "Accepted corrupt GPU $field" }
        Write-Output "PASS rejected corrupt GPU $field"
    }
    $catalog = Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json
    $catalog.packages[0].size++
    $catalog | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $test -Encoding utf8NoBOM
    $rejected = $false
    try { & $verify -Manifest $Manifest -EmbeddedManifest $test | Out-Null } catch { $rejected = $true }
    if (!$rejected) { throw 'Accepted inconsistent embedded GPU size' }
    Write-Output 'PASS rejected inconsistent embedded catalog'
} finally { if (Test-Path -LiteralPath $test) { Remove-Item -LiteralPath $test -Force } }
