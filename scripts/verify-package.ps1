param([string]$Directory = 'artifacts/GameOCR-windows-x64-portable')
$ErrorActionPreference = 'Stop'
foreach ($file in @('GameOCR.exe','vcruntime140.dll','msvcp140.dll','workers/cpu/ocr-worker.exe','workers/cuda/ocr-worker.exe','workers/cuda/cudart64_12.dll','workers/cuda/cublas64_12.dll','workers/cuda/cublasLt64_12.dll','licenses/dependencies.txt')) {
    if (!(Test-Path -LiteralPath (Join-Path $Directory $file))) { throw "Missing runtime dependency: $file" }
}
$forbidden = Get-ChildItem -LiteralPath $Directory -Recurse -File | Where-Object { $_.Name -match '\.gguf$|WebView2.*Setup|msedgewebview2.exe|nvcc.exe' }
if ($forbidden) { throw 'Portable unexpectedly contains a model, WebView installer/browser, or development tool.' }
$portableRoot = (Resolve-Path -LiteralPath $Directory).Path
$zip = [IO.Compression.ZipFile]::OpenRead((Join-Path $PWD 'artifacts/GameOCR-windows-x64-portable.zip'))
try {
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in $zip.Entries) {
        if (!$entry.Name) { continue }
        $target = [IO.Path]::GetFullPath((Join-Path $portableRoot $entry.FullName))
        if (!$target.StartsWith($portableRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid ZIP entry path' }
        if (!$seen.Add($target)) { throw "Duplicate ZIP entry: $($entry.FullName)" }
        $expected = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash
        $stream = $entry.Open()
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $actual = ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-','') }
        finally { $sha.Dispose(); $stream.Dispose() }
        if ($actual -ne $expected) { throw "ZIP content differs from tested portable file: $($entry.FullName)" }
    }
    if ($seen.Count -ne @(Get-ChildItem -LiteralPath $portableRoot -Recurse -File).Count) { throw 'ZIP file count differs from portable directory' }
    Write-Output "Verified $($seen.Count) ZIP entries byte-for-byte against the portable directory."
} finally { $zip.Dispose() }
$hashes = foreach ($name in @('GameOCR-windows-x64-portable.zip','GameOCR-windows-x64-setup.exe','GameOCR-windows-x64.msi')) {
    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path 'artifacts' $name)
    "$($hash.Hash.ToLowerInvariant())  $name"
}
$hashes | Set-Content artifacts/SHA256SUMS.txt -Encoding utf8
Write-Output 'Runtime layout and three release artifacts verified; this is not a clean-VM execution test.'
node scripts/verify-native.mjs $Directory
if ($LASTEXITCODE) { exit $LASTEXITCODE }
