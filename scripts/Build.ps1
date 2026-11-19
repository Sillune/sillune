param(
    [string] $Trunk = 'trunk',
    [switch] $Offline
)

$ErrorActionPreference = 'Stop'
$project = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$output = Join-Path $project 'out'
$config = Get-Content -LiteralPath (Join-Path $project 'desktop\tauri.conf.json') -Raw | ConvertFrom-Json
$version = $config.version
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
$env:CARGO_ENCODED_RUSTFLAGS = @("--remap-path-prefix=$project=sillune", "--remap-path-prefix=$env:USERPROFILE=windows-profile") -join [char]31

Push-Location (Join-Path $project 'ui')
try {
    $trunkArgs = @('build', '--release', '--locked')
    if ($Offline) { $trunkArgs += '--offline' }
    & $Trunk @trunkArgs
    if ($LASTEXITCODE -ne 0) { throw "Échec de la compilation de l’interface" }
} finally {
    Pop-Location
}

Push-Location $project
try {
    $cargoArgs = @('build', '--release', '--locked', '-p', 'sillune')
    if ($Offline) { $cargoArgs += '--offline' }
    cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw 'Échec de la compilation de Sillune' }
} finally {
    Pop-Location
}

$package = Join-Path $output 'Sillune'
New-Item -ItemType Directory -Force -Path $package | Out-Null
Copy-Item -LiteralPath (Join-Path $project 'target\release\sillune.exe') -Destination (Join-Path $package 'Sillune.exe') -Force
Copy-Item -LiteralPath (Join-Path $project 'LICENSE') -Destination (Join-Path $package 'LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $project 'README.md') -Destination (Join-Path $package 'LISEZ-MOI.md') -Force
$releaseFiles = @('Sillune.exe', 'LICENSE.txt', 'LISEZ-MOI.md')
$skillCatalog = Get-Content -LiteralPath (Join-Path $project 'skills/catalog.json') -Raw | ConvertFrom-Json
foreach ($entry in $skillCatalog) {
    $name = $entry.id
    if ($name -notmatch '^[a-z0-9-]+$') { throw 'Identifiant de skill invalide' }
    $dest = Join-Path $package "Licences tierces\$name"
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item -LiteralPath (Join-Path $project "skills\$name\LICENSE.txt"),(Join-Path $project "skills\$name\SOURCE.txt") -Destination $dest -Force
    $releaseFiles += @("Licences tierces/$name/LICENSE.txt", "Licences tierces/$name/SOURCE.txt")
}
$archive = Join-Path $output "Sillune-$version-windows-x64.zip"
Add-Type -AssemblyName System.IO.Compression.FileSystem
$temporary = "$archive.$([guid]::NewGuid().ToString('N')).tmp"
try {
    $zip = [System.IO.Compression.ZipFile]::Open($temporary, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($file in $releaseFiles) {
            [void] [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $zip, (Join-Path $package $file), "Sillune/$file",
                [System.IO.Compression.CompressionLevel]::Optimal
            )
        }
    } finally {
        $zip.Dispose()
    }
    Move-Item -LiteralPath $temporary -Destination $archive -Force
} finally {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
}
$hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $(Split-Path -Leaf $archive)" | Set-Content -LiteralPath "$archive.sha256" -Encoding ascii
Write-Output $archive
