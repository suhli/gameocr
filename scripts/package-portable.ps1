param([string]$Binary = 'target/release/gameocr.exe', [ValidateSet('both','cpu','ondemand')][string]$Backend = 'ondemand')
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location $repo
$name = if ($Backend -eq 'cpu') { 'GameOCR-windows-x64-cpu-portable' } elseif ($Backend -eq 'ondemand') { 'GameOCR-windows-x64-ondemand-portable' } else { 'GameOCR-windows-x64-portable' }
$destination = Join-Path $repo "artifacts/$name"
$archive = Join-Path $repo "artifacts/$name.zip"
if (Test-Path -LiteralPath $destination) {
    $resolved = (Resolve-Path -LiteralPath $destination).Path
    if (!$resolved.StartsWith((Join-Path $repo 'artifacts') + [IO.Path]::DirectorySeparatorChar)) { throw 'Unsafe packaging directory' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $destination | Out-Null
Copy-Item -LiteralPath $Binary -Destination (Join-Path $destination 'GameOCR.exe')
foreach ($runtime in @('llamacpp','onnx','translation')) {
    $runtimeDir = Join-Path $destination "workers/$runtime"
    New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
    $devices = if ($Backend -eq 'both') { @('cpu','cuda') } else { @('cpu') }
    foreach ($device in $devices) {
        Copy-Item -LiteralPath "src-tauri/resources/workers/$runtime/$device" -Destination (Join-Path $runtimeDir $device) -Recurse
    }
}
if ($Backend -eq 'ondemand') {
    $bootstrap = Join-Path $destination 'workers/onnx/cuda'
    New-Item -ItemType Directory -Force -Path $bootstrap | Out-Null
    Copy-Item -LiteralPath 'src-tauri/resources/workers/onnx/cuda/onnx-worker.exe' -Destination $bootstrap
    Copy-Item -LiteralPath 'src-tauri/gpu-downloads.json' -Destination $destination
    if (!(Test-Path -LiteralPath 'artifacts/gpu-backends.json')) { throw 'Prepare GPU assets before building: scripts/package-gpu-backends.ps1' }
    $catalogCheck = Join-Path $repo ('.cache/gpu-catalog-check-' + [Guid]::NewGuid() + '.json')
    try {
        $process = Start-Process -FilePath (Resolve-Path -LiteralPath $Binary).Path -ArgumentList @('--gpu-catalog',('"' + $catalogCheck + '"')) -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode) { throw 'Unable to read embedded GPU catalog' }
        $expected = Get-Content -LiteralPath 'artifacts/gpu-backends.json' -Raw | ConvertFrom-Json
        $actual = Get-Content -LiteralPath $catalogCheck -Raw | ConvertFrom-Json
        if ($actual.packages.Count -ne $expected.packages.Count) { throw 'EXE GPU catalog count differs; rebuild with GAMEOCR_GPU_MANIFEST.' }
        foreach ($package in $expected.packages) {
            $match = @($actual.packages | Where-Object id -eq $package.id)
            if ($match.Count -ne 1 -or $match[0].sha256 -ne $package.sha256 -or $match[0].url -ne $package.url) { throw 'EXE GPU catalog does not match companion assets; rebuild with GAMEOCR_GPU_MANIFEST.' }
        }
    } finally { if (Test-Path -LiteralPath $catalogCheck) { Remove-Item -LiteralPath $catalogCheck -Force } }
}
if ($Backend -eq 'both') {
    Copy-Item -LiteralPath 'src-tauri/resources/workers/cuda-runtime' -Destination (Join-Path $destination 'workers/cuda-runtime') -Recurse
}
Copy-Item -LiteralPath 'src-tauri/resources/licenses' -Destination (Join-Path $destination 'licenses') -Recurse
Copy-Item -Path 'src-tauri/resources/runtime/*.dll' -Destination $destination
Copy-Item -LiteralPath 'LICENSE','THIRD-PARTY-NOTICES.md' -Destination $destination
@'
GameOCR for Windows 11 x64
Run GameOCR.exe directly. No installation or administrator rights required.
Uses the existing system WebView2 Runtime. No WebView2 installer, bootstrapper,
pre-install check, bundled browser, or runtime download is included.
Model weights are not included. Download/import GGUF + mmproj or PP-OCRv6 ONNX in the app.
Settings and models: %LOCALAPPDATA%\GameOCR
This is an unsigned build; clean Windows 11 validation is documented separately.
'@ | Set-Content -LiteralPath (Join-Path $destination 'README.txt') -Encoding utf8
$backendNote = if ($Backend -eq 'cpu') { 'CPU-only package: no GPU backends. Auto falls back to CPU; explicit CUDA reports a missing backend.' } elseif ($Backend -eq 'ondemand') { 'GPU backends download on first explicit CUDA model load: ONNX libraries from Microsoft, llama.cpp GPU Worker from the matching GameOCR Release, and CUDA/cuDNN from NVIDIA. Auto does not download. Downloads support cancellation/resume and SHA-256 verification. Cache: %LOCALAPPDATA%\GameOCR\runtimes. A compatible NVIDIA driver must already be installed. Optional matching offline archives may be placed in runtime-downloads beside GameOCR.exe.' } else { 'CPU and CUDA workers and NVIDIA runtime libraries included for offline use. CUDA requires a compatible NVIDIA driver.' }
Add-Content -LiteralPath (Join-Path $destination 'README.txt') -Value $backendNote -Encoding utf8
$requiredFiles = @('GameOCR.exe','workers/llamacpp/cpu/ocr-worker.exe','workers/onnx/cpu/onnx-worker.exe','workers/onnx/cpu/onnxruntime.dll','vcruntime140.dll')
$requiredFiles += 'workers/translation/cpu/translation-worker.exe'
if ($Backend -eq 'both') { $requiredFiles += 'workers/translation/cuda/translation-worker.exe' }
if ($Backend -eq 'both') { $requiredFiles += @('workers/llamacpp/cuda/ocr-worker.exe','workers/onnx/cuda/onnx-worker.exe','workers/onnx/cuda/onnxruntime_providers_cuda.dll') }
if ($Backend -eq 'both') { $requiredFiles += @('workers/cuda-runtime/cudart64_12.dll','workers/cuda-runtime/cublas64_12.dll','workers/cuda-runtime/cublasLt64_12.dll','workers/cuda-runtime/cudnn64_9.dll') }
foreach ($required in $requiredFiles) {
    if (!(Test-Path -LiteralPath (Join-Path $destination $required))) { throw "Portable is incomplete: $required" }
}
Compress-Archive -Path (Join-Path $destination '*') -DestinationPath $archive -Force
Get-FileHash -Algorithm SHA256 -LiteralPath $archive
