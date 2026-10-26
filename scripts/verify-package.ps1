param([string]$Directory = '', [ValidateSet('both','cpu','ondemand')][string]$Backend = 'ondemand', [switch]$PortableOnly)
$ErrorActionPreference = 'Stop'
if (!$Directory) { $Directory = if ($Backend -eq 'cpu') { 'artifacts/GameOCR-windows-x64-cpu-portable' } elseif ($Backend -eq 'ondemand') { 'artifacts/GameOCR-windows-x64-ondemand-portable' } else { 'artifacts/GameOCR-windows-x64-portable' } }
$requiredFiles = @('GameOCR.exe','vcruntime140.dll','msvcp140.dll','workers/llamacpp/cpu/ocr-worker.exe','workers/onnx/cpu/onnx-worker.exe','workers/onnx/cpu/onnxruntime.dll','licenses/dependencies.txt','licenses/onnxruntime-LICENSE.txt')
$requiredFiles += 'workers/translation/cpu/translation-worker.exe'
if ($Backend -eq 'both') { $requiredFiles += 'workers/translation/cuda/translation-worker.exe' }
if ($Backend -eq 'both') { $requiredFiles += @('workers/llamacpp/cuda/ocr-worker.exe','workers/onnx/cuda/onnx-worker.exe','workers/onnx/cuda/onnxruntime_providers_cuda.dll','workers/cuda-runtime/cudart64_12.dll','workers/cuda-runtime/cublas64_12.dll','workers/cuda-runtime/cublasLt64_12.dll','workers/cuda-runtime/cudnn64_9.dll','licenses/cuDNN-LICENSE.txt') }
if ($Backend -eq 'ondemand') {
    $requiredFiles += @('workers/onnx/cuda/onnx-worker.exe','gpu-downloads.json')
    if (Test-Path -LiteralPath (Join-Path $Directory 'workers/cuda-runtime')) { throw 'On-demand package must not bundle NVIDIA runtime libraries.' }
    if (Test-Path -LiteralPath (Join-Path $Directory 'workers/llamacpp/cuda')) { throw 'On-demand package must not bundle the llama.cpp CUDA worker.' }
    if (Test-Path -LiteralPath (Join-Path $Directory 'workers/translation/cuda')) { throw 'On-demand package must not bundle the translation CUDA worker.' }
    $gpuDlls = Get-ChildItem -LiteralPath (Join-Path $Directory 'workers/onnx/cuda') -Filter '*.dll'
    if ($gpuDlls) { throw 'On-demand package must not bundle ONNX GPU DLLs.' }
}
foreach ($file in $requiredFiles) {
    if (!(Test-Path -LiteralPath (Join-Path $Directory $file))) { throw "Missing runtime dependency: $file" }
}
$forbidden = Get-ChildItem -LiteralPath $Directory -Recurse -File | Where-Object { $_.Name -match '\.(gguf|onnx)$|WebView2.*Setup|msedgewebview2.exe|nvcc.exe' }
if ($forbidden) { throw 'Portable unexpectedly contains a model, WebView installer/browser, or development tool.' }
if ($Backend -eq 'cpu') {
    if (Test-Path -LiteralPath (Join-Path $Directory 'gpu-downloads.json')) { throw 'CPU-only package unexpectedly enables GPU downloads.' }
    $gpuFiles = Get-ChildItem -LiteralPath $Directory -Recurse -File | Where-Object { $_.Name -match '(cuda|cublas|cudnn|cufft|nvrtc|nvJitLink).*\.dll$' -or $_.Directory.Name -eq 'cuda' }
    if ($gpuFiles) { throw 'CPU package contains GPU binaries.' }
}
$portableRoot = (Resolve-Path -LiteralPath $Directory).Path
$zip = [IO.Compression.ZipFile]::OpenRead("$portableRoot.zip")
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
$archiveName = if ($Backend -eq 'ondemand') { 'GameOCR-windows-x64-ondemand-portable.zip' } else { 'GameOCR-windows-x64-portable.zip' }
$artifacts = if ($Backend -eq 'cpu') { @('GameOCR-windows-x64-cpu-portable.zip') } elseif ($PortableOnly) { @($archiveName) } else { @($archiveName,'GameOCR-windows-x64-setup.exe','GameOCR-windows-x64.msi') }
$hashes = foreach ($name in $artifacts) {
    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path 'artifacts' $name)
    "$($hash.Hash.ToLowerInvariant())  $name"
}
$hashFile = if ($Backend -eq 'cpu') { 'artifacts/SHA256SUMS-cpu.txt' } elseif ($Backend -eq 'ondemand') { 'artifacts/SHA256SUMS-ondemand.txt' } elseif ($PortableOnly) { 'artifacts/SHA256SUMS-portable.txt' } else { 'artifacts/SHA256SUMS.txt' }
$hashes | Set-Content $hashFile -Encoding utf8
Write-Output 'Runtime layout and release artifact hashes verified; this is not a clean-VM execution test.'
if ($Backend -eq 'ondemand') { node scripts/verify-native.mjs $Directory --on-demand } else { node scripts/verify-native.mjs $Directory }
if ($LASTEXITCODE) { exit $LASTEXITCODE }
