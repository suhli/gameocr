param(
    [ValidateSet('cpu','cuda','both')][string]$Backend = 'both',
    [ValidateSet('debug','release')][string]$Profile = 'release',
    [ValidateSet('all','llamacpp','onnx')][string]$Runtime = 'all',
    [string]$CudaArchitectures = '75;80;86;89;120',
    [string]$CudnnPath = $env:CUDNN_PATH,
    [switch]$LocalCuda121Compatibility,
    [int]$Jobs = 4
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location $repo
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vs) { throw 'Visual Studio C++ build tools are required on the build machine.' }
Import-Module (Join-Path $vs 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
$env:CMAKE_GENERATOR = 'Ninja'
$env:LIBCLANG_PATH = if ($env:LIBCLANG_PATH) { $env:LIBCLANG_PATH } else { 'C:\Program Files\LLVM\bin' }
$env:LLAMA_LIB_PROFILE = 'Release'
$env:CMAKE_BUILD_PARALLEL_LEVEL = "$Jobs"
if ($LocalCuda121Compatibility) {
    $env:CMAKE_CUDA_FLAGS = '-allow-unsupported-compiler -D_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH'
}
$backends = if ($Backend -eq 'both') { @('cpu','cuda') } else { @($Backend) }
foreach ($kind in $backends) {
  if ($Runtime -in @('all','llamacpp')) {
    $env:CMAKE_CUDA_ARCHITECTURES = $CudaArchitectures
    $buildArgs = @('build','-p','gameocr-core','--bins','--locked','--jobs',"$Jobs",'--target-dir',"target/workers-$kind")
    if ($Profile -eq 'release') { $buildArgs += '--release' }
    if ($kind -eq 'cuda') { $buildArgs += @('--features','cuda') }
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "$kind worker build failed" }
    $destination = Join-Path $repo "src-tauri/resources/workers/llamacpp/$kind"
    New-Item -ItemType Directory -Force -Path $destination | Out-Null
    Copy-Item -LiteralPath "target/workers-$kind/$Profile/ocr-worker.exe" -Destination $destination
    # Keep the independent CLI in target/workers-*/release; do not duplicate the
    # large CUDA kernel image in every GUI installer.
    $oldCli = Join-Path $destination 'model-test.exe'
    if (Test-Path -LiteralPath $oldCli) { Remove-Item -LiteralPath $oldCli -Force }
    if ($kind -eq 'cuda') {
        $sharedCuda = Join-Path $repo 'src-tauri/resources/workers/cuda-runtime'
        New-Item -ItemType Directory -Force -Path $sharedCuda | Out-Null
        if (!$env:CUDA_PATH) { throw 'CUDA_PATH must point to a CUDA 12.x Toolkit.' }
        foreach ($name in @('cudart64_12.dll','cublas64_12.dll','cublasLt64_12.dll')) {
            Copy-Item -LiteralPath (Join-Path $env:CUDA_PATH "bin/$name") -Destination $sharedCuda
        }
        $license = Join-Path $env:CUDA_PATH 'EULA.txt'
        if (Test-Path -LiteralPath $license) { Copy-Item -LiteralPath $license -Destination (Join-Path $destination 'CUDA-EULA.txt') }
    }
  }
  if ($Runtime -in @('all','onnx')) {
    $onnxArgs=@('build','-p','gameocr-onnx','--bins','--locked','--jobs',"$Jobs",'--target-dir',"target/onnx-$kind")
    if ($Profile -eq 'release') { $onnxArgs+='--release' }
    if ($kind -eq 'cuda') { $onnxArgs+=@('--features','cuda') }
    & cargo @onnxArgs
    if ($LASTEXITCODE) { throw "ONNX $kind build failed" }
    $destination=Join-Path $repo "src-tauri/resources/workers/onnx/$kind"
    New-Item -ItemType Directory -Force -Path $destination | Out-Null
    Copy-Item -LiteralPath "target/onnx-$kind/$Profile/onnx-worker.exe" -Destination $destination
    & (Join-Path $PSScriptRoot 'collect-onnx-runtime.ps1') -Backend $kind -CudnnPath $CudnnPath
  }
}
& (Join-Path $PSScriptRoot 'collect-runtime.ps1') -VsPath $vs
# Remove superseded build resources only after their replacement exists. This
# prevents legacy workers/CUDA DLLs from doubling installer size after upgrade.
foreach($kind in $backends) {
    $workersRoot=[IO.Path]::GetFullPath((Join-Path $repo 'src-tauri/resources/workers'))
    $legacy=Join-Path $workersRoot $kind
    if((Test-Path -LiteralPath $legacy) -and (Test-Path -LiteralPath (Join-Path $workersRoot "llamacpp/$kind/ocr-worker.exe"))) {
        $resolved=(Resolve-Path -LiteralPath $legacy).Path
        if(!$resolved.StartsWith($workersRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Unsafe legacy resource path'}
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
