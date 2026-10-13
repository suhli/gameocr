param([string]$Directory = 'artifacts/GameOCR-windows-x64-portable')
$ErrorActionPreference = 'Stop'
foreach ($file in @('GameOCR.exe','vcruntime140.dll','msvcp140.dll','workers/cpu/ocr-worker.exe','workers/cuda/ocr-worker.exe','workers/cuda/cudart64_12.dll','workers/cuda/cublas64_12.dll','workers/cuda/cublasLt64_12.dll','licenses/dependencies.txt')) {
    if (!(Test-Path -LiteralPath (Join-Path $Directory $file))) { throw "Missing runtime dependency: $file" }
}
$forbidden = Get-ChildItem -LiteralPath $Directory -Recurse -File | Where-Object { $_.Name -match '\.gguf$|WebView2.*Setup|msedgewebview2.exe|nvcc.exe' }
if ($forbidden) { throw 'Portable unexpectedly contains a model, WebView installer/browser, or development tool.' }
$hashes = foreach ($name in @('GameOCR-windows-x64-portable.zip','GameOCR-windows-x64-setup.exe','GameOCR-windows-x64.msi')) {
    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path 'artifacts' $name)
    "$($hash.Hash.ToLowerInvariant())  $name"
}
$hashes | Set-Content artifacts/SHA256SUMS.txt -Encoding utf8
Write-Output 'Runtime layout and three release artifacts verified; this is not a clean-VM execution test.'
node scripts/verify-native.mjs $Directory
if ($LASTEXITCODE) { exit $LASTEXITCODE }
