#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$SetupKey = "",
    [string]$SetupKeyFile = "",
    [string]$ManagementUrl = "",
    [string]$Version = "",
    [string]$BaseUrl = "",
    [switch]$Force,
    [switch]$Uninstall,
    [switch]$DryRun,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

$WarpVersion = "0.1.3"
$NetbirdVersion = "0.80.0"
$DefaultBaseUrl = "https://github.com/dayu-sec/warp-ztna-client/releases/download"
$ServiceName = "warp-ztna"
$ProgramRoot = if ($env:ProgramFiles) { $env:ProgramFiles } else { "C:\Program Files" }
$DataRoot = if ($env:ProgramData) { $env:ProgramData } else { "C:\ProgramData" }
$InstallDir = "$ProgramRoot\Warp ZTNA"
$StateDir = "$DataRoot\warp-ztna"
$LogFile = "$StateDir\client.log"

function Write-Note {
    param([string]$Message)
    Write-Host $Message
}

function Fail {
    param([string]$Message)
    Write-Error "错误：$Message"
    exit 1
}

function Show-Usage {
    @"
用法：install.ps1 [选项]

在 Windows 机器上安装 Warp 路由节点：装引擎（netbird）+ 注册 warp-ztna 服务 + 可选入网。
请以管理员身份运行 PowerShell。

选项：
  -SetupKey <KEY>         机器入网凭据，一次性使用
  -SetupKeyFile <PATH>    从文件读取机器入网凭据，优先于 -SetupKey
  -ManagementUrl <URL>    Management 地址；带 setup key 入网时必填（默认取环境变量 WARP_ZTNA_MANAGEMENT_URL）
  -Version <x.y.z>        覆盖安装的包版本，默认脚本内嵌版本
  -BaseUrl <URL>          产物基址，默认 https://github.com/dayu-sec/warp-ztna-client/releases/download
  -Force                  （兼容保留：遇同类组网客户端现在一律先替换，无需开关）
  -Uninstall              卸载服务与二进制；state 保留
  -DryRun                 只打印计划，不下载不安装
  -Help                   显示本帮助
"@ | Write-Host
}

function Get-Arch {
    switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
        "X64" { return "amd64" }
        "Arm64" { return "arm64" }
        default { Fail "不支持的架构：$([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)" }
    }
}

function Get-OfficialNetbird {
    $command = Get-Command "netbird.exe" -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $candidates = @((Join-Path $env:ProgramFiles "NetBird\netbird.exe"))
    $programFilesX86 = ${env:ProgramFiles(x86)}
    if ($programFilesX86) { $candidates += (Join-Path $programFilesX86 "NetBird\netbird.exe") }
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return $candidate }
    }
    return $null
}

function Get-RemoteFile {
    param([string]$Uri, [string]$OutFile)
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Invoke-WebRequest -Uri $Uri -OutFile $OutFile
            return
        } catch {
            if ($attempt -eq 3) { Fail "下载失败（已重试 3 次）：$Uri —— $($_.Exception.Message)" }
            Write-Note "下载中断，重试（$attempt/3）：$Uri"
            Start-Sleep -Seconds 2
        }
    }
}

function Get-NetBirdMsiProducts {
    $roots = @(
        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        foreach ($key in Get-ChildItem -Path $root -ErrorAction SilentlyContinue) {
            $props = Get-ItemProperty -Path $key.PSPath -ErrorAction SilentlyContinue
            if ($props -and $props.DisplayName -like "*NetBird*") { $props.PSChildName }
        }
    }
}

function Replace-Official {
    param([string]$ExistingBinary)
    Write-Note "替换同类组网客户端（${ExistingBinary}）：停用并移除其服务、命令与桌面应用；其 state 目录保留。"
    foreach ($svc in @(Get-Service -Name "*NetBird*" -ErrorAction SilentlyContinue)) {
        Stop-Service -Name $svc.Name -Force -ErrorAction SilentlyContinue
    }
    if ($ExistingBinary -and (Test-Path $ExistingBinary)) {
        & $ExistingBinary service uninstall
    }
    Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "*netbird*" } | Stop-Process -Force -ErrorAction SilentlyContinue
    $uninstaller = Join-Path $env:ProgramFiles "NetBird\netbird_uninstall.exe"
    $products = @(Get-NetBirdMsiProducts)
    if (Test-Path $uninstaller) {
        $proc = Start-Process -FilePath $uninstaller -ArgumentList @("/S") -Wait -PassThru
        if ($proc.ExitCode -ne 0) { Write-Warning "官方卸载器退出码 $($proc.ExitCode)" }
    } elseif ($products.Count -gt 0) {
        foreach ($product in $products) {
            $proc = Start-Process -FilePath msiexec.exe -ArgumentList @("/x", $product, "/qn", "/norestart") -Wait -PassThru
            if ($proc.ExitCode -ne 0) { Write-Warning "msiexec 退出码 $($proc.ExitCode)" }
        }
    }
    $netbirdDir = Join-Path $env:ProgramFiles "NetBird"
    if (Test-Path $netbirdDir) { Remove-Item -Path $netbirdDir -Recurse -Force -ErrorAction SilentlyContinue }
}

function Show-Plan {
    Write-Host "[dry-run] 架构 $Arch"
    Write-Host "[dry-run] 引擎归档 $ArchiveUrl"
    Write-Host "[dry-run] 校验和 $ChecksumsUrl"
    Write-Host "[dry-run] 安装目录 $InstallDir；状态目录 $StateDir；日志 $LogFile"
    Write-Host "[dry-run] （本机有同类组网客户端时：先替换它——停用并移除其服务、命令与桌面应用）"
    Write-Host "[dry-run] netbird.exe service install --service $ServiceName --service-env NB_STATE_DIR=$StateDir,NB_ENABLE_LOCAL_FORWARDING=true"
    Write-Host "[dry-run] netbird.exe up --setup-key-file <文件> --management-url <URL>（未提供凭据时打印两条命令）"
}

function Install-Engine {
    Write-Note "下载引擎归档：$ArchiveUrl"
    $work = Join-Path ([IO.Path]::GetTempPath()) ("warp-ztna-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    try {
        $archive = Join-Path $work $Artifact
        Get-RemoteFile -Uri $ArchiveUrl -OutFile $archive
        $checksums = Join-Path $work "checksums.txt"
        Get-RemoteFile -Uri $ChecksumsUrl -OutFile $checksums
        $expectedLine = Get-Content $checksums | Where-Object { $_ -match "\s$([Regex]::Escape($Artifact))$" } | Select-Object -First 1
        if (-not $expectedLine) { Fail "checksums.txt 里找不到 $Artifact 的校验值" }
        $expectedHash = ($expectedLine -split "\s+")[0]
        $actualHash = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash
        if ($actualHash.ToLower() -ne $expectedHash.ToLower()) {
            Fail "校验失败：$Artifact 期望 $expectedHash，实际 $actualHash"
        }
        Write-Note "校验通过：$Artifact"
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        New-Item -ItemType Directory -Path $StateDir -Force | Out-Null
        tar -xzf $archive -C $work
        $binary = Join-Path $work "netbird.exe"
        if (-not (Test-Path $binary)) { Fail "归档里没有 netbird.exe：$archive" }
        Copy-Item -Path $binary -Destination (Join-Path $InstallDir "netbird.exe") -Force
        $script:EngineBinary = Join-Path $InstallDir "netbird.exe"
    } finally {
        Remove-Item -Path $work -Recurse -Force -ErrorAction SilentlyContinue
    }
    $existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if ($existing) {
        Write-Note "检测到既有 $ServiceName 服务：先停用并注销再重装（state 保留）。"
        & $EngineBinary service stop --service $ServiceName
        & $EngineBinary service uninstall --service $ServiceName
    }
    & $EngineBinary service install --service $ServiceName --log-file $LogFile --log-level info --service-env "NB_STATE_DIR=$StateDir,NB_ENABLE_LOCAL_FORWARDING=true"
    if ($LASTEXITCODE -ne 0) { Fail "服务注册失败：netbird service install 退出码 $LASTEXITCODE" }
    & $EngineBinary service start --service $ServiceName
    if ($LASTEXITCODE -ne 0) { Fail "服务启动失败：netbird service start 退出码 $LASTEXITCODE" }
}

function Start-Enroll {
    $key = $SetupKey
    if ($SetupKeyFile) {
        if (-not (Test-Path $SetupKeyFile)) { Fail "找不到 -SetupKeyFile：$SetupKeyFile" }
        $key = (Get-Content -Raw $SetupKeyFile).Trim()
    }
    if (-not $key) {
        Write-Note "未请求入网。装完后的两条引擎命令（原文，不新增命令面）："
        Write-Note "  机器入网：$EngineBinary up --setup-key-file <文件> --management-url <URL>"
        Write-Note "  交互登录：$EngineBinary login --no-browser"
        return
    }
    $keyFile = [IO.Path]::GetTempFileName()
    try {
        Set-Content -Path $keyFile -Value $key -NoNewline
        $arguments = @("up", "--setup-key-file", $keyFile, "--management-url", $ManagementUrl)
        & $EngineBinary @arguments
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "入网命令未成功（引擎与服务已装好）；请稍后重试 $EngineBinary up，或检查凭据。"
        } else {
            Write-Note "已发起入网；用 $EngineBinary status 复查连接状态。"
        }
    } finally {
        Remove-Item -Path $keyFile -Force -ErrorAction SilentlyContinue
    }
}

function Remove-Install {
    $binary = Join-Path $InstallDir "netbird.exe"
    if (Test-Path $binary) {
        & $binary service stop --service $ServiceName
        & $binary service uninstall --service $ServiceName
    } else {
        Write-Note "未发现 $InstallDir\netbird.exe，跳过服务注销。"
    }
    if (Test-Path $InstallDir) { Remove-Item -Path $InstallDir -Recurse -Force }
    Write-Note "state 已保留（$StateDir）；如需一并删除请手动移除该目录。"
    Write-Note "脚本不触碰 Management 里的 peer 记录；摘 Router 与删 peer 是运维动作。"
}

if ($Help) {
    Show-Usage
    exit 0
}

$Arch = Get-Arch
if (-not $Version) { $Version = $WarpVersion }
if (-not $BaseUrl) {
    $BaseUrl = if ($env:WARP_ZTNA_BASE_URL) { $env:WARP_ZTNA_BASE_URL } else { $DefaultBaseUrl }
}
if (-not $ManagementUrl -and $env:WARP_ZTNA_MANAGEMENT_URL) { $ManagementUrl = $env:WARP_ZTNA_MANAGEMENT_URL }
if (($SetupKey -or $SetupKeyFile) -and -not $ManagementUrl) {
    Fail "setup key 入网必须给 -ManagementUrl（或环境变量 WARP_ZTNA_MANAGEMENT_URL）：不给时引擎会打它内置的公有云默认地址（不是本部署的管理服务器），本处签发的 key 在那边无效"
}
$Base = $BaseUrl.TrimEnd("/")
$Artifact = "warp-ztna_${Version}_windows_${Arch}.tar.gz"
$ArchiveUrl = "$Base/client-v$Version/$Artifact"
$ChecksumsUrl = "$Base/client-v$Version/checksums.txt"
$EngineBinary = "$InstallDir\netbird.exe"

if ($DryRun) {
    Show-Plan
    exit 0
}

if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) { Fail "本脚本只能在 Windows 上运行" }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Fail "需要管理员权限：请以管理员身份运行 PowerShell"
}

if ($Uninstall) {
    Remove-Install
    exit 0
}

$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if (-not $service) {
    $existing = Get-OfficialNetbird
    if ($existing) {
        Replace-Official -ExistingBinary $existing
    }
}

Install-Engine
Start-Enroll
