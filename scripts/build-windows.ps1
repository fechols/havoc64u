param([string]$Distribution='Ubuntu-24.04', [string]$OutputDirectory='')
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$linuxRoot=(& wsl -d $Distribution -- wslpath -a -u $projectRoot).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Unable to locate project in WSL.' }
& wsl -d $Distribution -- bash "$linuxRoot/scripts/build.sh"
if ($LASTEXITCODE -ne 0) { throw 'C64 build failed; no output copied.' }
if ($OutputDirectory) {
    $destination=$ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
    New-Item -ItemType Directory -Force -Path $destination | Out-Null
    foreach ($name in @('havoc.prg','havoc.reu','havoc.cfg','havoc.elf','memory-map.txt')) {
        Copy-Item -LiteralPath (Join-Path $projectRoot "dist/$name") -Destination (Join-Path $destination $name)
    }
}
