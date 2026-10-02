param(
    [Parameter(Mandatory = $true)][string]$DeployDir,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [Parameter(Mandatory = $true)][string]$IdentityName,
    [Parameter(Mandatory = $true)][string]$Publisher,
    [ValidateSet("x64", "arm64")][string]$Architecture = "x64"
)

$ErrorActionPreference = "Stop"

function Import-BatchEnvironment {
    param([Parameter(Mandatory = $true)][string]$BatchFile)

    $environmentLines = & $env:ComSpec /d /s /c "call `"$BatchFile`" >nul 2>&1 && set"
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "Failed to initialize the Visual Studio developer environment (exit code $exitCode): $BatchFile"
    }

    $pathCandidates = [Collections.Generic.List[string]]::new()
    foreach ($line in $environmentLines) {
        if ($line -notmatch '^([^=]+)=(.*)$') {
            continue
        }

        $name = $Matches[1]
        $value = $Matches[2]
        if ($name -ieq 'Path') {
            $pathCandidates.Add($value)
            continue
        }

        [Environment]::SetEnvironmentVariable($name, $value, 'Process')
    }

    # Some hosts expose both Path and PATH. Prefer the value produced by vcvars,
    # which contains the Windows SDK tools, rather than allowing casing to undo it.
    $developerPath = $pathCandidates |
        Where-Object { $_ -match '(?i)\\Windows Kits\\' } |
        Sort-Object Length -Descending |
        Select-Object -First 1
    if (-not $developerPath) {
        throw "Visual Studio developer environment did not provide a Windows SDK tools path: $BatchFile"
    }
    [Environment]::SetEnvironmentVariable('Path', $developerPath, 'Process')
}

function Resolve-MakeAppx {
    $command = Get-Command "makeappx.exe" -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $vsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path -LiteralPath $vsWhere)) {
        throw "MakeAppx.exe is not on PATH and Visual Studio Installer's vswhere.exe was not found: $vsWhere"
    }

    $vsPath = [string](& $vsWhere -latest -property installationPath)
    if ([string]::IsNullOrWhiteSpace($vsPath)) {
        throw "MakeAppx.exe is not on PATH and vswhere.exe did not find a Visual Studio installation"
    }

    $vcVars = Join-Path $vsPath.Trim() "VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path -LiteralPath $vcVars)) {
        throw "Visual Studio developer environment was not found: $vcVars"
    }

    Import-BatchEnvironment -BatchFile $vcVars
    $command = Get-Command "makeappx.exe" -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "MakeAppx.exe was not found after initializing the Visual Studio developer environment: $vcVars"
    }
    return $command.Source
}

$packageDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$temporaryRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$stagingDir = Join-Path $temporaryRoot "iDescriptor-msix"
$normalizedVersion = $Version.TrimStart('v')
if ($normalizedVersion -notmatch '^\d+\.\d+\.\d+$') {
    throw "MSIX version must be a three-part numeric version: $Version"
}
$msixVersion = "$normalizedVersion.0"

if (Test-Path -LiteralPath $stagingDir) {
    Remove-Item -LiteralPath $stagingDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stagingDir | Out-Null
Copy-Item -Path (Join-Path (Resolve-Path $DeployDir).Path '*') -Destination $stagingDir -Recurse -Force

$prohibitedScripts = Get-ChildItem -LiteralPath $stagingDir -Recurse -File |
    Where-Object { $_.Extension -in @('.ps1', '.bat', '.cmd') }
if ($prohibitedScripts) {
    $paths = ($prohibitedScripts.FullName -join ', ')
    throw "Microsoft Store staging contains prohibited installer scripts: $paths"
}

$makeAppx = Resolve-MakeAppx

$manifest = Get-Content -LiteralPath (Join-Path $packageDir "AppxManifest.xml.in") -Raw
$manifest = $manifest.Replace('@IDENTITY_NAME@', $IdentityName)
$manifest = $manifest.Replace('@PUBLISHER@', $Publisher)
$manifest = $manifest.Replace('@VERSION@', $msixVersion)
$manifest = $manifest.Replace('@ARCHITECTURE@', $Architecture)
[IO.File]::WriteAllText((Join-Path $stagingDir "AppxManifest.xml"), $manifest, [Text.UTF8Encoding]::new($false))

$assetDir = Join-Path $stagingDir "Assets"
New-Item -ItemType Directory -Force -Path $assetDir | Out-Null
$assetSourceDir = Join-Path $packageDir "Assets"
$requiredAssets = @(
    "StoreLogo.png",
    "Square44x44Logo.png",
    "Square150x150Logo.png",
    "Square310x310Logo.png",
    "Wide310x150Logo.png"
)
foreach ($asset in $requiredAssets) {
    $source = Join-Path $assetSourceDir $asset
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Required MSIX asset was not found: $source"
    }
    Copy-Item -LiteralPath $source -Destination (Join-Path $assetDir $asset) -Force
}

$resolvedOutputPath = [IO.Path]::GetFullPath($OutputPath)
$outputDirectory = Split-Path -Parent $resolvedOutputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
& $makeAppx pack /d $stagingDir /p $resolvedOutputPath /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed with exit code $LASTEXITCODE" }
