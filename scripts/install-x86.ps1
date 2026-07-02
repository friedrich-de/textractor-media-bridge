param(
    [string]$InstallRoot = "C:\Users\ald\Documents\x86",
    [switch]$NoRestart
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
$SourceServer = Join-Path $RepoRoot "target\release\textractor_bridge_server.exe"
$SourceBridgeDll = Join-Path $RepoRoot "target\i686-pc-windows-msvc\release\textractor_bridge_dll.dll"
$SourceFfmpeg = Join-Path $RepoRoot "vendor\ffmpeg\ffmpeg.exe"
if (-not (Test-Path -LiteralPath $SourceFfmpeg -PathType Leaf)) {
    $SourceFfmpeg = Join-Path $RepoRoot "vendor\ffmpeg\bin\ffmpeg.exe"
}
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
$ServerPath = Join-Path $InstallRoot "textractor_bridge_server.exe"
$BridgeDllPath = Join-Path $InstallRoot "Textractor Media Bridge.xdll"
$LegacyBridgeDllPath = Join-Path $InstallRoot "Textractor Media Bridge.dll"
$FfmpegPath = Join-Path $InstallRoot "ffmpeg.exe"
$WebRoot = Join-Path $InstallRoot "web_ui"
$ServerStdoutLogPath = Join-Path $InstallRoot "textractor_bridge_server.stdout.log"
$ServerStderrLogPath = Join-Path $InstallRoot "textractor_bridge_server.stderr.log"
$ServerSessionPath = Join-Path $InstallRoot "textractor_bridge_server.session.json"

if (-not (Test-Path -LiteralPath $SourceServer -PathType Leaf)) {
    throw "Missing $SourceServer. Run: cargo build -p textractor_bridge_server --release"
}

if (-not (Test-Path -LiteralPath $SourceBridgeDll -PathType Leaf)) {
    throw "Missing $SourceBridgeDll. Run: cargo build -p textractor_bridge_dll --release --target i686-pc-windows-msvc"
}

if (-not (Test-Path -LiteralPath $SourceFfmpeg -PathType Leaf)) {
    throw "Missing ffmpeg.exe under vendor\ffmpeg"
}

if (-not (Test-Path -LiteralPath $InstallRoot -PathType Container)) {
    throw "Install root does not exist: $InstallRoot"
}

$ResolvedInstallRoot = (Resolve-Path -LiteralPath $InstallRoot).Path
$InstallRootPrefix = $ResolvedInstallRoot.TrimEnd("\") + "\"

function Assert-UnderInstallRoot {
    param([string]$Path)
    $FullPath = [System.IO.Path]::GetFullPath($Path)
    if (
        $FullPath -ne $ResolvedInstallRoot -and
        -not $FullPath.StartsWith($InstallRootPrefix, [System.StringComparison]::OrdinalIgnoreCase)
    ) {
        throw "Refusing to touch path outside install root: $FullPath"
    }
}

Assert-UnderInstallRoot $ServerPath
Assert-UnderInstallRoot $BridgeDllPath
Assert-UnderInstallRoot $LegacyBridgeDllPath
Assert-UnderInstallRoot $FfmpegPath
Assert-UnderInstallRoot $WebRoot
Assert-UnderInstallRoot $ServerStdoutLogPath
Assert-UnderInstallRoot $ServerStderrLogPath
Assert-UnderInstallRoot $ServerSessionPath

$ExistingServer = Get-CimInstance Win32_Process |
    Where-Object { $_.ExecutablePath -eq $ServerPath }
foreach ($Process in $ExistingServer) {
    Stop-Process -Id $Process.ProcessId -Force
}

Start-Sleep -Milliseconds 700
Copy-Item -LiteralPath $SourceServer -Destination $ServerPath -Force
Copy-Item -LiteralPath $SourceBridgeDll -Destination $BridgeDllPath -Force
if (Test-Path -LiteralPath $LegacyBridgeDllPath) {
    Remove-Item -LiteralPath $LegacyBridgeDllPath -Force
}
Copy-Item -LiteralPath $SourceFfmpeg -Destination $FfmpegPath -Force

if (Test-Path -LiteralPath $WebRoot) {
    Assert-UnderInstallRoot $WebRoot
    Remove-Item -LiteralPath $WebRoot -Recurse -Force
}

if (-not $NoRestart) {
    Remove-Item -LiteralPath $ServerStdoutLogPath, $ServerStderrLogPath -Force -ErrorAction SilentlyContinue
    Start-Process `
        -FilePath $ServerPath `
        -WorkingDirectory $InstallRoot `
        -WindowStyle Hidden `
        -RedirectStandardOutput $ServerStdoutLogPath `
        -RedirectStandardError $ServerStderrLogPath
    Start-Sleep -Milliseconds 800
}

Get-Item -LiteralPath $ServerPath | Select-Object FullName, Length
Get-Item -LiteralPath $BridgeDllPath | Select-Object FullName, Length
Get-Item -LiteralPath $FfmpegPath | Select-Object FullName, Length
[pscustomobject]@{
    WebUiFolderExists = Test-Path -LiteralPath $WebRoot
    EmbeddedWebUi = $true
    ServerStdoutLog = $ServerStdoutLogPath
    ServerStderrLog = $ServerStderrLogPath
    ServerSessionInfo = $ServerSessionPath
}
