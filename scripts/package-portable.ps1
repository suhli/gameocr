param([string]$Binary = 'target/release/gameocr.exe')
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location $repo
$destination = Join-Path $repo 'artifacts/GameOCR-windows-x64-portable'
if (Test-Path -LiteralPath $destination) {
    $resolved = (Resolve-Path -LiteralPath $destination).Path
    if (!$resolved.StartsWith((Join-Path $repo 'artifacts') + [IO.Path]::DirectorySeparatorChar)) { throw 'Unsafe packaging directory' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $destination | Out-Null
Copy-Item -LiteralPath $Binary -Destination (Join-Path $destination 'GameOCR.exe')
Copy-Item -LiteralPath 'src-tauri/resources/workers' -Destination (Join-Path $destination 'workers') -Recurse
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
CUDA requires a compatible NVIDIA driver; CPU inference is always available.
This is an unsigned build; clean Windows 11 validation is documented separately.
'@ | Set-Content -LiteralPath (Join-Path $destination 'README.txt') -Encoding utf8
foreach ($required in @('GameOCR.exe','workers/llamacpp/cpu/ocr-worker.exe','workers/llamacpp/cuda/ocr-worker.exe','workers/onnx/cpu/onnx-worker.exe','workers/onnx/cuda/onnx-worker.exe','workers/onnx/cpu/onnxruntime.dll','workers/onnx/cuda/onnxruntime_providers_cuda.dll','workers/cuda-runtime/cudart64_12.dll','workers/cuda-runtime/cublas64_12.dll','workers/cuda-runtime/cublasLt64_12.dll','workers/cuda-runtime/cudnn64_9.dll','vcruntime140.dll')) {
    if (!(Test-Path -LiteralPath (Join-Path $destination $required))) { throw "Portable is incomplete: $required" }
}
Compress-Archive -Path (Join-Path $destination '*') -DestinationPath 'artifacts/GameOCR-windows-x64-portable.zip' -Force
Get-FileHash -Algorithm SHA256 -LiteralPath 'artifacts/GameOCR-windows-x64-portable.zip'
