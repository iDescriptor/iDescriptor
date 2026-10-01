param(
    [Parameter(Mandatory = $true)][string]$DeployDir,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [ValidateSet("x64", "arm64")][string]$Architecture = "x64"
)

$ErrorActionPreference = "Stop"
$packageDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $packageDir "..\..\..")).Path
$deployRoot = (Resolve-Path $DeployDir).Path
$temporaryRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$generatedPath = Join-Path $temporaryRoot "iDescriptor-Files.wxs"

function Get-Sha256Bytes([string]$Value) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Value.ToLowerInvariant())
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return $sha256.ComputeHash($bytes)
    }
    finally {
        $sha256.Dispose()
    }
}

function ConvertTo-Hex([byte[]]$Bytes) {
    return [BitConverter]::ToString($Bytes).Replace('-', '')
}

function Get-RelativePath([string]$BasePath, [string]$Path) {
    $baseFullPath = [IO.Path]::GetFullPath($BasePath).TrimEnd('\', '/')
    $baseUri = [Uri]($baseFullPath + [IO.Path]::DirectorySeparatorChar)
    $pathUri = [Uri]([IO.Path]::GetFullPath($Path))
    return [Uri]::UnescapeDataString($baseUri.MakeRelativeUri($pathUri).ToString()).Replace('/', '\')
}

function Get-StableId([string]$Prefix, [string]$Value) {
    $hash = Get-Sha256Bytes $Value
    return $Prefix + ((ConvertTo-Hex $hash).Substring(0, 24))
}

function ConvertTo-Guid([string]$Value) {
    $hash = Get-Sha256Bytes $Value
    $guidBytes = New-Object byte[] 16
    [Array]::Copy($hash, $guidBytes, 16)
    return (New-Object Guid (,$guidBytes)).ToString().ToUpperInvariant()
}

function Escape-Xml([string]$Value) {
    return [Security.SecurityElement]::Escape($Value)
}

$files = Get-ChildItem -LiteralPath $deployRoot -File -Recurse | Sort-Object FullName
if (-not $files) {
    throw "Deployment directory is empty: $deployRoot"
}

$directories = @{}
foreach ($file in $files) {
    $relative = Get-RelativePath $deployRoot $file.FullName
    $relativeDirectory = [IO.Path]::GetDirectoryName($relative)
    while ($relativeDirectory) {
        $directories[$relativeDirectory] = $true
        $parent = [IO.Path]::GetDirectoryName($relativeDirectory)
        if ($parent -eq $relativeDirectory) { break }
        $relativeDirectory = $parent
    }
}

$lines = [Collections.Generic.List[string]]::new()
$lines.Add('<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">')
$lines.Add('  <Fragment>')
$lines.Add('    <DirectoryRef Id="INSTALLFOLDER">')

function Add-DirectoryTree([string]$ParentPath, [int]$Indent) {
    $children = $directories.Keys | Where-Object {
        [IO.Path]::GetDirectoryName($_) -eq $ParentPath
    } | Sort-Object
    foreach ($child in $children) {
        $name = [IO.Path]::GetFileName($child)
        $id = Get-StableId "Dir" $child
        $spaces = ' ' * $Indent
        $lines.Add("$spaces<Directory Id=`"$id`" Name=`"$(Escape-Xml $name)`">")
        Add-DirectoryTree $child ($Indent + 2)
        $lines.Add("$spaces</Directory>")
    }
}

Add-DirectoryTree "" 6
$lines.Add('    </DirectoryRef>')
$lines.Add('  </Fragment>')
$lines.Add('  <Fragment>')
$lines.Add('    <ComponentGroup Id="ApplicationFiles">')

foreach ($file in $files) {
    $relative = Get-RelativePath $deployRoot $file.FullName
    $relativeDirectory = [IO.Path]::GetDirectoryName($relative)
    $directoryId = if ($relativeDirectory) { Get-StableId "Dir" $relativeDirectory } else { "INSTALLFOLDER" }
    $componentId = Get-StableId "Cmp" $relative
    $fileId = if ($relative -ieq "idescriptor.exe") { "MainExecutable" } else { Get-StableId "File" $relative }
    $source = Escape-Xml $file.FullName
    $guid = ConvertTo-Guid $relative
    $lines.Add("      <Component Id=`"$componentId`" Directory=`"$directoryId`" Guid=`"$guid`">")
    $lines.Add("        <File Id=`"$fileId`" Source=`"$source`" KeyPath=`"yes`" Checksum=`"yes`" />")
    if ($fileId -eq "MainExecutable") {
        $lines.Add('        <Shortcut Id="StartMenuShortcut" Directory="ApplicationProgramsFolder" Name="iDescriptor" Target="[#MainExecutable]" WorkingDirectory="INSTALLFOLDER" Advertise="no" />')
        $lines.Add('        <Shortcut Id="DesktopShortcut" Directory="DesktopFolder" Name="iDescriptor" Target="[#MainExecutable]" WorkingDirectory="INSTALLFOLDER" Advertise="no" />')
    }
    $lines.Add('      </Component>')
}

$lines.Add('    </ComponentGroup>')
$lines.Add('  </Fragment>')
$lines.Add('</Wix>')
[IO.File]::WriteAllLines($generatedPath, $lines, [Text.UTF8Encoding]::new($false))

$normalizedVersion = $Version.TrimStart('v')
if ($normalizedVersion -notmatch '^\d+\.\d+\.\d+$') {
    throw "MSI version must be a three-part numeric version: $Version"
}

$resolvedOutputPath = [IO.Path]::GetFullPath($OutputPath)
$outputDirectory = Split-Path -Parent $resolvedOutputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null

$globalToolWix = if ($env:USERPROFILE) { Join-Path $env:USERPROFILE ".dotnet\tools\wix.exe" } else { $null }
if ($globalToolWix -and (Test-Path -LiteralPath $globalToolWix -PathType Leaf)) {
    $wixExecutable = $globalToolWix
}
else {
    $wixCommand = Get-Command "wix.exe" -CommandType Application -ErrorAction SilentlyContinue |
        Where-Object { $_.Source -notlike "*\Microsoft\WindowsApps\wix.exe" } |
        Select-Object -First 1
    $wixExecutable = if ($wixCommand) { $wixCommand.Source } else { $null }
}

if (-not $wixExecutable) {
    throw "WiX 4 was not found. Install it with 'dotnet tool install --global wix --version 4.0.6' and add the required UI and Util extensions."
}

Write-Host "Using WiX: $wixExecutable"
$utilBinary = if ($Architecture -eq "arm64") { "Wix4UtilCA_ARM64" } else { "Wix4UtilCA_X64" }

& $wixExecutable build `
    (Join-Path $packageDir "Product.wxs") `
    $generatedPath `
    -arch $Architecture `
    -ext WixToolset.UI.wixext `
    -ext WixToolset.Util.wixext `
    -d "Version=$normalizedVersion" `
    -d "MsiResources=$(Join-Path $packageDir 'resources')" `
    -d "SharedResources=$(Join-Path $repoRoot 'packaging\shared\resources\app-icon')" `
    -d "WixUtilBinary=$utilBinary" `
    -o $resolvedOutputPath
$wixExitCode = $LASTEXITCODE
if ($null -eq $wixExitCode) {
    throw "WiX did not run as a native executable: $wixExecutable"
}
if ($wixExitCode -ne 0) {
    throw "WiX failed with exit code $wixExitCode"
}

if (-not (Test-Path -LiteralPath $resolvedOutputPath)) {
    throw "WiX did not create the expected MSI: $resolvedOutputPath"
}
