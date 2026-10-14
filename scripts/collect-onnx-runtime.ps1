param([ValidateSet('cpu','cuda')][string]$Backend='cpu', [string]$CudnnPath=$env:CUDNN_PATH)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$version='1.24.3'
$suffix=if($Backend -eq 'cuda'){'-gpu'}else{''}
$name="onnxruntime-win-x64$suffix-$version"
$expected=if($Backend -eq 'cuda'){'f2af33fea71f477027961b8027e7496ab01c6b962ae4f596bf8163192b03b326'}else{'4fbfb85d0e9de9bb6fb8a9866a7cb477cbad404d889b236931bf3f5d547e5f48'}
$cache=Join-Path $repo '.cache/native'
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$zip=Join-Path $cache "$name.zip"
if(!(Test-Path -LiteralPath $zip)) { Invoke-WebRequest "https://github.com/microsoft/onnxruntime/releases/download/v$version/$name.zip" -OutFile $zip }
if((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected){throw 'ONNX Runtime archive SHA-256 mismatch'}
Expand-Archive -LiteralPath $zip -DestinationPath $cache -Force
$destination=Join-Path $repo "src-tauri/resources/workers/onnx/$Backend"
New-Item -ItemType Directory -Force -Path $destination | Out-Null
foreach($dll in @('onnxruntime.dll','onnxruntime_providers_shared.dll','onnxruntime_providers_cuda.dll')) {
    $source=Join-Path $cache "$name/lib/$dll"
    if(Test-Path -LiteralPath $source){Copy-Item -LiteralPath $source -Destination $destination}
}
$licenses=Join-Path $repo 'src-tauri/resources/licenses'
New-Item -ItemType Directory -Force -Path $licenses | Out-Null
foreach($file in @('LICENSE','ThirdPartyNotices.txt')) { Copy-Item -LiteralPath (Join-Path $cache "$name/$file") -Destination (Join-Path $licenses "onnxruntime-$file.txt") }
if($Backend -eq 'cuda') {
    if(!$env:CUDA_PATH -or !$CudnnPath){throw 'CUDA_PATH (CUDA 12.8+) and CUDNN_PATH (cuDNN 9.8+) are required for GPU distribution'}
    $shared=Join-Path $repo 'src-tauri/resources/workers/cuda-runtime'
    New-Item -ItemType Directory -Force -Path $shared | Out-Null
    foreach($name in @('cudart64_12.dll','cublas64_12.dll','cublasLt64_12.dll','cufft64_11.dll')){Copy-Item -LiteralPath (Join-Path $env:CUDA_PATH "bin/$name") -Destination $shared}
    Get-ChildItem -Path (Join-Path $env:CUDA_PATH 'bin/nvrtc*.dll'),(Join-Path $env:CUDA_PATH 'bin/nvJitLink*.dll') -ErrorAction SilentlyContinue | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $shared }
    $cudnnDlls=Get-ChildItem -LiteralPath $CudnnPath -Recurse -Filter 'cudnn*64_9.dll' -File
    if(!$cudnnDlls){throw 'cuDNN 9 DLLs missing'}
    $cudnnDlls | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $shared }
    $cudnnLicense=Get-ChildItem -LiteralPath $CudnnPath -Recurse -File | Where-Object Name -Match 'LICENSE|EULA' | Select-Object -First 1
    if(!$cudnnLicense){throw 'cuDNN redistribution license missing'}
    Copy-Item -LiteralPath $cudnnLicense.FullName -Destination (Join-Path $licenses 'cuDNN-LICENSE.txt')
    Copy-Item -LiteralPath (Join-Path $env:CUDA_PATH 'EULA.txt') -Destination (Join-Path $licenses 'CUDA-EULA.txt')
}
