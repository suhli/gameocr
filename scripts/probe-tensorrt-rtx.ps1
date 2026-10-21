param([string]$Output='docs/test-results/tensorrt-rtx-probe.json')
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$probe=Join-Path $repo 'target/onnx-cuda/release/onnx-probe.exe'
$dll=Join-Path $repo 'src-tauri/resources/workers/onnx/cuda/onnxruntime.dll'
$previousOrt=$env:ORT_DYLIB_PATH
try {
    $env:ORT_DYLIB_PATH=$dll
    $providers=(& $probe | ConvertFrom-Json)
    if($LASTEXITCODE){throw 'ORT capability probe failed'}
    $gpu=& nvidia-smi.exe --query-gpu=name,compute_cap,driver_version --format=csv,noheader
    $supported=$false
    if($LASTEXITCODE -eq 0){$fields=$gpu.Split(',');$supported=([double]::Parse($fields[1].Trim(),[Globalization.CultureInfo]::InvariantCulture) -ge 8.0)}
    $report=[ordered]@{executedAt=(Get-Date).ToUniversalTime().ToString('o');gpu=$gpu;providers=$providers;ortVersion=(Get-Item -LiteralPath $dll).VersionInfo.FileVersion;ortSha256=(Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash;hardwareMeetsAmpereMinimum=$supported;modelBuildTested=$false;cacheCreated=$false;defaultChanged=$false;status=if(!$supported){'unsupported hardware: TensorRT RTX excludes Turing'}elseif(!$providers.tensorRtRtxCompiled){'provider/plugin unavailable; CUDA retained'}else{'capability detected; model build and accuracy still untested'};reference='https://nvidia.github.io/TensorRT-RTX-EP-ABI/prerequisites.html'}
    $report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $repo $Output) -Encoding utf8
    $report | ConvertTo-Json -Depth 10
} finally {$env:ORT_DYLIB_PATH=$previousOrt}
