param([string]$Destination = '.cache/native/cudnn')
$ErrorActionPreference='Stop'
$archive=Join-Path ([IO.Path]::GetFullPath($Destination)) 'cudnn-9.8.0.87.zip'
New-Item -ItemType Directory -Force -Path (Split-Path $archive) | Out-Null
if(!(Test-Path -LiteralPath $archive)) {
    Invoke-WebRequest 'https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-9.8.0.87_cuda12-archive.zip' -OutFile $archive
}
if((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'd8a23705e3884b137b7e05449fb2b61bfa524e7cfc3fda80743d633f423c6ce4'){throw 'cuDNN SHA-256 mismatch'}
Expand-Archive -LiteralPath $archive -DestinationPath $Destination -Force
$env:CUDNN_PATH=Join-Path ([IO.Path]::GetFullPath($Destination)) 'cudnn-windows-x86_64-9.8.0.87_cuda12-archive'
if($env:GITHUB_ENV){"CUDNN_PATH=$env:CUDNN_PATH" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8}
Write-Output $env:CUDNN_PATH
