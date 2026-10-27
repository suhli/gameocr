param([string]$ReleaseTag = '')
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location $repo
$bootstrap = Join-Path $repo 'src-tauri/resources/workers/onnx/bootstrap'
New-Item -ItemType Directory -Force -Path $bootstrap | Out-Null
Copy-Item -LiteralPath 'src-tauri/resources/workers/onnx/cuda/onnx-worker.exe' -Destination $bootstrap
if (!$ReleaseTag -and $env:GITHUB_REF_TYPE -eq 'tag') { $ReleaseTag = $env:GITHUB_REF_NAME }
if ($ReleaseTag -and $ReleaseTag -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$') { throw 'Unsafe release tag' }
$packages = @()
$checksums = @()
foreach ($runtime in @('llamacpp','translation')) {
    $binary = if ($runtime -eq 'translation') { 'translation-worker.exe' } else { 'ocr-worker.exe' }
    $worker = Join-Path $repo "src-tauri/resources/workers/$runtime/cuda/$binary"
    $workerHash = (Get-FileHash -LiteralPath $worker -Algorithm SHA256).Hash.ToLowerInvariant()
    $stage = Join-Path $repo ('.cache/gpu-backend-' + [Guid]::NewGuid())
    $archive = "$stage.zip"
    New-Item -ItemType Directory -Force -Path $stage,(Join-Path $repo 'artifacts') | Out-Null
    try {
        Copy-Item -LiteralPath $worker -Destination $stage
        Copy-Item -LiteralPath 'LICENSE' -Destination (Join-Path $stage 'GameOCR-LICENSE.txt')
        Copy-Item -LiteralPath 'src-tauri/resources/licenses/dependencies.txt' -Destination (Join-Path $stage 'dependency-LICENSES.txt')
        Copy-Item -LiteralPath 'src-tauri/resources/licenses/CUDA-EULA.txt','THIRD-PARTY-NOTICES.md' -Destination $stage
        Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $archive -Force
    } finally {
        $resolved = [IO.Path]::GetFullPath($stage)
        if (!$resolved.StartsWith((Join-Path $repo '.cache') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe staging path' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    $archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    $asset = "GameOCR-$runtime-cuda-windows-x64-$($archiveHash.Substring(0,12)).zip"
    $assetPath = Join-Path $repo "artifacts/$asset"
    Move-Item -LiteralPath $archive -Destination $assetPath -Force
    $package = @{
        id="gameocr-$runtime-cuda"; name="GameOCR $runtime CUDA"; kind=$runtime;
        version=$workerHash.Substring(0,12); runtimes=@($runtime); required=@($binary);
        url="https://github.com/suhli/gameocr/releases/download/$ReleaseTag/$asset";
        sha256=$archiveHash; size=(Get-Item -LiteralPath $assetPath).Length
    }
    $packages += $package
    $checksums += "$archiveHash  $asset"
    Write-Output "Prepared $asset ($($package.size) bytes)."
}
if (!$ReleaseTag) {
    # Both independent components determine the release identity. Updating only
    # translation must not reuse an older OCR-only companion tag.
    $digest = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes(($packages.sha256 -join ':')))
    $ReleaseTag = 'gpu-' + [Convert]::ToHexString($digest).ToLowerInvariant().Substring(0,12)
}
foreach ($package in $packages) {
    $asset = [IO.Path]::GetFileName(([Uri]$package.url).AbsolutePath)
    $package.url = "https://github.com/suhli/gameocr/releases/download/$ReleaseTag/$asset"
}
$manifest = Join-Path $repo 'artifacts/gpu-backends.json'
@{packages=$packages} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifest -Encoding utf8NoBOM
$env:GAMEOCR_GPU_MANIFEST = $manifest
if ($env:GITHUB_ENV) { "GAMEOCR_GPU_MANIFEST=$manifest" | Add-Content -LiteralPath $env:GITHUB_ENV }
$checksums | Set-Content -LiteralPath 'artifacts/SHA256SUMS-gpu-backends.txt' -Encoding utf8NoBOM
Write-Output "Manifest: $manifest. Publish both assets with release $ReleaseTag before distributing this base package."
& (Join-Path $PSScriptRoot 'verify-gpu-backends.ps1') -Manifest $manifest
