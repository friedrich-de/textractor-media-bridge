param(
    [Parameter(Mandatory = $true)]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [ValidateSet("x86", "x64")]
    [string]$Arch,

    [Parameter(Mandatory = $true)]
    [string]$Version,

    [string]$OutputDir = "dist\release",
    [string]$FfmpegPath = ""
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
$ReleaseDir = Join-Path $RepoRoot $OutputDir
$PackageName = "textractor-media-bridge-$Version-$Arch"
$PackageDir = Join-Path $ReleaseDir $PackageName
$TargetReleaseDir = Join-Path $RepoRoot "target\$Target\release"
$ServerExe = Join-Path $TargetReleaseDir "textractor_bridge_server.exe"
$BridgeDll = Join-Path $TargetReleaseDir "textractor_bridge_dll.dll"

if (-not (Test-Path -LiteralPath $ServerExe -PathType Leaf)) {
    throw "Missing server executable: $ServerExe"
}

if (-not (Test-Path -LiteralPath $BridgeDll -PathType Leaf)) {
    throw "Missing Textractor extension DLL: $BridgeDll"
}

if (-not $FfmpegPath) {
    $FfmpegPath = Join-Path $RepoRoot "vendor\ffmpeg\ffmpeg.exe"
}

if (-not (Test-Path -LiteralPath $FfmpegPath -PathType Leaf)) {
    throw "Missing ffmpeg.exe: $FfmpegPath"
}

if (Test-Path -LiteralPath $PackageDir) {
    Remove-Item -LiteralPath $PackageDir -Recurse -Force
}

New-Item -ItemType Directory -Path $PackageDir -Force | Out-Null

$ExtensionName = if ($Arch -eq "x86") { "Textractor Media Bridge.xdll" } else { "Textractor Media Bridge.dll" }

Copy-Item -LiteralPath $ServerExe -Destination (Join-Path $PackageDir "textractor_bridge_server.exe")
Copy-Item -LiteralPath $BridgeDll -Destination (Join-Path $PackageDir $ExtensionName)
Copy-Item -LiteralPath $FfmpegPath -Destination (Join-Path $PackageDir "ffmpeg.exe")

$ZipPath = Join-Path $ReleaseDir "$PackageName.zip"
if (Test-Path -LiteralPath $ZipPath) {
    Remove-Item -LiteralPath $ZipPath -Force
}

Compress-Archive -LiteralPath $PackageDir -DestinationPath $ZipPath -CompressionLevel Optimal
Get-Item -LiteralPath $ZipPath | Select-Object FullName, Length
