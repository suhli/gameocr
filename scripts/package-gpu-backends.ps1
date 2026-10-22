param([string]$ReleaseTag = '')
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location $repo
$worker = Join-Path $repo 'src-tauri/resources/workers/llamacpp/cuda/ocr-worker.exe'
$workerHash = (Get-FileHash -LiteralPath $worker -Algorithm SHA256).Hash.ToLowerInvariant()
if (!$ReleaseTag) {
    $ReleaseTag = if ($env:GITHUB_REF_TYPE -eq 'tag') { $env:GITHUB_REF_NAME } else { 'gpu-' + $workerHash.Substring(0,12) }
}
if ($ReleaseTag -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$') { throw 'Unsafe release tag' }
$asset = "GameOCR-llamacpp-cuda-windows-x64-$($workerHash.Substring(0,12)).zip"
$stage = Join-Path $repo ('.cache/gpu-backend-' + [Guid]::NewGuid())
$archive = Join-Path $repo "artifacts/$asset"
New-Item -ItemType Directory -Force -Path $stage,(Join-Path $repo 'artifacts') | Out-Null
try {
    Copy-Item -LiteralPath $worker -Destination $stage
    Copy-Item -LiteralPath 'LICENSE' -Destination (Join-Path $stage 'GameOCR-LICENSE.txt')
    Copy-Item -LiteralPath 'src-tauri/resources/licenses/dependencies.txt' -Destination (Join-Path $stage 'dependency-LICENSES.txt')
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $archive -Force
} finally {
    $resolved = [IO.Path]::GetFullPath($stage)
    if (!$resolved.StartsWith((Join-Path $repo '.cache') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe staging path' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
$package = @{
    id='gameocr-llamacpp-cuda'; name='GameOCR llama.cpp CUDA'; kind='llamacpp';
    version=$workerHash.Substring(0,12); runtimes=@('llamacpp'); required=@('ocr-worker.exe');
    url="https://github.com/suhli/gameocr/releases/download/$ReleaseTag/$asset";
    sha256=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant();
    size=(Get-Item -LiteralPath $archive).Length
}
$manifest = Join-Path $repo 'artifacts/gpu-backends.json'
@{packages=@($package)} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifest -Encoding utf8NoBOM
$env:GAMEOCR_GPU_MANIFEST = $manifest
if ($env:GITHUB_ENV) { "GAMEOCR_GPU_MANIFEST=$manifest" | Add-Content -LiteralPath $env:GITHUB_ENV }
"$($package.sha256)  $asset" | Set-Content -LiteralPath 'artifacts/SHA256SUMS-gpu-backends.txt' -Encoding utf8NoBOM
Write-Output "Prepared $asset ($($package.size) bytes). Manifest: $manifest"
Write-Output "Publish this asset with release $ReleaseTag before distributing the matching base package."
