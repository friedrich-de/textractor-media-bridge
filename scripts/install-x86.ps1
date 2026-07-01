param(
    [string]$InstallRoot = "C:\Users\ald\Documents\x86",
    [switch]$NoRestart
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
$SourceServer = Join-Path $RepoRoot "target\release\textractor_bridge_server.exe"
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
$ServerPath = Join-Path $InstallRoot "textractor_bridge_server.exe"
$WebRoot = Join-Path $InstallRoot "web_ui"

if (-not (Test-Path -LiteralPath $SourceServer -PathType Leaf)) {
    throw "Missing $SourceServer. Run: cargo build -p textractor_bridge_server --release"
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
Assert-UnderInstallRoot $WebRoot

$ExistingServer = Get-CimInstance Win32_Process |
    Where-Object { $_.ExecutablePath -eq $ServerPath }
foreach ($Process in $ExistingServer) {
    Stop-Process -Id $Process.ProcessId -Force
}

Start-Sleep -Milliseconds 700
Copy-Item -LiteralPath $SourceServer -Destination $ServerPath -Force

if (Test-Path -LiteralPath $WebRoot) {
    Assert-UnderInstallRoot $WebRoot
    Remove-Item -LiteralPath $WebRoot -Recurse -Force
}

if (-not $NoRestart) {
    Start-Process -FilePath $ServerPath -WorkingDirectory $InstallRoot -WindowStyle Hidden
    Start-Sleep -Milliseconds 800
}

Get-Item -LiteralPath $ServerPath | Select-Object FullName, Length
[pscustomobject]@{
    WebUiFolderExists = Test-Path -LiteralPath $WebRoot
    EmbeddedWebUi = $true
}
