param([string]$VsPath)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (!$VsPath) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $VsPath = & $vswhere -latest -products '*' -property installationPath
}
$crt = Get-ChildItem -Path (Join-Path $VsPath 'VC/Redist/MSVC/*/x64/Microsoft.VC143.CRT') -Directory | Select-Object -First 1
if (!$crt) { throw 'MSVC redistributable CRT directory not found.' }
$runtime = Join-Path $repo 'src-tauri/resources/runtime'
New-Item -ItemType Directory -Force -Path $runtime | Out-Null
Copy-Item -Path (Join-Path $crt.FullName '*.dll') -Destination $runtime
foreach ($kind in @('llamacpp/cpu','llamacpp/cuda','onnx/cpu','onnx/cuda','translation/cpu','translation/cuda')) {
    $worker = Join-Path $repo "src-tauri/resources/workers/$kind"
    if (Test-Path -LiteralPath $worker) { Copy-Item -Path (Join-Path $runtime '*.dll') -Destination $worker }
}
